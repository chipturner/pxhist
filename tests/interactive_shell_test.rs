use std::{fs, thread, time::Duration};

use pxh::test_utils::PxhTestHelper;
use rexpect::session::spawn_command;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

// Shell type enum for parameterized tests
#[derive(Clone, Copy)]
enum Shell {
    Bash,
    Zsh,
}

impl Shell {
    fn name(&self) -> &'static str {
        match self {
            Shell::Bash => "bash",
            Shell::Zsh => "zsh",
        }
    }

    fn rc_file(&self) -> &'static str {
        match self {
            Shell::Bash => ".bashrc",
            Shell::Zsh => ".zshrc",
        }
    }

    fn is_available(&self) -> bool {
        which::which(self.name()).is_ok()
    }
}

// Helper to count commands in a database using pxh
fn count_commands(helper: &PxhTestHelper) -> Result<usize> {
    let output = helper.command_with_args(&["show", "--suppress-headers"]).output()?;

    if !output.status.success() {
        return Ok(0);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().count())
}

// Helper to get commands from database using pxh export
fn get_commands(helper: &PxhTestHelper) -> Result<Vec<String>> {
    let output = helper.command_with_args(&["export"]).output()?;

    if !output.status.success() {
        return Ok(vec![]);
    }

    let invocations: Vec<pxh::Invocation> = serde_json::from_slice(&output.stdout)?;
    Ok(invocations.into_iter().map(|inv| inv.command.to_string()).collect())
}

#[test]
fn test_bash_interactive_shell() -> Result<()> {
    // Create test helper
    let helper = PxhTestHelper::new();
    let home_dir = helper.home_dir();
    let bashrc_path = home_dir.join(".bashrc");
    let pxh_db_path = helper.db_path();

    // Create empty .bashrc
    fs::write(&bashrc_path, "")?;

    // First, install pxh for bash
    let install_output = helper.command_with_args(&["install", "bash"]).output()?;

    eprintln!("Install output stdout: {}", String::from_utf8_lossy(&install_output.stdout));
    eprintln!("Install output stderr: {}", String::from_utf8_lossy(&install_output.stderr));

    assert!(
        install_output.status.success(),
        "Install failed: {}",
        String::from_utf8_lossy(&install_output.stderr)
    );

    // Verify bashrc was modified
    let bashrc_content = fs::read_to_string(&bashrc_path)?;
    eprintln!("Bashrc content after install:\n{}", bashrc_content);
    assert!(bashrc_content.contains("pxh shell-config bash"));

    // Now spawn an interactive bash session with proper environment
    let cmd = helper.shell_command("bash");
    eprintln!("Spawning bash with command: {:?}", cmd);
    let mut session = spawn_command(cmd, Some(30_000))?;

    // Wait for shell initialization and rc file loading
    thread::sleep(Duration::from_millis(1000));

    // Check if pxh is available
    session.send_line("which pxh")?;
    session.exp_regex(r"(/[^\r\n]+/pxh)")?;

    // Check environment variables
    session.send_line("echo PXH_DB_PATH=$PXH_DB_PATH")?;
    session.exp_string(&format!("PXH_DB_PATH={}", pxh_db_path.display()))?;

    // Give shell more time to initialize with preexec/precmd
    thread::sleep(Duration::from_millis(1000));

    // Run some test commands
    session.send_line("echo 'Hello from interactive bash'")?;
    session.exp_string("Hello from interactive bash")?;

    session.send_line("pwd")?;
    session.exp_regex(r"(/[^\r\n]+)")?;

    session.send_line("ls /tmp > /dev/null 2>&1")?;
    thread::sleep(Duration::from_millis(100));

    // Run a command that will fail
    session.send_line("false")?;
    thread::sleep(Duration::from_millis(100));

    // Exit the shell
    session.send_line("exit")?;
    session.exp_eof()?;

    // Give a moment for any final writes
    thread::sleep(Duration::from_millis(500));

    // Now verify that commands were recorded
    let command_count = count_commands(&helper)?;

    // Debug: Check if the database exists and has content
    if command_count == 0 {
        eprintln!("Debug: No commands found. Checking database path: {:?}", pxh_db_path);
        eprintln!("Debug: Database exists: {}", pxh_db_path.exists());

        // Try running pxh show directly to see what's happening
        let show_output = helper.command_with_args(&["show", "--suppress-headers"]).output()?;
        eprintln!("Debug: pxh show exit status: {}", show_output.status);
        eprintln!("Debug: pxh show stdout: {}", String::from_utf8_lossy(&show_output.stdout));
        eprintln!("Debug: pxh show stderr: {}", String::from_utf8_lossy(&show_output.stderr));
    }

    assert!(
        command_count >= 4,
        "Expected at least 4 commands (echo, pwd, ls, false), found {}",
        command_count
    );

    let commands = get_commands(&helper)?;
    assert!(
        commands.iter().any(|c| c.contains("echo 'Hello from interactive bash'")),
        "Should have recorded echo command"
    );
    assert!(commands.iter().any(|c| c == "pwd"), "Should have recorded pwd command");
    assert!(commands.iter().any(|c| c.contains("ls /tmp")), "Should have recorded ls command");
    assert!(commands.iter().any(|c| c == "false"), "Should have recorded false command");

    // Also verify using pxh show command
    let show_output = helper.command_with_args(&["show", "--limit", "10"]).output()?;

    assert!(show_output.status.success(), "Show command should succeed");
    let history = String::from_utf8_lossy(&show_output.stdout);
    assert!(history.contains("Hello from interactive bash"), "History should contain echo command");

    Ok(())
}

