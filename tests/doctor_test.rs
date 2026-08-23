use std::{env, fs, path::Path, process::Command};

use pxh::test_utils::pxh_path;
use tempfile::TempDir;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn pxh_command() -> Command {
    let mut cmd = Command::new(pxh_path());
    if let Ok(profile_file) = env::var("LLVM_PROFILE_FILE") {
        cmd.env("LLVM_PROFILE_FILE", profile_file);
    }
    if let Ok(llvm_cov) = env::var("CARGO_LLVM_COV") {
        cmd.env("CARGO_LLVM_COV", llvm_cov);
    }
    cmd
}

fn create_test_db(db_path: &std::path::Path) -> Result<()> {
    let output = pxh_command()
        .args([
            "--db",
            db_path.to_str().unwrap(),
            "insert",
            "--hostname",
            "test",
            "--shellname",
            "bash",
            "--username",
            "test",
            "--session-id",
            "1",
            "--start-unix-timestamp",
            "1000000",
            "echo hello",
        ])
        .output()?;
    assert!(output.status.success(), "insert failed: {}", String::from_utf8_lossy(&output.stderr));
    Ok(())
}

#[test]
fn doctor_runs_on_fresh_db() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("pxh.db");
    create_test_db(&db_path)?;

    let output =
        pxh_command().args(["--db", db_path.to_str().unwrap(), "doctor", "--verbose"]).output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("pxh "), "should show version");
    assert!(stdout.contains("Schema version"), "should check schema");
    assert!(stdout.contains("SQLite"), "should show SQLite version");
    Ok(())
}

#[test]
fn doctor_report_produces_markdown() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("pxh.db");
    create_test_db(&db_path)?;

    let output =
        pxh_command().args(["--db", db_path.to_str().unwrap(), "doctor", "--report"]).output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("<details>"), "should have details tag");
    assert!(stdout.contains("pxh version:"), "should have version line");
    assert!(stdout.contains("</details>"), "should close details tag");
    assert!(stdout.contains("SQLite:"), "should show sqlite version");
    assert!(stdout.contains("Schema version:"), "should show schema version");
    Ok(())
}

#[test]
fn doctor_fix_repairs_permissions() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("pxh.db");
    create_test_db(&db_path)?;

    // Set wrong permissions
    fs::set_permissions(&db_path, fs::Permissions::from_mode(0o644))?;

    let output = pxh_command()
        .args(["--db", db_path.to_str().unwrap(), "doctor", "--fix", "--yes"])
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Fixing"), "should show fix action");

    let mode = fs::metadata(&db_path)?.permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "permissions should be fixed");

    Ok(())
}

#[test]
fn doctor_default_hides_passing_checks() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("pxh.db");
    create_test_db(&db_path)?;

    let output = pxh_command().args(["--db", db_path.to_str().unwrap(), "doctor"]).output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Default mode should NOT show "ok" lines
    assert!(!stdout.contains("  ok  Schema version"), "should hide passing checks in default mode");
    Ok(())
}

#[test]
fn doctor_verbose_shows_passing_checks() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("pxh.db");
    create_test_db(&db_path)?;

    let output =
        pxh_command().args(["--db", db_path.to_str().unwrap(), "doctor", "--verbose"]).output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("  ok  Schema version"), "should show passing checks with --verbose");
    Ok(())
}

fn insert_command_into_db(db_path: &Path, command: &str) -> Result<()> {
    let output = pxh_command()
        .args([
            "--db",
            db_path.to_str().unwrap(),
            "insert",
            "--hostname",
            "test",
            "--shellname",
            "bash",
            "--username",
            "test",
            "--session-id",
            "1",
            "--start-unix-timestamp",
            "1000000",
            command,
        ])
        .output()?;
    assert!(output.status.success(), "insert failed: {}", String::from_utf8_lossy(&output.stderr));
    Ok(())
}

