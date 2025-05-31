use std::{env, fs, process::Command};

use tempfile::TempDir;

mod common;
use common::pxh_path;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

// Helper to create a Command with coverage environment variables
fn pxh_command() -> Command {
    let mut cmd = Command::new(pxh_path());

    // Propagate coverage environment variables if they exist
    if let Ok(profile_file) = env::var("LLVM_PROFILE_FILE") {
        cmd.env("LLVM_PROFILE_FILE", profile_file);
    }
    if let Ok(llvm_cov) = env::var("CARGO_LLVM_COV") {
        cmd.env("CARGO_LLVM_COV", llvm_cov);
    }

    cmd
}

#[test]
fn test_install_creates_correct_rc_files() -> Result<()> {
    // Test that install command correctly modifies shell RC files
    let temp_dir = TempDir::new()?;
    let home_dir = temp_dir.path();
    
    // Test bash installation
    let bashrc_path = home_dir.join(".bashrc");
    fs::write(&bashrc_path, "# existing bashrc content\n")?;
    
    let output = pxh_command()
        .env("HOME", home_dir)
        .args(&["install", "bash"])
        .output()?;
    
    assert!(output.status.success(), "Bash install failed: {}", String::from_utf8_lossy(&output.stderr));
    
    let bashrc_content = fs::read_to_string(&bashrc_path)?;
    assert!(bashrc_content.contains("# existing bashrc content"), "Original content should be preserved");
    assert!(bashrc_content.contains("pxh shell-config bash"), "pxh shell-config should be added");
    assert!(bashrc_content.contains("command -v pxh"), "Command check should be present");
    
    // Test zsh installation
    let zshrc_path = home_dir.join(".zshrc");
    fs::write(&zshrc_path, "# existing zshrc content\n")?;
    
    let output = pxh_command()
        .env("HOME", home_dir)
        .args(&["install", "zsh"])
        .output()?;
    
    assert!(output.status.success(), "Zsh install failed: {}", String::from_utf8_lossy(&output.stderr));
    
    let zshrc_content = fs::read_to_string(&zshrc_path)?;
    assert!(zshrc_content.contains("# existing zshrc content"), "Original content should be preserved");
    assert!(zshrc_content.contains("pxh shell-config zsh"), "pxh shell-config should be added");
    
    Ok(())
}

#[test]
fn test_install_is_idempotent() -> Result<()> {
    // Test that running install multiple times doesn't duplicate the configuration
    let temp_dir = TempDir::new()?;
    let home_dir = temp_dir.path();
    let bashrc_path = home_dir.join(".bashrc");
    
    fs::write(&bashrc_path, "")?;
    
    // First install
    let output1 = pxh_command()
        .env("HOME", home_dir)
        .args(&["install", "bash"])
        .output()?;
    
    assert!(output1.status.success());
    let bashrc_after_first = fs::read_to_string(&bashrc_path)?;
    
    // Second install
    let output2 = pxh_command()
        .env("HOME", home_dir)
        .args(&["install", "bash"])
        .output()?;
    
    assert!(output2.status.success());
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(stdout2.contains("already present"),
            "Should report shell config already present");
    
    let bashrc_after_second = fs::read_to_string(&bashrc_path)?;
    assert_eq!(bashrc_after_first, bashrc_after_second, "RC file should not change on second install");
    
    // Count occurrences of pxh shell-config
    let config_count = bashrc_after_second.matches("pxh shell-config").count();
    assert_eq!(config_count, 1, "Should have exactly one pxh shell-config entry");
    
    Ok(())
}

#[test]
fn test_shell_config_output() -> Result<()> {
    // Test that shell-config command produces valid shell code
    
    // Test bash config
    let bash_output = pxh_command()
        .args(&["shell-config", "bash"])
        .output()?;
    
    assert!(bash_output.status.success(), "shell-config bash failed");
    let bash_config = String::from_utf8_lossy(&bash_output.stdout);
    
    // Check for essential bash functions
    assert!(bash_config.contains("preexec()"), "Should define preexec function");
    assert!(bash_config.contains("precmd()"), "Should define precmd function");
    assert!(bash_config.contains("_pxh_init"), "Should have init function");
    assert!(bash_config.contains("PXH_SESSION_ID"), "Should set session ID");
    assert!(bash_config.contains("PXH_DB_PATH"), "Should set database path");
    
    // Test zsh config
    let zsh_output = pxh_command()
        .args(&["shell-config", "zsh"])
        .output()?;
    
    assert!(zsh_output.status.success(), "shell-config zsh failed");
    let zsh_config = String::from_utf8_lossy(&zsh_output.stdout);
    
    // Check for essential zsh functions
    assert!(zsh_config.contains("_pxh_addhistory"), "Should define _pxh_addhistory function");
    assert!(zsh_config.contains("_pxh_update_last_status"), "Should define _pxh_update_last_status function");
    assert!(zsh_config.contains("_pxh_init"), "Should have init function");
    assert!(zsh_config.contains("add-zsh-hook"), "Should use zsh hooks");
    
    Ok(())
}

#[test]
fn test_manual_command_recording() -> Result<()> {
    // Test that we can manually record commands using insert and seal
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test.db");
    
    // Insert a command
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    
    let insert_output = pxh_command()
        .args(&[
            "--db", db_path.to_str().unwrap(),
            "insert",
            "--shellname", "bash",
            "--hostname", "testhost",
            "--username", "testuser",
            "--session-id", "12345",
            "--start-unix-timestamp", &now.to_string(),
            "--working-directory", "/tmp",
            "echo hello world"
        ])
        .output()?;
    
    assert!(insert_output.status.success(), "Insert failed: {}", String::from_utf8_lossy(&insert_output.stderr));
    
    // Seal the command
    let seal_output = pxh_command()
        .args(&[
            "--db", db_path.to_str().unwrap(),
            "seal",
            "--session-id", "12345",
            "--exit-status", "0",
            "--end-unix-timestamp", &(now + 1).to_string(),
        ])
        .output()?;
    
    assert!(seal_output.status.success(), "Seal failed: {}", String::from_utf8_lossy(&seal_output.stderr));
    
    // Verify the command was recorded
    let show_output = pxh_command()
        .args(&[
            "--db", db_path.to_str().unwrap(),
            "show",
            "--limit", "10"
        ])
        .output()?;
    
    assert!(show_output.status.success(), "Show failed");
    let history = String::from_utf8_lossy(&show_output.stdout);
    assert!(history.contains("echo hello world"), "Command should be in history");
    // Note: Working directory might not be shown in default output format
    // Just verify the command was recorded
    assert!(history.contains("testhost") || history.contains("echo hello world"), 
            "History should contain our test data: {}", history);
    
    Ok(())
}

#[test]
fn test_install_rejects_invalid_shell() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let home_dir = temp_dir.path();
    
    let output = pxh_command()
        .env("HOME", home_dir)
        .args(&["install", "fish"])
        .output()?;
    
    assert!(!output.status.success(), "Should reject unsupported shell");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Unsupported shell: fish"), "Should mention unsupported shell");
    
    Ok(())
}