#[test]
fn test_zsh_interactive_shell() -> Result<()> {
    // Skip test if zsh is not available
    if which::which("zsh").is_err() {
        eprintln!("Skipping zsh integration test: zsh not found in PATH");
        return Ok(());
    }

    // Create test helper
    let helper = PxhTestHelper::new();
    let home_dir = helper.home_dir();
    let zshrc_path = home_dir.join(".zshrc");

    // Create empty .zshrc
    fs::write(&zshrc_path, "")?;

    // Install pxh for zsh
    let install_output = helper.command_with_args(&["install", "zsh"]).output()?;

    assert!(
        install_output.status.success(),
        "Install failed: {}",
        String::from_utf8_lossy(&install_output.stderr)
    );

    // Verify zshrc was modified
    let zshrc_content = fs::read_to_string(&zshrc_path)?;
    assert!(zshrc_content.contains("pxh shell-config zsh"));

    // Spawn an interactive zsh session with proper environment
    let cmd = helper.shell_command("zsh");
    let mut session = spawn_command(cmd, Some(30_000))?;

    // Wait for shell initialization and rc file loading
    thread::sleep(Duration::from_millis(1000));

    // Run test commands
    session.send_line("echo 'Hello from interactive zsh'")?;
    session.exp_string("Hello from interactive zsh")?;

    session.send_line("date +%Y-%m-%d")?;
    session.exp_regex(r"\d{4}-\d{2}-\d{2}")?;

    session.send_line("cd /tmp && pwd")?;
    session.exp_string("/tmp")?;

    // Exit the shell
    session.send_line("exit")?;
    session.exp_eof()?;

    // Verify commands were recorded
    let command_count = count_commands(&helper)?;
    assert!(command_count >= 3, "Expected at least 3 commands, found {}", command_count);

    let commands = get_commands(&helper)?;
    assert!(
        commands.iter().any(|c| c.contains("echo 'Hello from interactive zsh'")),
        "Should have recorded echo command"
    );
    assert!(
        commands.iter().any(|c| c.contains("date +%Y-%m-%d")),
        "Should have recorded date command"
    );

    Ok(())
}

#[test]
fn test_bash_command_with_exit_status() -> Result<()> {
    // This test verifies that exit statuses are properly recorded
    let helper = PxhTestHelper::new();
    let home_dir = helper.home_dir();
    let bashrc_path = home_dir.join(".bashrc");

    fs::write(&bashrc_path, "")?;

    // Install pxh
    let install_output = helper.command_with_args(&["install", "bash"]).output()?;

    assert!(install_output.status.success());

    // Spawn bash session with proper environment
    let cmd = helper.shell_command("bash");
    let mut session = spawn_command(cmd, Some(30_000))?;

    // Wait for shell initialization
    thread::sleep(Duration::from_millis(1000));

    // Run a successful command
    session.send_line("true")?;
    thread::sleep(Duration::from_millis(100));

    // Run a failing command
    session.send_line("false")?;
    thread::sleep(Duration::from_millis(100));

    // Run a command with specific exit code
    session.send_line("exit 42")?;
    session.exp_eof()?;

    // Check the database for exit statuses using pxh export
    let output = helper.command_with_args(&["export"]).output()?;
    assert!(output.status.success(), "Export should succeed");

    let invocations: Vec<pxh::Invocation> = serde_json::from_slice(&output.stdout)?;

    // Verify exit statuses
    assert!(
        invocations.iter().any(|inv| inv.command == "true" && inv.exit_status == Some(0)),
        "true command should have exit status 0"
    );
    assert!(
        invocations.iter().any(|inv| inv.command == "false" && inv.exit_status == Some(1)),
        "false command should have exit status 1"
    );

    Ok(())
}