#[test]
fn doctor_fix_merges_legacy_db() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let home_dir = temp_dir.path();

    // Create legacy database at ~/.pxh/pxh.db
    let legacy_dir = home_dir.join(".pxh");
    fs::create_dir_all(&legacy_dir)?;
    let legacy_db = legacy_dir.join("pxh.db");
    insert_command_into_db(&legacy_db, "legacy_command_123")?;

    // Create XDG database at ~/.local/share/pxh/pxh.db
    let xdg_dir = home_dir.join(".local").join("share").join("pxh");
    fs::create_dir_all(&xdg_dir)?;
    let xdg_db = xdg_dir.join("pxh.db");
    insert_command_into_db(&xdg_db, "xdg_command_456")?;

    // Run doctor --fix --yes with HOME pointing to our temp dir
    let output = pxh_command()
        .env("HOME", home_dir)
        .args(["--db", xdg_db.to_str().unwrap(), "doctor", "--fix", "--yes"])
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "doctor --fix should succeed, stdout: {stdout}, stderr: {stderr}"
    );
    assert!(stdout.contains("Merged"), "should report merged commands: {stdout}");

    // Verify legacy command was merged into XDG db
    let export_output =
        pxh_command().args(["--db", xdg_db.to_str().unwrap(), "export"]).output()?;
    let export_stdout = String::from_utf8_lossy(&export_output.stdout);
    assert!(
        export_stdout.contains("legacy_command_123"),
        "XDG db should contain legacy command after merge"
    );
    assert!(
        export_stdout.contains("xdg_command_456"),
        "XDG db should still contain its own command"
    );

    // Legacy dir should have been renamed to backup
    assert!(!legacy_dir.exists(), "legacy dir should be moved");
    assert!(home_dir.join(".pxh.backup").exists(), "backup should exist");

    Ok(())
}

// -- Broken-state diagnostics ------------------------------------------------

/// Run doctor with an isolated HOME/SHELL and return (stdout, success).
fn doctor(home: &Path, db: &Path, shell: &str, extra: &[&str]) -> Result<(String, bool)> {
    let mut args = vec!["--db", db.to_str().unwrap(), "doctor", "--verbose"];
    args.extend_from_slice(extra);
    let output = pxh_command()
        .env_clear()
        .env("HOME", home)
        .env("SHELL", shell)
        .env("PATH", env::var("PATH").unwrap_or_default())
        .args(args)
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok((format!("{stdout}{stderr}"), output.status.success()))
}

#[test]
fn doctor_warns_when_no_commands_recorded() -> Result<()> {
    let temp = TempDir::new()?;
    let db = temp.path().join("pxh.db");
    pxh_command().args(["--db", db.to_str().unwrap(), "stats"]).output()?; // creates empty db
    let (out, _) = doctor(temp.path(), &db, "/bin/bash", &[])?;
    assert!(out.contains("No commands recorded yet"), "{out}");
    Ok(())
}

#[test]
fn doctor_warns_when_last_command_is_stale() -> Result<()> {
    let temp = TempDir::new()?;
    let db = temp.path().join("pxh.db");
    create_test_db(&db)?; // timestamp 1000000 -- 1970
    let (out, _) = doctor(temp.path(), &db, "/bin/bash", &[])?;
    assert!(out.contains("shell hooks may not be active"), "{out}");
    assert!(out.contains("days ago"), "{out}");
    Ok(())
}

#[test]
fn doctor_reports_hooks_active_for_recent_command() -> Result<()> {
    let temp = TempDir::new()?;
    let db = temp.path().join("pxh.db");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
    let status = pxh_command()
        .args([
            "--db",
            db.to_str().unwrap(),
            "insert",
            "--hostname",
            "h",
            "--shellname",
            "bash",
            "--username",
            "u",
            "--session-id",
            "1",
            "--start-unix-timestamp",
            &now.to_string(),
            "echo recent",
        ])
        .status()?;
    assert!(status.success());
    let (out, _) = doctor(temp.path(), &db, "/bin/bash", &[])?;
    assert!(out.contains("Shell hooks active"), "{out}");
    Ok(())
}

