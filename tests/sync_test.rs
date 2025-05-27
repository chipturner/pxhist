use std::{
    env,
    path::Path,
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

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

// Helper to create test commands using pxh insert
// If days_ago is provided, creates command with that age, otherwise uses current time
fn insert_test_command(db_path: &Path, command: &str, days_ago: Option<u32>) -> Result<()> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let timestamp = match days_ago {
        Some(days) => now - (days as u64 * 86400),
        None => now,
    };

    let output = pxh_command()
        .args(&[
            "--db",
            db_path.to_str().unwrap(),
            "insert",
            "--shellname",
            "bash",
            "--hostname",
            "test-host",
            "--username",
            "test-user",
            "--session-id",
            "1",
            "--start-unix-timestamp",
            &timestamp.to_string(),
            command,
        ])
        .output()?;

    if !output.status.success() {
        return Err(format!(
            "Failed to insert command: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    let seal_output = pxh_command()
        .args(&[
            "--db",
            db_path.to_str().unwrap(),
            "seal",
            "--session-id",
            "1",
            "--exit-status",
            "0",
            "--end-unix-timestamp",
            &(timestamp + 100).to_string(),
        ])
        .output()?;

    if !seal_output.status.success() {
        return Err(format!(
            "Failed to seal command: {}",
            String::from_utf8_lossy(&seal_output.stderr)
        )
        .into());
    }

    Ok(())
}

// Helper to count commands in a database
fn count_commands(db_path: &Path) -> Result<usize> {
    use rusqlite::Connection;
    let conn = Connection::open(db_path)?;
    let count: usize =
        conn.prepare("SELECT COUNT(*) FROM command_history")?.query_row([], |row| row.get(0))?;
    Ok(count)
}

// Helper to spawn connected sync processes for testing stdin/stdout mode
// Creates two pxh processes with their stdin/stdout cross-connected:
// - Server process: reads from stdin, writes to stdout
// - Client process: reads from server's stdout, writes to server's stdin
// This allows bidirectional communication between client and server for sync testing
fn spawn_sync_processes(
    client_args: Vec<String>,
    server_args: Vec<String>,
) -> Result<(std::process::Child, std::process::Child)> {
    let mut server = pxh_command()
        .args(server_args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let server_stdin = server.stdin.take().expect("Failed to get server stdin");
    let server_stdout = server.stdout.take().expect("Failed to get server stdout");

    // Brief delay to ensure server process is initialized before client connects
    std::thread::sleep(std::time::Duration::from_millis(50));

    let client = pxh_command()
        .args(client_args)
        .stdin(server_stdout)
        .stdout(server_stdin)
        .stderr(Stdio::piped())
        .spawn()?;

    Ok((client, server))
}

// Helper to create a database with standard test commands
fn create_test_db_with_commands(db_path: &Path, commands: &[&str]) -> Result<()> {
    for (i, command) in commands.iter().enumerate() {
        insert_test_command(db_path, command, None)?;
        if i < commands.len() - 1 {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    Ok(())
}

// Helper to create a pair of databases for sync testing
fn create_test_db_pair(
    temp_dir: &Path,
    client_commands: &[&str],
    server_commands: &[&str],
) -> Result<(std::path::PathBuf, std::path::PathBuf)> {
    let client_db = temp_dir.join("client.db");
    let server_db = temp_dir.join("server.db");

    create_test_db_with_commands(&client_db, client_commands)?;
    create_test_db_with_commands(&server_db, server_commands)?;

    Ok((client_db, server_db))
}

// =============================================================================
// Directory-based sync tests
// =============================================================================

#[test]
fn test_directory_sync() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let sync_dir = temp_dir.path().join("sync_dir");
    std::fs::create_dir(&sync_dir)?;

    let db1 = sync_dir.join("db1.db");
    let db2 = sync_dir.join("db2.db");
    let output_db = temp_dir.path().join("output.db");

    // Create commands in db1
    insert_test_command(&db1, "echo from_db1_1", None)?;
    insert_test_command(&db1, "echo from_db1_2", None)?;

    // Create commands in db2
    insert_test_command(&db2, "echo from_db2_1", None)?;
    insert_test_command(&db2, "echo from_db2_2", None)?;

    // Sync databases from sync_dir
    let output = pxh_command()
        .args(&["--db", output_db.to_str().unwrap(), "sync", sync_dir.to_str().unwrap()])
        .output()?;

    assert!(output.status.success());

    // Verify merged database has all commands
    assert_eq!(count_commands(&output_db)?, 4);

    Ok(())
}

#[test]
fn test_directory_sync_ignores_since() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let source_db = temp_dir.path().join("source.db");
    let dest_db = temp_dir.path().join("dest.db");

    // Create commands with different ages
    insert_test_command(&source_db, "old command 1", Some(10))?;
    insert_test_command(&source_db, "mid command 1", Some(5))?;
    insert_test_command(&source_db, "recent command 1", Some(1))?;
    insert_test_command(&source_db, "current command 1", Some(0))?;

    // Sync with --since (should be ignored for directory sync)
    let output = pxh_command()
        .args(&[
            "--db",
            dest_db.to_str().unwrap(),
            "sync",
            "--since",
            "3",
            temp_dir.path().to_str().unwrap(),
        ])
        .output()?;

    assert!(output.status.success());

    // Should have ALL 4 commands (--since is ignored)
    assert_eq!(count_commands(&dest_db)?, 4);

    Ok(())
}

// =============================================================================
// Remote sync tests (stdin/stdout mode)
// =============================================================================

#[test]
fn test_bidirectional_sync_via_stdin_stdout() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let (client_db, server_db) = create_test_db_pair(
        temp_dir.path(),
        &["echo client1", "echo client2", "echo unique_client"],
        &["echo server1", "echo server2", "echo unique_server"],
    )?;

    // Spawn connected processes
    let server_args = vec![
        "--db".to_string(),
        server_db.to_str().unwrap().to_string(),
        "sync".to_string(),
        "--server".to_string(),
    ];

    let client_args = vec![
        "--db".to_string(),
        client_db.to_str().unwrap().to_string(),
        "sync".to_string(),
        "--stdin-stdout".to_string(),
    ];

    let (client, server) = spawn_sync_processes(client_args, server_args)?;

    let client_output = client.wait_with_output()?;
    let server_output = server.wait_with_output()?;

    assert!(
        client_output.status.success(),
        "Client failed: {}",
        String::from_utf8_lossy(&client_output.stderr)
    );
    assert!(
        server_output.status.success(),
        "Server failed: {}",
        String::from_utf8_lossy(&server_output.stderr)
    );

    // Check command counts
    let client_count = count_commands(&client_db)?;
    let server_count = count_commands(&server_db)?;

    // Both databases should have the same number of commands after bidirectional sync
    assert_eq!(client_count, server_count);

    // We expect 6 unique commands total:
    // client1, client2, unique_client, server1, server2, unique_server
    assert_eq!(client_count, 6);

    Ok(())
}

#[test]
fn test_send_only_sync() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let (client_db, server_db) =
        create_test_db_pair(temp_dir.path(), &["echo from_client1", "echo from_client2"], &[])?;

    let server_args = vec![
        "--db".to_string(),
        server_db.to_str().unwrap().to_string(),
        "sync".to_string(),
        "--server".to_string(),
    ];

    let client_args = vec![
        "--db".to_string(),
        client_db.to_str().unwrap().to_string(),
        "sync".to_string(),
        "--stdin-stdout".to_string(),
        "--send-only".to_string(),
    ];

    let (client, server) = spawn_sync_processes(client_args, server_args)?;

    let client_output = client.wait_with_output()?;
    let server_output = server.wait_with_output()?;

    assert!(
        client_output.status.success(),
        "Client failed: {}",
        String::from_utf8_lossy(&client_output.stderr)
    );
    assert!(
        server_output.status.success(),
        "Server failed: {}",
        String::from_utf8_lossy(&server_output.stderr)
    );

    // Server should have client's commands
    assert_eq!(count_commands(&server_db)?, 2);
    // Client should still have only its original commands
    assert_eq!(count_commands(&client_db)?, 2);

    Ok(())
}

#[test]
fn test_receive_only_sync() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let (client_db, server_db) =
        create_test_db_pair(temp_dir.path(), &[], &["echo from_server1", "echo from_server2"])?;

    let server_args = vec![
        "--db".to_string(),
        server_db.to_str().unwrap().to_string(),
        "sync".to_string(),
        "--server".to_string(),
    ];

    let client_args = vec![
        "--db".to_string(),
        client_db.to_str().unwrap().to_string(),
        "sync".to_string(),
        "--stdin-stdout".to_string(),
        "--receive-only".to_string(),
    ];

    let (client, server) = spawn_sync_processes(client_args, server_args)?;

    let client_output = client.wait_with_output()?;
    let server_output = server.wait_with_output()?;

    assert!(
        client_output.status.success(),
        "Client failed: {}",
        String::from_utf8_lossy(&client_output.stderr)
    );
    assert!(
        server_output.status.success(),
        "Server failed: {}",
        String::from_utf8_lossy(&server_output.stderr)
    );

    // Client should have server's commands
    assert_eq!(count_commands(&client_db)?, 2);
    // Server should still have only its original commands
    assert_eq!(count_commands(&server_db)?, 2);

    Ok(())
}

#[test]
fn test_sync_with_since_option() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let client_db = temp_dir.path().join("client.db");
    let server_db = temp_dir.path().join("server.db");

    // Create old and new commands in both databases
    insert_test_command(&client_db, "echo old_client", Some(10))?;
    insert_test_command(&client_db, "echo recent_client", Some(1))?;

    insert_test_command(&server_db, "echo old_server", Some(10))?;
    insert_test_command(&server_db, "echo medium_server", Some(5))?;
    insert_test_command(&server_db, "echo recent_server", Some(1))?;

    let server_args = vec![
        "--db".to_string(),
        server_db.to_str().unwrap().to_string(),
        "sync".to_string(),
        "--server".to_string(),
        "--since".to_string(),
        "7".to_string(),
    ];

    let client_args = vec![
        "--db".to_string(),
        client_db.to_str().unwrap().to_string(),
        "sync".to_string(),
        "--stdin-stdout".to_string(),
        "--since".to_string(),
        "7".to_string(),
    ];

    let (client, server) = spawn_sync_processes(client_args, server_args)?;

    let client_output = client.wait_with_output()?;
    let server_output = server.wait_with_output()?;

    assert!(
        client_output.status.success(),
        "Client failed: {}",
        String::from_utf8_lossy(&client_output.stderr)
    );
    assert!(
        server_output.status.success(),
        "Server failed: {}",
        String::from_utf8_lossy(&server_output.stderr)
    );

    // Client should have its old command plus recent commands from both
    assert_eq!(count_commands(&client_db)?, 4); // old_client, recent_client, medium_server, recent_server

    // Server should have all its commands plus recent client command
    assert_eq!(count_commands(&server_db)?, 4); // old_server, medium_server, recent_server, recent_client

    Ok(())
}