#[test]
fn test_bash_working_directory_tracking() -> Result<()> {
    // Test that working directories are properly tracked
    let helper = PxhTestHelper::new();
    let home_dir = helper.home_dir();
    let bashrc_path = home_dir.join(".bashrc");

    fs::write(&bashrc_path, "")?;

    // Create test directories
    let test_dir1 = home_dir.join("test1");
    let test_dir2 = home_dir.join("test2");
    fs::create_dir(&test_dir1)?;
    fs::create_dir(&test_dir2)?;

    // Install pxh
    let install_output = helper.command_with_args(&["install", "bash"]).output()?;

    assert!(install_output.status.success());

    // Spawn bash session with proper environment
    let cmd = helper.shell_command("bash");
    let mut session = spawn_command(cmd, Some(30_000))?;

    // Wait for shell initialization
    thread::sleep(Duration::from_millis(1000));

    // Run commands in different directories
    // First cd to test1, then run a command
    session.send_line(&format!("cd {}", test_dir1.display()))?;
    thread::sleep(Duration::from_millis(100));

    session.send_line("echo 'in test1'")?;
    session.exp_string("in test1")?;

    // Now cd to test2 and run another command
    session.send_line(&format!("cd {}", test_dir2.display()))?;
    thread::sleep(Duration::from_millis(100));

    session.send_line("echo 'in test2'")?;
    session.exp_string("in test2")?;

    session.send_line("exit")?;
    session.exp_eof()?;

    // Verify working directories were recorded using pxh export
    let output = helper.command_with_args(&["export"]).output()?;
    assert!(output.status.success(), "Export should succeed");

    let invocations: Vec<pxh::Invocation> = serde_json::from_slice(&output.stdout)?;

    assert!(
        invocations.iter().any(|inv| inv.command.to_string().contains("in test1")
            && inv
                .working_directory
                .as_ref()
                .map(|d| d.to_string().ends_with("test1"))
                .unwrap_or(false)),
        "Should record test1 directory"
    );
    assert!(
        invocations.iter().any(|inv| inv.command.to_string().contains("in test2")
            && inv
                .working_directory
                .as_ref()
                .map(|d| d.to_string().ends_with("test2"))
                .unwrap_or(false)),
        "Should record test2 directory"
    );

    Ok(())
}

#[test]
fn test_multiple_sessions() -> Result<()> {
    // Test that multiple concurrent sessions each get unique session IDs
    let helper = PxhTestHelper::new();
    let home_dir = helper.home_dir();
    let bashrc_path = home_dir.join(".bashrc");

    fs::write(&bashrc_path, "")?;

    // Install pxh
    let install_output = helper.command_with_args(&["install", "bash"]).output()?;

    assert!(install_output.status.success());

    // Spawn two bash sessions with proper environment
    let cmd1 = helper.shell_command("bash");
    let cmd2 = helper.shell_command("bash");

    let mut session1 = spawn_command(cmd1, Some(30_000))?;
    let mut session2 = spawn_command(cmd2, Some(30_000))?;

    // Wait for shell initialization
    thread::sleep(Duration::from_millis(1000));

    // Run commands in both sessions
    for (i, session) in [&mut session1, &mut session2].iter_mut().enumerate() {
        // Run a unique command in each session
        session.send_line(&format!("echo 'Hello from session {}'", i + 1))?;
        session.exp_string(&format!("Hello from session {}", i + 1))?;
    }

    // Exit both sessions
    session1.send_line("exit")?;
    session1.exp_eof()?;

    session2.send_line("exit")?;
    session2.exp_eof()?;

    // Verify that we have commands from two different sessions using pxh export
    let output = helper.command_with_args(&["export"]).output()?;
    assert!(output.status.success(), "Export should succeed");

    let invocations: Vec<pxh::Invocation> = serde_json::from_slice(&output.stdout)?;

    // Count unique session IDs
    let unique_sessions: std::collections::HashSet<_> =
        invocations.iter().map(|inv| inv.session_id).collect();

    assert_eq!(unique_sessions.len(), 2, "Should have exactly 2 different session IDs");

    // Verify each session has its command
    assert!(
        invocations.iter().any(|inv| inv.command.to_string().contains("Hello from session 1")),
        "Should have command from session 1"
    );
    assert!(
        invocations.iter().any(|inv| inv.command.to_string().contains("Hello from session 2")),
        "Should have command from session 2"
    );

    Ok(())
}

// ============================================================================
// SHELL PARITY TESTS - Ensure bash and zsh have equivalent coverage
// ============================================================================

/// Helper to set up a shell session and return the session handle
fn setup_shell_session(
    shell: Shell,
    helper: &PxhTestHelper,
) -> Result<rexpect::session::PtySession> {
    let home_dir = helper.home_dir();
    let rc_path = home_dir.join(shell.rc_file());

    // Create empty rc file
    fs::write(&rc_path, "")?;

    // Install pxh for the shell
    let install_output = helper.command_with_args(&["install", shell.name()]).output()?;
    assert!(
        install_output.status.success(),
        "Install failed for {}: {}",
        shell.name(),
        String::from_utf8_lossy(&install_output.stderr)
    );

    // Spawn shell session
    let cmd = helper.shell_command(shell.name());
    let session = spawn_command(cmd, Some(30_000))?;

    Ok(session)
}