#[test]
fn doctor_warns_when_rc_file_lacks_hooks_and_fix_installs_them() -> Result<()> {
    let temp = TempDir::new()?;
    let db = temp.path().join("pxh.db");
    create_test_db(&db)?;
    let zshrc = temp.path().join(".zshrc");
    fs::write(&zshrc, "# nothing here\n")?;

    let (out, _) = doctor(temp.path(), &db, "/usr/bin/zsh", &[])?;
    assert!(out.contains("~/.zshrc does not contain pxh shell-config"), "{out}");
    assert!(out.contains("Run: pxh install zsh"), "{out}");

    let (out, ok) = doctor(temp.path(), &db, "/usr/bin/zsh", &["--fix", "--yes"])?;
    assert!(ok, "{out}");
    assert!(out.contains("Install pxh shell hooks into ~/.zshrc"), "{out}");
    let contents = fs::read_to_string(&zshrc)?;
    assert!(contents.contains("pxh shell-config zsh"), "fix should install hooks: {contents}");
    assert!(contents.contains("# nothing here"), "fix must preserve existing rc content");
    Ok(())
}

#[test]
fn doctor_warns_when_rc_file_missing() -> Result<()> {
    let temp = TempDir::new()?;
    let db = temp.path().join("pxh.db");
    create_test_db(&db)?;
    let (out, _) = doctor(temp.path(), &db, "/bin/bash", &[])?;
    assert!(out.contains("~/.bashrc not found"), "{out}");
    Ok(())
}

#[test]
fn doctor_reports_invalid_config_toml_as_failure() -> Result<()> {
    let temp = TempDir::new()?;
    let db = temp.path().join("pxh.db");
    create_test_db(&db)?;
    let config_dir = temp.path().join(".config/pxh");
    fs::create_dir_all(&config_dir)?;
    fs::write(config_dir.join("config.toml"), "[recall\nkeymap = \n")?;
    let (out, _) = doctor(temp.path(), &db, "/bin/bash", &[])?;
    assert!(out.contains("invalid TOML"), "{out}");
    Ok(())
}

#[test]
fn doctor_fix_generates_machine_id_in_config() -> Result<()> {
    let temp = TempDir::new()?;
    let db = temp.path().join("pxh.db");
    create_test_db(&db)?;
    let (out, _) = doctor(temp.path(), &db, "/bin/bash", &[])?;
    assert!(out.contains("No machine_id in config"), "{out}");

    let (out, ok) = doctor(temp.path(), &db, "/bin/bash", &["--fix", "--yes"])?;
    assert!(ok, "{out}");
    let config = fs::read_to_string(temp.path().join(".config/pxh/config.toml"))?;
    assert!(config.contains("machine_id"), "fix should persist a machine_id: {config}");
    let (out, _) = doctor(temp.path(), &db, "/bin/bash", &[])?;
    assert!(out.contains("machine_id present"), "{out}");
    Ok(())
}

#[test]
fn doctor_warns_when_legacy_and_xdg_configs_both_exist() -> Result<()> {
    let temp = TempDir::new()?;
    let db = temp.path().join("pxh.db");
    create_test_db(&db)?;
    for dir in [".pxh", ".config/pxh"] {
        let d = temp.path().join(dir);
        fs::create_dir_all(&d)?;
        fs::write(d.join("config.toml"), "[host]\nhostname = \"x\"\n")?;
    }
    let (out, _) = doctor(temp.path(), &db, "/bin/bash", &[])?;
    assert!(out.contains("Both legacy ~/.pxh/config.toml and XDG config exist"), "{out}");
    Ok(())
}

#[test]
fn doctor_fails_cleanly_on_unreadable_database() -> Result<()> {
    let temp = TempDir::new()?;
    let db = temp.path().join("pxh.db");
    fs::write(&db, "this is not a sqlite database at all")?;
    let (out, ok) = doctor(temp.path(), &db, "/bin/bash", &[])?;
    assert!(!ok || out.contains("Could not open database"), "{out}");
    Ok(())
}
