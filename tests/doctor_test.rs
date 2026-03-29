use std::{env, fs, process::Command};

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