// ----------------------------------------------------------------------------
// ZSH PARITY: Exit status tracking (matches test_bash_command_with_exit_status)
// ----------------------------------------------------------------------------

#[test]
fn test_zsh_command_with_exit_status() -> Result<()> {
    if !Shell::Zsh.is_available() {
        eprintln!("Skipping zsh test: zsh not found in PATH");
        return Ok(());
    }

    let helper = PxhTestHelper::new();
    let mut session = setup_shell_session(Shell::Zsh, &helper)?;

    thread::sleep(Duration::from_millis(1000));

    // Run a successful command
    session.send_line("true")?;
    thread::sleep(Duration::from_millis(100));

    // Run a failing command
    session.send_line("false")?;
    thread::sleep(Duration::from_millis(100));

    // Exit
    session.send_line("exit")?;
    session.exp_eof()?;

    // Check the database for exit statuses
    let output = helper.command_with_args(&["export"]).output()?;
    assert!(output.status.success(), "Export should succeed");

    let invocations: Vec<pxh::Invocation> = serde_json::from_slice(&output.stdout)?;

    assert!(
        invocations.iter().any(|inv| inv.command == "true" && inv.exit_status == Some(0)),
        "true command should have exit status 0"
    );
    assert!(
        invocations.iter().any(|inv| inv.command == "false" && inv.exit_status == Some(1)),
        "false command should have exit status 1"
    );

    Ok(())
}

// ----------------------------------------------------------------------------
// ZSH PARITY: Working directory tracking (matches test_bash_working_directory_tracking)
// ----------------------------------------------------------------------------

#[test]
fn test_zsh_working_directory_tracking() -> Result<()> {
    if !Shell::Zsh.is_available() {
        eprintln!("Skipping zsh test: zsh not found in PATH");
        return Ok(());
    }

    let helper = PxhTestHelper::new();
    let home_dir = helper.home_dir();

    // Create test directories
    let test_dir1 = home_dir.join("zsh_test1");
    let test_dir2 = home_dir.join("zsh_test2");
    fs::create_dir(&test_dir1)?;
    fs::create_dir(&test_dir2)?;

    let mut session = setup_shell_session(Shell::Zsh, &helper)?;

    thread::sleep(Duration::from_millis(1000));

    // Run commands in different directories
    session.send_line(&format!("cd {}", test_dir1.display()))?;
    thread::sleep(Duration::from_millis(100));

    session.send_line("echo 'in zsh_test1'")?;
    session.exp_string("in zsh_test1")?;

    session.send_line(&format!("cd {}", test_dir2.display()))?;
    thread::sleep(Duration::from_millis(100));

    session.send_line("echo 'in zsh_test2'")?;
    session.exp_string("in zsh_test2")?;

    session.send_line("exit")?;
    session.exp_eof()?;

    // Verify working directories were recorded
    let output = helper.command_with_args(&["export"]).output()?;
    assert!(output.status.success(), "Export should succeed");

    let invocations: Vec<pxh::Invocation> = serde_json::from_slice(&output.stdout)?;

    assert!(
        invocations.iter().any(|inv| inv.command.to_string().contains("in zsh_test1")
            && inv
                .working_directory
                .as_ref()
                .map(|d| d.to_string().ends_with("zsh_test1"))
                .unwrap_or(false)),
        "Should record zsh_test1 directory"
    );
    assert!(
        invocations.iter().any(|inv| inv.command.to_string().contains("in zsh_test2")
            && inv
                .working_directory
                .as_ref()
                .map(|d| d.to_string().ends_with("zsh_test2"))
                .unwrap_or(false)),
        "Should record zsh_test2 directory"
    );

    Ok(())
}

// ============================================================================
// PIPED COMMANDS - Test command pipelines
// ============================================================================

#[test]
fn test_bash_piped_commands() -> Result<()> {
    let helper = PxhTestHelper::new();
    let mut session = setup_shell_session(Shell::Bash, &helper)?;

    thread::sleep(Duration::from_millis(1000));

    // Simple pipe
    session.send_line("echo 'hello world' | grep hello")?;
    session.exp_string("hello world")?;

    // Multi-stage pipeline
    session.send_line("echo -e 'b\\na\\nc' | sort | head -1")?;
    session.exp_string("a")?;

    // Pipeline with redirection
    session.send_line("echo 'test output' | cat > /dev/null")?;
    thread::sleep(Duration::from_millis(100));

    session.send_line("exit")?;
    session.exp_eof()?;

    let commands = get_commands(&helper)?;

    assert!(
        commands.iter().any(|c| c.contains("echo 'hello world' | grep hello")),
        "Should record simple pipe command"
    );
    assert!(
        commands.iter().any(|c| c.contains("sort") && c.contains("head")),
        "Should record multi-stage pipeline"
    );

    Ok(())
}

