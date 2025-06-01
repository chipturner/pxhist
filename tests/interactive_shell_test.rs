use std::{fs, thread, time::Duration};

use pxh::test_utils::PxhTestHelper;
use rexpect::session::spawn_command;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

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

    assert!(
        install_output.status.success(),
        "Install failed: {}",
        String::from_utf8_lossy(&install_output.stderr)
    );

    // Verify bashrc was modified
    let bashrc_content = fs::read_to_string(&bashrc_path)?;
    assert!(bashrc_content.contains("pxh shell-config bash"));

    // Now spawn an interactive bash session with proper environment
    let cmd = helper.shell_command("bash");
    let mut session = spawn_command(cmd, Some(30_000))?;

    // Wait for shell initialization and rc file loading
    thread::sleep(Duration::from_millis(1000));

    // Check if pxh is available
    session.send_line("which pxh")?;
    session.exp_regex(r"(/[^\r\n]+/pxh)")?;

    // Check environment variables
    session.send_line("echo PXH_DB_PATH=$PXH_DB_PATH")?;
    session.exp_string(&format!("PXH_DB_PATH={}", pxh_db_path.display()))?;

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
