use assert_cmd::Command;
use tempfile::TempDir;

#[test]
fn test_ssh_sync_command_help() {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("pxh"));

    cmd.arg("sync")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("--remote"))
        .stdout(predicates::str::contains("--send-only"))
        .stdout(predicates::str::contains("--receive-only"))
        .stdout(predicates::str::contains("--remote-db"))
        .stdout(predicates::str::contains("--remote-pxh"))
        .stdout(predicates::str::contains("--ssh-cmd"))
        .stdout(predicates::str::contains("--server"));
}

#[test]
fn test_ssh_sync_send_only() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("pxh"));

    cmd.arg("--db")
        .arg(&db_path)
        .arg("sync")
        .arg("--remote")
        .arg("nonexistent-host")
        .arg("--send-only")
        .assert()
        .failure()
        .stderr(predicates::str::contains("Could not resolve hostname"));
}

#[test]
fn test_ssh_sync_receive_only() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("pxh"));

    cmd.arg("--db")
        .arg(&db_path)
        .arg("sync")
        .arg("--remote")
        .arg("nonexistent-host")
        .arg("--receive-only")
        .assert()
        .failure()
        .stderr(predicates::str::contains("Could not resolve hostname"));
}

#[test]
fn test_ssh_sync_bidirectional() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("pxh"));

    cmd.arg("--db")
        .arg(&db_path)
        .arg("sync")
        .arg("--remote")
        .arg("nonexistent-host")
        .assert()
        .failure()
        .stderr(predicates::str::contains("Could not resolve hostname"));
}

#[test]
fn test_directory_sync() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let sync_dir = temp_dir.path().join("sync");

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("pxh"));

    cmd.arg("--db").arg(&db_path).arg("sync").arg(&sync_dir).assert().success();
}

#[test]
fn test_sync_without_path_or_remote() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("pxh"));

    cmd.arg("--db")
        .arg(&db_path)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicates::str::contains("Directory path is required for directory-based sync"));
}

#[test]
fn test_send_only_without_remote() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("pxh"));

    cmd.arg("--db").arg(&db_path).arg("sync").arg("--send-only").assert().failure().stderr(
        predicates::str::contains(
            "--send-only and --receive-only flags require --remote or --stdin-stdout to be specified",
        ),
    );
}

#[test]
fn test_receive_only_without_remote() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("pxh"));

    cmd.arg("--db").arg(&db_path).arg("sync").arg("--receive-only").assert().failure().stderr(
        predicates::str::contains(
            "--send-only and --receive-only flags require --remote or --stdin-stdout to be specified",
        ),
    );
}

#[test]
fn test_remote_with_directory() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let sync_dir = temp_dir.path().join("sync");

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("pxh"));

    cmd.arg("--db")
        .arg(&db_path)
        .arg("sync")
        .arg("--remote")
        .arg("localhost")
        .arg(&sync_dir)
        .assert()
        .failure()
        .stderr(predicates::str::contains("Cannot specify both --remote and a directory path"));
}