#[test]
fn test_zsh_piped_commands() -> Result<()> {
    if !Shell::Zsh.is_available() {
        eprintln!("Skipping zsh test: zsh not found in PATH");
        return Ok(());
    }

    let helper = PxhTestHelper::new();
    let mut session = setup_shell_session(Shell::Zsh, &helper)?;

    thread::sleep(Duration::from_millis(1000));

    // Simple pipe
    session.send_line("echo 'hello world' | grep hello")?;
    session.exp_string("hello world")?;

    // Multi-stage pipeline
    session.send_line("echo -e 'b\\na\\nc' | sort | head -1")?;
    session.exp_string("a")?;

    // Pipeline with redirection
    session.send_line("echo 'test output' | cat > /dev/null")?;
    thread::sleep(Duration::from_millis(100));

    session.send_line("exit")?;
    session.exp_eof()?;

    let commands = get_commands(&helper)?;

    assert!(
        commands.iter().any(|c| c.contains("echo 'hello world' | grep hello")),
        "Should record simple pipe command"
    );
    assert!(
        commands.iter().any(|c| c.contains("sort") && c.contains("head")),
        "Should record multi-stage pipeline"
    );

    Ok(())
}

// ============================================================================
// COMPOUND COMMANDS - Test &&, ||, and ; operators
// ============================================================================

#[test]
fn test_bash_compound_commands() -> Result<()> {
    let helper = PxhTestHelper::new();
    let mut session = setup_shell_session(Shell::Bash, &helper)?;

    thread::sleep(Duration::from_millis(1000));

    // AND operator
    session.send_line("true && echo 'and succeeded'")?;
    session.exp_string("and succeeded")?;

    // OR operator
    session.send_line("false || echo 'or fallback'")?;
    session.exp_string("or fallback")?;

    // Semicolon chaining
    session.send_line("echo 'first'; echo 'second'")?;
    session.exp_string("first")?;
    session.exp_string("second")?;

    // Mixed operators
    session.send_line("true && echo 'yes' || echo 'no'")?;
    session.exp_string("yes")?;

    session.send_line("exit")?;
    session.exp_eof()?;

    let commands = get_commands(&helper)?;

    assert!(
        commands.iter().any(|c| c.contains("&&") && c.contains("and succeeded")),
        "Should record AND compound command"
    );
    assert!(
        commands.iter().any(|c| c.contains("||") && c.contains("or fallback")),
        "Should record OR compound command"
    );
    assert!(
        commands.iter().any(|c| c.contains(";") && c.contains("first") && c.contains("second")),
        "Should record semicolon-chained command"
    );

    Ok(())
}

#[test]
fn test_zsh_compound_commands() -> Result<()> {
    if !Shell::Zsh.is_available() {
        eprintln!("Skipping zsh test: zsh not found in PATH");
        return Ok(());
    }

    let helper = PxhTestHelper::new();
    let mut session = setup_shell_session(Shell::Zsh, &helper)?;

    thread::sleep(Duration::from_millis(1000));

    // AND operator
    session.send_line("true && echo 'and succeeded'")?;
    session.exp_string("and succeeded")?;

    // OR operator
    session.send_line("false || echo 'or fallback'")?;
    session.exp_string("or fallback")?;

    // Semicolon chaining
    session.send_line("echo 'first'; echo 'second'")?;
    session.exp_string("first")?;
    session.exp_string("second")?;

    // Mixed operators
    session.send_line("true && echo 'yes' || echo 'no'")?;
    session.exp_string("yes")?;

    session.send_line("exit")?;
    session.exp_eof()?;

    let commands = get_commands(&helper)?;

    assert!(
        commands.iter().any(|c| c.contains("&&") && c.contains("and succeeded")),
        "Should record AND compound command"
    );
    assert!(
        commands.iter().any(|c| c.contains("||") && c.contains("or fallback")),
        "Should record OR compound command"
    );
    assert!(
        commands.iter().any(|c| c.contains(";") && c.contains("first") && c.contains("second")),
        "Should record semicolon-chained command"
    );

    Ok(())
}

// ============================================================================
// MULTILINE COMMANDS - Test backslash continuation
// ============================================================================