#[test]
fn test_sync_error_handling() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let (client_db, server_db) =
        create_test_db_pair(temp_dir.path(), &["echo test"], &["echo test"])?;

    // Test with conflicting options
    let server_args = vec![
        "--db".to_string(),
        server_db.to_str().unwrap().to_string(),
        "sync".to_string(),
        "--server".to_string(),
    ];

    let client_args = vec![
        "--db".to_string(),
        client_db.to_str().unwrap().to_string(),
        "sync".to_string(),
        "--stdin-stdout".to_string(),
        "--send-only".to_string(),
        "--receive-only".to_string(),
    ];

    let result = spawn_sync_processes(client_args, server_args);

    match result {
        Ok((client, _server)) => {
            let client_output = client.wait_with_output()?;
            // Client should fail due to conflicting options
            assert!(!client_output.status.success());
        }
        Err(_) => {
            // Expected failure during spawn
        }
    }

    Ok(())
}

// =============================================================================
// SSH sync tests
// =============================================================================

#[test]
fn test_ssh_sync_command_parsing() -> Result<()> {
    // Test that SSH sync commands are properly formed
    let output = pxh_command().args(&["sync", "--remote", "user@host", "--help"]).output()?;

    // Should show help without error
    assert!(String::from_utf8(output.stdout)?.contains("Remote host to sync with"));

    Ok(())
}