#[test]
fn test_bash_multiline_commands() -> Result<()> {
    let helper = PxhTestHelper::new();
    let mut session = setup_shell_session(Shell::Bash, &helper)?;

    thread::sleep(Duration::from_millis(1000));

    // Backslash continuation - send as separate lines
    session.send_line("echo 'line1' \\")?;
    thread::sleep(Duration::from_millis(50));
    session.send_line("'line2' \\")?;
    thread::sleep(Duration::from_millis(50));
    session.send_line("'line3'")?;
    session.exp_string("line1 line2 line3")?;

    // Simple heredoc (inline for easier testing)
    session.send_line("cat << 'ENDMARKER'\nheredoc content\nENDMARKER")?;
    session.exp_string("heredoc content")?;

    session.send_line("exit")?;
    session.exp_eof()?;

    let commands = get_commands(&helper)?;

    // The multiline command should be recorded - exact format may vary by shell
    // At minimum, the content should be captured
    assert!(
        commands.iter().any(|c| c.contains("line1") && c.contains("line2") && c.contains("line3")),
        "Should record multiline echo command. Commands: {:?}",
        commands
    );

    Ok(())
}

#[test]
fn test_zsh_multiline_commands() -> Result<()> {
    if !Shell::Zsh.is_available() {
        eprintln!("Skipping zsh test: zsh not found in PATH");
        return Ok(());
    }

    let helper = PxhTestHelper::new();
    let mut session = setup_shell_session(Shell::Zsh, &helper)?;

    thread::sleep(Duration::from_millis(1000));

    // Backslash continuation - send as separate lines
    session.send_line("echo 'line1' \\")?;
    thread::sleep(Duration::from_millis(50));
    session.send_line("'line2' \\")?;
    thread::sleep(Duration::from_millis(50));
    session.send_line("'line3'")?;
    session.exp_string("line1 line2 line3")?;

    // Simple heredoc (inline for easier testing)
    session.send_line("cat << 'ENDMARKER'\nheredoc content\nENDMARKER")?;
    session.exp_string("heredoc content")?;

    session.send_line("exit")?;
    session.exp_eof()?;

    let commands = get_commands(&helper)?;

    // The multiline command should be recorded - exact format may vary by shell
    assert!(
        commands.iter().any(|c| c.contains("line1") && c.contains("line2") && c.contains("line3")),
        "Should record multiline echo command. Commands: {:?}",
        commands
    );

    Ok(())
}

// ============================================================================
// BACKGROUND PROCESSES - Test & operator
// ============================================================================

#[test]
fn test_bash_background_commands() -> Result<()> {
    let helper = PxhTestHelper::new();
    let mut session = setup_shell_session(Shell::Bash, &helper)?;

    thread::sleep(Duration::from_millis(1000));

    // Background command
    session.send_line("sleep 0.1 &")?;
    thread::sleep(Duration::from_millis(200));

    // Another command while background runs
    session.send_line("echo 'foreground'")?;
    session.exp_string("foreground")?;

    // Wait for background to complete
    session.send_line("wait")?;
    thread::sleep(Duration::from_millis(100));

    session.send_line("exit")?;
    session.exp_eof()?;

    let commands = get_commands(&helper)?;

    // Background command should be recorded
    assert!(
        commands.iter().any(|c| c.contains("sleep") && c.contains("&")),
        "Should record background command. Commands: {:?}",
        commands
    );
    assert!(
        commands.iter().any(|c| c.contains("foreground")),
        "Should record foreground command while background runs"
    );

    Ok(())
}

#[test]
fn test_zsh_background_commands() -> Result<()> {
    if !Shell::Zsh.is_available() {
        eprintln!("Skipping zsh test: zsh not found in PATH");
        return Ok(());
    }

    let helper = PxhTestHelper::new();
    let mut session = setup_shell_session(Shell::Zsh, &helper)?;

    thread::sleep(Duration::from_millis(1000));

    // Background command
    session.send_line("sleep 0.1 &")?;
    thread::sleep(Duration::from_millis(200));

    // Another command while background runs
    session.send_line("echo 'foreground'")?;
    session.exp_string("foreground")?;

    // Wait for background to complete
    session.send_line("wait")?;
    thread::sleep(Duration::from_millis(100));

    session.send_line("exit")?;
    session.exp_eof()?;

    let commands = get_commands(&helper)?;

    // Background command should be recorded
    assert!(
        commands.iter().any(|c| c.contains("sleep") && c.contains("&")),
        "Should record background command. Commands: {:?}",
        commands
    );
    assert!(
        commands.iter().any(|c| c.contains("foreground")),
        "Should record foreground command while background runs"
    );

    Ok(())
}

// ============================================================================
// SUBSHELLS - Test ( ) syntax
// Note: bash-preexec has known limitations with subshells
// ============================================================================

#[test]
fn test_bash_subshell_commands() -> Result<()> {
    // NOTE: bash-preexec has documented limitations with subshells.
    // Commands run inside ( ) are NOT captured - this is a known limitation.
    // However, command substitution $(...) within an outer command IS captured.
    // This test documents the current behavior.

    let helper = PxhTestHelper::new();
    let mut session = setup_shell_session(Shell::Bash, &helper)?;

    thread::sleep(Duration::from_millis(1000));

    // Parenthesized subshell - NOT captured by bash-preexec (known limitation)
    session.send_line("(cd /tmp && pwd)")?;
    session.exp_string("/tmp")?;

    // Command substitution - the outer command IS captured
    session.send_line("echo \"today is $(date +%Y)\"")?;
    session.exp_regex(r"today is \d{4}")?;

    session.send_line("exit")?;
    session.exp_eof()?;

    let commands = get_commands(&helper)?;

    // Parenthesized subshells (cd /tmp && pwd) are NOT captured - known bash-preexec limitation
    // But command substitution in echo is captured
    assert!(
        commands.iter().any(|c| c.contains("today is $(date")),
        "Should record command substitution. Commands: {:?}",
        commands
    );

    // Document the limitation: parenthesized subshells are not captured
    assert!(
        !commands.iter().any(|c| c.contains("(cd /tmp")),
        "KNOWN LIMITATION: Parenthesized subshells are not captured by bash-preexec"
    );

    Ok(())
}

#[test]
fn test_zsh_subshell_commands() -> Result<()> {
    if !Shell::Zsh.is_available() {
        eprintln!("Skipping zsh test: zsh not found in PATH");
        return Ok(());
    }

    let helper = PxhTestHelper::new();
    let mut session = setup_shell_session(Shell::Zsh, &helper)?;

    thread::sleep(Duration::from_millis(1000));

    // Subshell command
    session.send_line("(cd /tmp && pwd)")?;
    session.exp_string("/tmp")?;

    // Command substitution
    session.send_line("echo \"today is $(date +%Y)\"")?;
    session.exp_regex(r"today is \d{4}")?;

    session.send_line("exit")?;
    session.exp_eof()?;

    let commands = get_commands(&helper)?;

    // Zsh should capture subshell commands
    assert!(
        commands.iter().any(|c| c.contains("cd /tmp") || c.contains("(cd")),
        "Should record subshell command. Commands: {:?}",
        commands
    );

    Ok(())
}

// ============================================================================
// SPECIAL CHARACTERS - Test quoting and escaping
// ============================================================================

#[test]
fn test_bash_special_characters() -> Result<()> {
    let helper = PxhTestHelper::new();
    let mut session = setup_shell_session(Shell::Bash, &helper)?;

    thread::sleep(Duration::from_millis(1000));

    // Single quotes
    session.send_line("echo 'single quoted $VAR'")?;
    session.exp_string("single quoted $VAR")?;

    // Double quotes with variable
    session.send_line("VAR=test; echo \"double quoted $VAR\"")?;
    session.exp_string("double quoted test")?;

    // Escaped characters
    session.send_line("echo \"quotes: \\\"nested\\\"\"")?;
    session.exp_string("quotes: \"nested\"")?;

    // Special shell characters
    session.send_line("echo 'asterisk * and question ?'")?;
    session.exp_string("asterisk * and question ?")?;

    session.send_line("exit")?;
    session.exp_eof()?;

    let commands = get_commands(&helper)?;

    assert!(
        commands.iter().any(|c| c.contains("single quoted")),
        "Should record single-quoted command"
    );
    assert!(
        commands.iter().any(|c| c.contains("double quoted")),
        "Should record double-quoted command"
    );

    Ok(())
}

#[test]
fn test_zsh_special_characters() -> Result<()> {
    if !Shell::Zsh.is_available() {
        eprintln!("Skipping zsh test: zsh not found in PATH");
        return Ok(());
    }

    let helper = PxhTestHelper::new();
    let mut session = setup_shell_session(Shell::Zsh, &helper)?;

    thread::sleep(Duration::from_millis(1000));

    // Single quotes
    session.send_line("echo 'single quoted $VAR'")?;
    session.exp_string("single quoted $VAR")?;

    // Double quotes with variable
    session.send_line("VAR=test; echo \"double quoted $VAR\"")?;
    session.exp_string("double quoted test")?;

    // Escaped characters
    session.send_line("echo \"quotes: \\\"nested\\\"\"")?;
    session.exp_string("quotes: \"nested\"")?;

    // Special shell characters
    session.send_line("echo 'asterisk * and question ?'")?;
    session.exp_string("asterisk * and question ?")?;

    session.send_line("exit")?;
    session.exp_eof()?;

    let commands = get_commands(&helper)?;

    assert!(
        commands.iter().any(|c| c.contains("single quoted")),
        "Should record single-quoted command"
    );
    assert!(
        commands.iter().any(|c| c.contains("double quoted")),
        "Should record double-quoted command"
    );

    Ok(())
}

// ============================================================================
// CONTROL STRUCTURES - Test loops and conditionals
// ============================================================================

#[test]
fn test_bash_control_structures() -> Result<()> {
    let helper = PxhTestHelper::new();
    let mut session = setup_shell_session(Shell::Bash, &helper)?;

    thread::sleep(Duration::from_millis(1000));

    // For loop (single line)
    session.send_line("for i in 1 2 3; do echo $i; done")?;
    session.exp_string("1")?;
    session.exp_string("2")?;
    session.exp_string("3")?;

    // If statement (single line)
    session.send_line("if true; then echo 'condition met'; fi")?;
    session.exp_string("condition met")?;

    // While loop (single line)
    session.send_line("x=0; while [ $x -lt 2 ]; do echo $x; x=$((x+1)); done")?;
    session.exp_string("0")?;
    session.exp_string("1")?;

    session.send_line("exit")?;
    session.exp_eof()?;

    let commands = get_commands(&helper)?;

    assert!(
        commands.iter().any(|c| c.contains("for") && c.contains("do") && c.contains("done")),
        "Should record for loop. Commands: {:?}",
        commands
    );
    assert!(
        commands.iter().any(|c| c.contains("if") && c.contains("then") && c.contains("fi")),
        "Should record if statement. Commands: {:?}",
        commands
    );

    Ok(())
}

#[test]
fn test_zsh_control_structures() -> Result<()> {
    if !Shell::Zsh.is_available() {
        eprintln!("Skipping zsh test: zsh not found in PATH");
        return Ok(());
    }

    let helper = PxhTestHelper::new();
    let mut session = setup_shell_session(Shell::Zsh, &helper)?;

    thread::sleep(Duration::from_millis(1000));

    // For loop (single line)
    session.send_line("for i in 1 2 3; do echo $i; done")?;
    session.exp_string("1")?;
    session.exp_string("2")?;
    session.exp_string("3")?;

    // If statement (single line)
    session.send_line("if true; then echo 'condition met'; fi")?;
    session.exp_string("condition met")?;

    // While loop (single line)
    session.send_line("x=0; while [ $x -lt 2 ]; do echo $x; x=$((x+1)); done")?;
    session.exp_string("0")?;
    session.exp_string("1")?;

    session.send_line("exit")?;
    session.exp_eof()?;

    let commands = get_commands(&helper)?;

    assert!(
        commands.iter().any(|c| c.contains("for") && c.contains("do") && c.contains("done")),
        "Should record for loop. Commands: {:?}",
        commands
    );
    assert!(
        commands.iter().any(|c| c.contains("if") && c.contains("then") && c.contains("fi")),
        "Should record if statement. Commands: {:?}",
        commands
    );

    Ok(())
}

// ============================================================================
// TIMING AND DURATION - Verify command timing is recorded
// ============================================================================

#[test]
fn test_bash_command_timing() -> Result<()> {
    let helper = PxhTestHelper::new();
    let mut session = setup_shell_session(Shell::Bash, &helper)?;

    thread::sleep(Duration::from_millis(1000));

    // Run a command that takes measurable time
    session.send_line("sleep 0.5")?;
    thread::sleep(Duration::from_millis(600));

    // Run a quick command
    session.send_line("true")?;
    thread::sleep(Duration::from_millis(100));

    session.send_line("exit")?;
    session.exp_eof()?;

    let output = helper.command_with_args(&["export"]).output()?;
    let invocations: Vec<pxh::Invocation> = serde_json::from_slice(&output.stdout)?;

    // Find the sleep command and verify it has timing
    let sleep_cmd = invocations.iter().find(|inv| inv.command.to_string().contains("sleep"));

    assert!(sleep_cmd.is_some(), "Should have recorded sleep command");

    // Verify start timestamp exists
    assert!(
        sleep_cmd.unwrap().start_unix_timestamp.is_some(),
        "Sleep command should have start timestamp"
    );

    Ok(())
}

#[test]
fn test_zsh_command_timing() -> Result<()> {
    if !Shell::Zsh.is_available() {
        eprintln!("Skipping zsh test: zsh not found in PATH");
        return Ok(());
    }

    let helper = PxhTestHelper::new();
    let mut session = setup_shell_session(Shell::Zsh, &helper)?;

    thread::sleep(Duration::from_millis(1000));

    // Run a command that takes measurable time
    session.send_line("sleep 0.5")?;
    thread::sleep(Duration::from_millis(600));

    // Run a quick command
    session.send_line("true")?;
    thread::sleep(Duration::from_millis(100));

    session.send_line("exit")?;
    session.exp_eof()?;

    let output = helper.command_with_args(&["export"]).output()?;
    let invocations: Vec<pxh::Invocation> = serde_json::from_slice(&output.stdout)?;

    // Find the sleep command and verify it has timing
    let sleep_cmd = invocations.iter().find(|inv| inv.command.to_string().contains("sleep"));

    assert!(sleep_cmd.is_some(), "Should have recorded sleep command");

    // Verify start timestamp exists
    assert!(
        sleep_cmd.unwrap().start_unix_timestamp.is_some(),
        "Sleep command should have start timestamp"
    );

    Ok(())
}
