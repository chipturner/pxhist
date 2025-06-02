use std::{
    env,
    fs::{self, File},
    path::PathBuf,
};

use assert_cmd::Command;
use bstr::BString;
use rand::Rng;
use rusqlite::Connection;
use tempfile::TempDir;

mod common;
use common::PxhCaller;

fn count_lines(bytes: &[u8]) -> usize {
    bytes.iter().filter(|&ch| *ch == b'\n').count()
}

#[test]
fn trivial_invocation() {
    let mut naked_cmd = Command::cargo_bin("pxh").unwrap();
    naked_cmd.env("PXH_DB_PATH", ":memory:").assert().failure();
    let mut show_cmd = Command::cargo_bin("pxh").unwrap();
    show_cmd
        .env_clear()
        .env("PXH_DB_PATH", ":memory:")
        .arg("show")
        .arg("--suppress-headers")
        .assert()
        .success();

    let pc = PxhCaller::new();
    pc.call("insert --shellname zsh --hostname testhost --username testuser --session-id 12345678 test_command_1")
        .assert()
        .success();

    pc.call("insert --shellname zsh --hostname testhost --username testuser --session-id 12345678 test_command_2")
        .assert()
        .success();

    pc.call("export").assert().success();

    // Ensure we see our history with show w/o a regex, don't see it
    // with a valid one, and see it with multiple joined regexes
    let output = pc.call("show --suppress-headers").output().unwrap();
    assert_eq!(count_lines(&output.stdout), 2);

    let output = pc.call("show --suppress-headers non-matching-regex").output().unwrap();
    assert_eq!(count_lines(&output.stdout), 0);

    let output = pc.call("show --suppress-headers test").output().unwrap();
    assert_eq!(count_lines(&output.stdout), 2);

    // Make sure we properly filter by joining regexes (which would then not match)
    let output = pc.call("show --suppress-headers command_1 command_2").output().unwrap();
    assert_eq!(count_lines(&output.stdout), 0);
}

#[test]
fn show_with_here() {
    let mut naked_cmd = Command::cargo_bin("pxh").unwrap();
    naked_cmd.env("PXH_DB_PATH", ":memory:").assert().failure();
    let mut show_cmd = Command::cargo_bin("pxh").unwrap();
    show_cmd
        .env_clear()
        .env("PXH_DB_PATH", ":memory:")
        .arg("show")
        .arg("--suppress-headers")
        .assert()
        .success();

    // Prepare some test data: four commands, three from /dirN and one
    // from wherever the test runs.
    let pc = PxhCaller::new();
    for i in 1..=3 {
        let cmd = format!(
            "insert --shellname s --hostname h --username u --session-id 1 --working-directory /dir{i} test_command_{i}"
        );
        pc.call(cmd).assert().success();
    }
    let cmd = format!(
        "insert --shellname s --hostname h --username u --session-id 1 --working-directory {} test_command_cwd",
        env::current_dir().unwrap_or_default().to_string_lossy()
    );
    pc.call(cmd).assert().success();

    // Now make sure we only see the relevant results when --here is
    // provided, both with and without --working-directory
    let output = pc.call("show --suppress-headers --here").output().unwrap();
    assert_eq!(count_lines(&output.stdout), 1);

    for i in 1..=3 {
        let cmd =
            format!("show --suppress-headers --here --working-directory /dir{i} test_command_{i}");
        let output = pc.call(cmd).output().unwrap();
        assert_eq!(count_lines(&output.stdout), 1);
    }
}

#[test]
fn show_with_loosen() {
    let mut naked_cmd = Command::cargo_bin("pxh").unwrap();
    naked_cmd.env("PXH_DB_PATH", ":memory:").assert().failure();
    let mut show_cmd = Command::cargo_bin("pxh").unwrap();
    show_cmd.env_clear().env("PXH_DB_PATH", ":memory:").arg("show").assert().success();

    // Prepare some test data: three commands of the form test.*xyz
    let pc = PxhCaller::new();
    for i in 1..=3 {
        let cmd = format!(
            "insert --shellname s --hostname h --username u --session-id {i} test_command_{i} xyz"
        );
        pc.call(cmd).assert().success();
    }

    // Verify we see all three commands with traditional show
    let output = pc.call("show --suppress-headers test xyz").output().unwrap();
    assert_eq!(count_lines(&output.stdout), 3);

    // Now verify we see none if we invert the order
    let output = pc.call("show --suppress-headers xyz test").output().unwrap();
    assert_eq!(count_lines(&output.stdout), 0);

    // Finally, the real test: loosen makes them show back up again
    let output = pc.call("show --suppress-headers --loosen xyz test").output().unwrap();
    assert_eq!(count_lines(&output.stdout), 3);
}

#[test]
fn show_with_session_id() {
    let mut naked_cmd = Command::cargo_bin("pxh").unwrap();
    naked_cmd.env("PXH_DB_PATH", ":memory:").assert().failure();
    let mut show_cmd = Command::cargo_bin("pxh").unwrap();
    show_cmd.env_clear().env("PXH_DB_PATH", ":memory:").arg("show").assert().success();

    // Prepare some test data: four commands spread across three sessions.
    let pc = PxhCaller::new();
    for i in 1..=3 {
        let cmd = format!(
            "insert --shellname s --hostname h --username u --session-id {i} test_command_{i}"
        );
        pc.call(cmd).assert().success();
    }
    let cmd = "insert --shellname s --hostname h --username u --session-id 1 test_command_4";
    pc.call(cmd).assert().success();

    // Now make sure we only see the relevant results when we specify
    // sessions to `show`.  First make sure we see all commands:
    let output = pc.call("show --suppress-headers").output().unwrap();
    assert_eq!(count_lines(&output.stdout), 4);

    // Now two in session 1
    let output = pc.call("show --suppress-headers --session 1").output().unwrap();
    assert_eq!(count_lines(&output.stdout), 2);

    // Finally, one in sessions 2 and 3
    for i in 2..=3 {
        let cmd = format!("show --suppress-headers --session {i}");
        let output = pc.call(cmd).output().unwrap();
        assert_eq!(count_lines(&output.stdout), 1);
    }
}

#[test]
fn show_with_limit() {
    let mut naked_cmd = Command::cargo_bin("pxh").unwrap();
    naked_cmd.env("PXH_DB_PATH", ":memory:").assert().failure();
    let mut show_cmd = Command::cargo_bin("pxh").unwrap();
    show_cmd.env_clear().env("PXH_DB_PATH", ":memory:").arg("show").assert().success();

    // Prepare some test data: 100 test commands
    let pc = PxhCaller::new();
    for i in 1..=100 {
        let cmd = format!(
            "insert --shellname s --hostname h --username u --session-id {i} test_command_{i}"
        );
        pc.call(cmd).assert().success();
    }

    // Verify we see all three commands with traditional show
    let output = pc.call("show --suppress-headers").output().unwrap();
    assert_eq!(count_lines(&output.stdout), 50);

    // Verify explicit limit 0 gives all results
    let output = pc.call("show --suppress-headers --limit 0").output().unwrap();
    assert_eq!(count_lines(&output.stdout), 100);
}

#[test]
fn show_with_case_insensitive() {
    let mut naked_cmd = Command::cargo_bin("pxh").unwrap();
    naked_cmd.env("PXH_DB_PATH", ":memory:").assert().failure();
    let mut show_cmd = Command::cargo_bin("pxh").unwrap();
    show_cmd.env_clear().env("PXH_DB_PATH", ":memory:").arg("show").assert().success();

    // Prepare some test data: three commands with mixed case
    let pc = PxhCaller::new();
    for i in 1..=3 {
        let cmd = format!(
            "insert --shellname s --hostname h --username u --session-id {i} TEST_command_{i}"
        );
        pc.call(cmd).assert().success();
    }

    // Test case-sensitive search (should find only exact match)
    let output = pc.call("show --suppress-headers test_command").output().unwrap();
    assert_eq!(count_lines(&output.stdout), 0);

    // Test case-insensitive search (should find all variations)
    let output = pc.call("show --suppress-headers --ignore-case test_command").output().unwrap();
    assert_eq!(count_lines(&output.stdout), 3);

    // Test with multiple patterns case-insensitive
    let output = pc.call("show --suppress-headers --ignore-case TEST_COMMAND").output().unwrap();
    assert_eq!(count_lines(&output.stdout), 3);

    // Test that uppercase pattern is converted to lowercase
    let output = pc.call("show --suppress-headers --ignore-case TEST_COMMAND_1").output().unwrap();
    assert_eq!(count_lines(&output.stdout), 1);
    let output = pc.call("show --suppress-headers --ignore-case test_command_1").output().unwrap();
    assert_eq!(count_lines(&output.stdout), 1);

    // Verify case-sensitive still works
    let output = pc.call("show --suppress-headers TEST").output().unwrap();
    assert_eq!(count_lines(&output.stdout), 3);
}

// Basic round trip test of inserting/sealing, then verify with json export.
#[test]
fn insert_seal_roundtrip() {
    let pc = PxhCaller::new();
    let commands = vec!["df", "sleep 1", "uptime"];
    for command in &commands {
        pc.call(format!(
	    "insert --shellname zsh --hostname testhost --username testuser --session-id 12345678 --start-unix-timestamp 1653573011 {command}"
	))
	    .assert()
	    .success();

        pc.call("seal --session-id 12345678 --exit-status 0 --end-unix-timestamp 1653573011")
            .assert()
            .success();
    }

    let output = pc.call("show --suppress-headers").output().unwrap();

    assert!(!output.stdout.is_empty());
    assert_eq!(count_lines(&output.stdout), commands.len());

    // Trivial regexp
    let output = pc.call("show --suppress-headers u....Z?e").output().unwrap();
    assert_eq!(count_lines(&output.stdout), 1,);

    let json_output = pc.call("export").output().unwrap();
    let invocations: Vec<pxh::Invocation> =
        serde_json::from_slice(json_output.stdout.as_slice()).unwrap();
    assert_eq!(invocations.len(), commands.len());
    for (idx, val) in invocations.iter().enumerate() {
        assert_eq!(val.command, commands[idx]);
    }
}

// Verify a given invocation list matches what we expect.  The data is
// a bit of a torture test of non-utf8 data, spaces, etc.
fn matches_expected_history(invocations: &[pxh::Invocation]) {
    let expected = vec![
        BString::from(r#"echo $'this "is" \'a\' \\n test\n\nboo'"#.to_string()),
        BString::from("fd zsh".to_string()),
        BString::from(
            [101, 99, 104, 111, 32, 0xf0, 0xce, 0xb1, 0xce, 0xa5, 0xef, 0xbd, 0xa9].to_vec(),
        ),
    ];

    assert_eq!(invocations.len(), expected.len());

    for (idx, val) in invocations.iter().enumerate() {
        assert_eq!(expected[idx], val.command);
    }
}

// Test cases for multiple shell history format roundtrips.

#[test]
fn zsh_import_roundtrip() {
    let resources = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/resources");
    let pc = PxhCaller::new();
    pc.call(format!(
        "import --shellname zsh --histfile {}",
        resources.join("zsh_histfile").to_string_lossy()
    ))
    .assert()
    .success();

    let output = pc.call("show --suppress-headers").output().unwrap();

    assert!(!output.stdout.is_empty());
    assert_eq!(count_lines(&output.stdout), 3);

    let json_output = pc.call("export").output().unwrap();
    let invocations: Vec<pxh::Invocation> =
        serde_json::from_slice(json_output.stdout.as_slice()).unwrap();
    matches_expected_history(&invocations);
}

#[test]
fn bash_import_roundtrip() {
    let resources = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/resources");
    let pc = PxhCaller::new();
    pc.call(format!(
        "import --shellname bash --histfile {}",
        resources.join("simple_bash_histfile").to_string_lossy()
    ))
    .assert()
    .success();

    let output = pc.call("show --suppress-headers").output().unwrap();

    assert!(!output.stdout.is_empty());
    assert_eq!(count_lines(&output.stdout), 3);

    let json_output = pc.call("export").output().unwrap();
    let invocations: Vec<pxh::Invocation> =
        serde_json::from_slice(json_output.stdout.as_slice()).unwrap();
    matches_expected_history(&invocations);
}

#[test]
fn timestamped_bash_import_roundtrip() {
    let resources = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/resources");
    let pc = PxhCaller::new();
    pc.call(format!(
        "import --shellname bash --histfile {}",
        resources.join("timestamped_bash_histfile").to_string_lossy()
    ))
    .assert()
    .success();

    let output = pc.call("show --suppress-headers").output().unwrap();

    assert!(!output.stdout.is_empty());
    assert_eq!(count_lines(&output.stdout), 3);

    let json_output = pc.call("export").output().unwrap();
    let invocations: Vec<pxh::Invocation> =
        serde_json::from_slice(json_output.stdout.as_slice()).unwrap();
    matches_expected_history(&invocations);
}

#[test]
fn install_command() {
    let tmpdir = TempDir::new().unwrap();
    let home = tmpdir.path();

    // Create empty RC files
    let zshrc = home.join(".zshrc");
    let bashrc = home.join(".bashrc");
    File::create(&zshrc).unwrap();
    File::create(&bashrc).unwrap();

    // Test zsh installation
    let output = Command::cargo_bin("pxh")
        .unwrap()
        .env_clear()
        .env("HOME", home)
        .args(["install", "zsh"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let zshrc_content = fs::read_to_string(&zshrc).unwrap();
    assert!(zshrc_content.contains("pxh shell-config zsh"));

    // Test bash installation
    let output = Command::cargo_bin("pxh")
        .unwrap()
        .env_clear()
        .env("HOME", home)
        .args(["install", "bash"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let bashrc_content = fs::read_to_string(&bashrc).unwrap();
    assert!(bashrc_content.contains("pxh shell-config bash"));

    // Test invalid shell
    let output = Command::cargo_bin("pxh")
        .unwrap()
        .env_clear()
        .env("HOME", home)
        .args(["install", "invalid"])
        .output()
        .unwrap();

    assert!(!output.status.success());
}

#[test]
fn shell_config_command() {
    // Test zsh config output
    let output = Command::cargo_bin("pxh")
        .unwrap()
        .env_clear()
        .args(["shell-config", "zsh"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stdout.len() > 0);
    assert!(String::from_utf8_lossy(&output.stdout).contains("_pxh_addhistory"));
    assert!(String::from_utf8_lossy(&output.stdout).contains("add-zsh-hook"));

    // Test bash config output
    let output = Command::cargo_bin("pxh")
        .unwrap()
        .env_clear()
        .args(["shell-config", "bash"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stdout.len() > 0);
    assert!(String::from_utf8_lossy(&output.stdout).contains("preexec()"));
    assert!(String::from_utf8_lossy(&output.stdout).contains("bash-preexec.sh"));

    // Test invalid shell
    let output = Command::cargo_bin("pxh")
        .unwrap()
        .env_clear()
        .args(["shell-config", "invalid"])
        .output()
        .unwrap();

    assert!(!output.status.success());
}

#[test]
fn scrub_command() {
    let mut naked_cmd = Command::cargo_bin("pxh").unwrap();
    naked_cmd.env("PXH_DB_PATH", ":memory:").assert().failure();
    let mut show_cmd = Command::cargo_bin("pxh").unwrap();
    show_cmd.env_clear().env("PXH_DB_PATH", ":memory:").arg("show").assert().success();

    // Prepare some test data: 10 test commands
    let pc = PxhCaller::new();
    for i in 1..=10 {
        let cmd = format!(
            "insert --shellname s --hostname h --username u --session-id {i} test_command_{i}"
        );
        pc.call(cmd).assert().success();
    }

    // Verify the rows are present
    let output = pc.call("show --suppress-headers").output().unwrap();
    assert_eq!(count_lines(&output.stdout), 10);

    // Scrub `test_command_10`
    let _output = pc.call("scrub test_command_10").output().unwrap();

    // Verify we have 9 rows now.
    let output = pc.call("show --suppress-headers").output().unwrap();
    assert_eq!(count_lines(&output.stdout), 9);

    // Scrub the rest
    let _output = pc.call("scrub test_command_").output().unwrap();

    // Verify we have none.
    let output = pc.call("show --suppress-headers").output().unwrap();
    assert_eq!(count_lines(&output.stdout), 0);
}

#[test]
fn symlink_pxhs_behavior() {
    // Create a temporary directory for our symlinks
    let tempdir = TempDir::new().unwrap();
    let pxh_path = tempdir.path().join("pxh");
    let pxhs_path = tempdir.path().join("pxhs");

    // Get the actual binary path and create symlinks
    let bin_path = Command::cargo_bin("pxh").unwrap().get_program().to_string_lossy().to_string();
    std::os::unix::fs::symlink(&bin_path, &pxh_path).unwrap();
    std::os::unix::fs::symlink(&pxh_path, &pxhs_path).unwrap();

    // Create a PxhCaller for our test
    let pc = PxhCaller::new();

    // Insert test data
    pc.call("insert --shellname zsh --hostname testhost --username testuser --session-id 12345678 test_command_1")
        .assert()
        .success();

    // Make sure the data is properly sealed with exit status
    pc.call("seal --session-id 12345678 --exit-status 0 --end-unix-timestamp 1600000000")
        .assert()
        .success();

    // Test 1: Verify our test data using the regular pxh command
    let base_output = pc.call("show --suppress-headers").output().unwrap();
    assert!(base_output.status.success());
    assert!(String::from_utf8_lossy(&base_output.stdout).contains("test_command_1"));

    // Test 2: pxhs with search term should inject "show" and work like "pxh show"
    let shorthand_output = Command::new(&pxhs_path)
        .env("PXH_DB_PATH", &pc.tmpdir().join("test"))
        .env("PXH_HOSTNAME", "testhost")
        .args(["test_command"])
        .output()
        .unwrap();

    assert!(shorthand_output.status.success());
    let shorthand_str = String::from_utf8_lossy(&shorthand_output.stdout);
    assert!(
        shorthand_str.contains("test_command_1"),
        "The shorthand form pxhs should act like pxh show"
    );

    // Test 3: pxhs with "--help" should work correctly and show help for the show command
    let help_output = Command::new(&pxhs_path)
        .env("PXH_DB_PATH", &pc.tmpdir().join("test"))
        .args(["--help"])
        .output()
        .unwrap();

    assert!(help_output.status.success());
    let help_str = String::from_utf8_lossy(&help_output.stdout);
    assert!(
        help_str.contains("search for and display history entries"),
        "Help output should include the show command description"
    );
}

#[test]
fn sync_roundtrip() {
    // Prepare some test data: 40 test commands
    let pc_even = PxhCaller::new();
    let pc_odd = PxhCaller::new();
    for i in 1..=40 {
        let cmd = format!(
            "insert --shellname s --hostname h --username u --working-directory d --start-unix-timestamp 1 --session-id {i} test_command_{i}",
        );
        if i % 2 == 0 {
            pc_even.call(cmd).assert().success();
        } else {
            pc_odd.call(cmd).assert().success();
        }
    }

    let sync_dir = TempDir::new().unwrap();
    let sync_cmd = format!("sync {}", sync_dir.path().to_string_lossy());
    pc_even.call(&sync_cmd).assert().success();
    pc_odd.call(&sync_cmd).assert().success();

    let even_output = pc_even.call("show --suppress-headers").output().unwrap();
    let even_odd_output = pc_odd.call("show --suppress-headers").output().unwrap();

    assert_eq!(count_lines(&even_output.stdout), 20);
    assert_eq!(count_lines(&even_odd_output.stdout), 40); // 40, not 20!  because the sync pulled in the 20 from the even sync above

    // For thoroughness case, let's see we pull in both files (total
    // of 60 entries) and properly dedupe into 40 just like the
    // even_odd case above.
    let pc_merged = PxhCaller::new();
    pc_merged.call(&sync_cmd).assert().success();
    let merged_output = pc_merged.call("show --suppress-headers").output().unwrap();

    assert_eq!(count_lines(&merged_output.stdout), 40);
}

#[test]
fn test_maintenance() {
    // Set up a new database with varied content but reduced size for faster testing
    let pc = PxhCaller::new();

    // Get the database path for SQLite access
    let db_path = pc.tmpdir().join("test");

    // Direct database access is faster than CLI for setup
    {
        // Create a direct database connection for faster setup
        let mut conn = Connection::open(&db_path).unwrap();

        // Set pragmas for faster operation during test setup
        conn.execute_batch(
            "
            PRAGMA synchronous = OFF;
            PRAGMA journal_mode = MEMORY;
            PRAGMA temp_store = MEMORY;
            PRAGMA cache_size = 10000;
        ",
        )
        .unwrap();

        // Create tables and schema
        conn.execute_batch(include_str!("../src/base_schema.sql")).unwrap();

        // Begin a transaction for bulk inserts (much faster)
        let tx = conn.transaction().unwrap();

        // Generate commands with varied metadata but fewer of them
        let num_commands = 3000; // Significantly reduced but still enough for testing
        let mut rng = rand::rng();

        // Working directories
        let working_dirs = ["/home/user", "/var/log", "/etc", "/tmp"];

        // Batch insert commands
        for i in 1..=num_commands {
            // Create session ID (grouped in batches)
            let session_id = i / 10 + 1;

            // Vary shell, hostname, username
            let shell = if i % 3 == 0 { "zsh" } else { "bash" };
            let hostname = format!("host{}", i % 3 + 1);
            let username = format!("user{}", i % 2 + 1);

            // Create commands
            let command = match i % 8 {
                0 => "git commit",
                1 => "ls",
                2 => "cd etc",
                3 => "cd home",
                4 => "cat file",
                5 => "uptime",
                6 => "history",
                _ => "command",
            };

            // Choose a random working directory
            let working_dir = working_dirs[rng.random_range(0..working_dirs.len())];

            // Direct SQL insert is much faster than command-line
            tx.execute(
                "INSERT INTO command_history (
                    session_id, full_command, shellname, hostname, username, 
                    working_directory, exit_status, start_unix_timestamp, end_unix_timestamp
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (
                    session_id,
                    command,
                    shell,
                    hostname,
                    username,
                    working_dir,
                    if i % 5 == 0 { None } else { Some(if i % 17 == 0 { 1 } else { 0 }) },
                    1600000000 + i,
                    if i % 5 == 0 { None } else { Some(1600000010 + i) },
                ),
            )
            .unwrap();
        }

        // Commit all inserts at once
        tx.commit().unwrap();

        // Delete a significant number of rows to create free space
        conn.execute("DELETE FROM command_history WHERE rowid % 3 = 0", []).unwrap();

        // Force creation of free space for testing VACUUM
        conn.execute("PRAGMA page_size = 4096", []).unwrap();
        conn.execute("PRAGMA incremental_vacuum(5)", []).unwrap();
    }

    // Get initial stats
    let conn = Connection::open(&db_path).unwrap();
    let initial_size: i64 = conn
        .query_row(
            "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
            [],
            |r| r.get(0),
        )
        .unwrap();

    // Count rows before maintenance
    let remaining_rows: i64 =
        conn.query_row("SELECT COUNT(*) FROM command_history", [], |r| r.get(0)).unwrap();

    println!("Database has {} rows before maintenance", remaining_rows);
    assert!(remaining_rows > 100, "Should have enough rows for testing");

    // Run the maintenance command via CLI
    println!("Running maintenance command...");
    pc.call("maintenance").assert().success();

    // Reconnect to check results
    let conn_after = Connection::open(&db_path).unwrap();

    // Verify database size after vacuum
    let after_size: i64 = conn_after
        .query_row(
            "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
            [],
            |r| r.get(0),
        )
        .unwrap();

    println!("Database size before: {} bytes, after: {} bytes", initial_size, after_size);

    // After running VACUUM, the database should be smaller and have no freelist
    let freelist_count_after: i64 =
        conn_after.query_row("PRAGMA freelist_count", [], |r| r.get(0)).unwrap();
    assert_eq!(freelist_count_after, 0, "Freelist should be empty after VACUUM");

    // Check that ANALYZE created statistics
    let stat_table_exists: i64 = conn_after
        .query_row("SELECT COUNT(*) FROM sqlite_master WHERE name = 'sqlite_stat1'", [], |r| {
            r.get(0)
        })
        .unwrap();

    assert!(stat_table_exists > 0, "ANALYZE should create the sqlite_stat1 table");

    // Verify statistics were created for our tables
    let stat_entries: i64 =
        conn_after.query_row("SELECT COUNT(*) FROM sqlite_stat1", [], |r| r.get(0)).unwrap_or(0);

    println!("Database has {} statistic entries after ANALYZE", stat_entries);
    assert!(stat_entries > 0, "sqlite_stat1 should have entries after ANALYZE");
}

#[test]
fn test_maintenance_multiple_files() {
    // Create PxhCallers for two test databases
    let pc_maint = PxhCaller::new();

    // Setup two test database files
    let db_path1 = pc_maint.tmpdir().join("test1.db");
    let db_path2 = pc_maint.tmpdir().join("test2.db");

    // Define a helper function to quickly set up a test database
    fn setup_test_db(path: &PathBuf, num_rows: usize, command_prefix: &str) -> (Connection, i64) {
        // Direct database access is faster than CLI for setup
        let mut conn = Connection::open(path).unwrap();

        // Set pragmas for faster operation during test setup
        conn.execute_batch(
            "
            PRAGMA synchronous = OFF;
            PRAGMA journal_mode = MEMORY;
            PRAGMA temp_store = MEMORY;
            PRAGMA cache_size = 10000;
        ",
        )
        .unwrap();

        // Create tables and schema
        conn.execute_batch(include_str!("../src/base_schema.sql")).unwrap();

        // Begin a transaction for bulk inserts (much faster)
        let tx = conn.transaction().unwrap();

        // Batch insert commands
        for i in 1..=num_rows {
            // Insert with minimal varied data for testing
            let session_id = i / 10 + 1;
            let shellname = if i % 2 == 0 { "zsh" } else { "bash" };
            let hostname = format!("host{}", i % 2 + 1);
            let username = format!("user{}", i % 2 + 1);
            let command = format!("{}_{}", command_prefix, i);

            // Direct SQL insert is much faster than command-line
            tx.execute(
                "INSERT INTO command_history (
                    session_id, full_command, shellname, hostname, username, 
                    working_directory, exit_status, start_unix_timestamp, end_unix_timestamp
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (
                    session_id,
                    command,
                    shellname,
                    hostname,
                    username,
                    "/tmp",
                    Some(0),
                    1600000000 + i as i64,
                    Some(1600000010 + i as i64),
                ),
            )
            .unwrap();
        }

        // Commit all inserts at once
        tx.commit().unwrap();

        // Delete some rows to create free space
        conn.execute("DELETE FROM command_history WHERE rowid % 3 = 0", []).unwrap();

        // Force creation of free space with PRAGMA settings
        conn.execute("PRAGMA page_size = 4096", []).unwrap();
        conn.execute("PRAGMA incremental_vacuum(5)", []).unwrap();

        // Get row count
        let row_count = conn
            .query_row("SELECT COUNT(*) FROM command_history", [], |r| r.get::<_, i64>(0))
            .unwrap();

        (conn, row_count)
    }

    // Setup test databases
    let (_conn1, rows_before1) = setup_test_db(&db_path1, 300, "command_db1");
    let (_conn2, rows_before2) = setup_test_db(&db_path2, 300, "command_db2");

    println!("Database 1: {} rows, Database 2: {} rows", rows_before1, rows_before2);

    // Run the maintenance command on both databases
    let maintenance_cmd =
        format!("maintenance {} {}", db_path1.to_string_lossy(), db_path2.to_string_lossy());
    pc_maint.call(&maintenance_cmd).assert().success();

    // Reconnect to check results
    let conn1_after = Connection::open(&db_path1).unwrap();
    let conn2_after = Connection::open(&db_path2).unwrap();

    // Count rows after maintenance to ensure we didn't lose data
    let rows_after1: i64 =
        conn1_after.query_row("SELECT COUNT(*) FROM command_history", [], |r| r.get(0)).unwrap();
    let rows_after2: i64 =
        conn2_after.query_row("SELECT COUNT(*) FROM command_history", [], |r| r.get(0)).unwrap();

    println!(
        "After maintenance - Database 1: {} rows, Database 2: {} rows",
        rows_after1, rows_after2
    );
    assert_eq!(rows_before1, rows_after1, "Row count should be the same after maintenance for DB1");
    assert_eq!(rows_before2, rows_after2, "Row count should be the same after maintenance for DB2");

    // After running VACUUM, the databases should have no freelist
    let freelist_count1_after: i64 =
        conn1_after.query_row("PRAGMA freelist_count", [], |r| r.get(0)).unwrap();
    let freelist_count2_after: i64 =
        conn2_after.query_row("PRAGMA freelist_count", [], |r| r.get(0)).unwrap();

    assert_eq!(freelist_count1_after, 0, "Freelist should be empty in DB1 after VACUUM");
    assert_eq!(freelist_count2_after, 0, "Freelist should be empty in DB2 after VACUUM");

    // Check that ANALYZE created statistics in both databases
    let stat_table_exists1: i64 = conn1_after
        .query_row("SELECT COUNT(*) FROM sqlite_master WHERE name = 'sqlite_stat1'", [], |r| {
            r.get(0)
        })
        .unwrap();
    let stat_table_exists2: i64 = conn2_after
        .query_row("SELECT COUNT(*) FROM sqlite_master WHERE name = 'sqlite_stat1'", [], |r| {
            r.get(0)
        })
        .unwrap();

    assert!(stat_table_exists1 > 0, "ANALYZE should create the sqlite_stat1 table in DB1");
    assert!(stat_table_exists2 > 0, "ANALYZE should create the sqlite_stat1 table in DB2");

    // Verify statistics were created
    let stat_entries1: i64 =
        conn1_after.query_row("SELECT COUNT(*) FROM sqlite_stat1", [], |r| r.get(0)).unwrap_or(0);
    let stat_entries2: i64 =
        conn2_after.query_row("SELECT COUNT(*) FROM sqlite_stat1", [], |r| r.get(0)).unwrap_or(0);

    assert!(stat_entries1 > 0, "sqlite_stat1 should have entries in DB1 after ANALYZE");
    assert!(stat_entries2 > 0, "sqlite_stat1 should have entries in DB2 after ANALYZE");
}

#[test]
fn test_maintenance_clean_nonstandard_tables() {
    // Create database directly for faster setup
    let pc = PxhCaller::new();
    let db_path = pc.tmpdir().join("test");

    // Direct database setup is much faster than using CLI commands
    let mut conn = Connection::open(&db_path).unwrap();

    // Set pragmas for faster operation
    conn.execute_batch(
        "
        PRAGMA synchronous = OFF;
        PRAGMA journal_mode = MEMORY;
        PRAGMA temp_store = MEMORY;
        PRAGMA cache_size = 10000;
    ",
    )
    .unwrap();

    // Create standard schema
    conn.execute_batch(include_str!("../src/base_schema.sql")).unwrap();

    // Insert a minimal amount of test data (just enough for the test)
    let tx = conn.transaction().unwrap();
    for i in 1..=3 {
        tx.execute(
            "INSERT INTO command_history (
                session_id, full_command, shellname, hostname, username, 
                working_directory, exit_status, start_unix_timestamp
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            (
                i,
                format!("command{}", i),
                "bash",
                "host1",
                "user1",
                "/tmp",
                Some(0),
                1600000000 + i as i64,
            ),
        )
        .unwrap();
    }
    tx.commit().unwrap();

    // Create several non-standard tables and indexes at once
    println!("Creating non-standard tables and indexes for testing...");
    conn.execute_batch(
        "
        -- Create non-standard tables that should be removed
        CREATE TABLE temp_table1 (id INTEGER PRIMARY KEY, data TEXT);
        CREATE TABLE custom_data (id INTEGER PRIMARY KEY, name TEXT, value TEXT);
        CREATE INDEX idx_custom_data_name ON custom_data (name);
        
        -- Create tables with KEEP_ prefix that should be preserved
        CREATE TABLE KEEP_important_data (id INTEGER PRIMARY KEY, data TEXT);
        CREATE INDEX KEEP_idx_important ON KEEP_important_data (data);
        
        -- Insert some data in all the tables with a transaction
        BEGIN TRANSACTION;
        INSERT INTO temp_table1 (id, data) VALUES (1, 'temp data');
        INSERT INTO custom_data (id, name, value) VALUES 
            (1, 'setting1', 'value1'),
            (2, 'setting2', 'value2');
        INSERT INTO KEEP_important_data (id, data) VALUES (1, 'important data');
        COMMIT;
    ",
    )
    .unwrap();

    // Verify that we have created the tables and indexes
    let table_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    assert!(
        table_count >= 5,
        "Should have at least 5 tables (command_history, settings, temp_table1, custom_data, KEEP_important_data)"
    );

    // Run maintenance command
    println!("Running maintenance command...");
    pc.call("maintenance").assert().success();

    // Reconnect and check what tables remain
    let conn_after = Connection::open(&db_path).unwrap();

    // Query for all tables and indexes in one go
    let tables_after: Vec<(String, String)> = {
        let mut stmt = conn_after
            .prepare(
                "
            SELECT name, type FROM sqlite_master 
            WHERE type IN ('table', 'index') 
              AND name NOT LIKE 'sqlite_%'
            ORDER BY type, name
        ",
            )
            .unwrap();

        let rows = stmt
            .query_map([], |row| {
                let name: String = row.get(0)?;
                let type_: String = row.get(1)?;
                Ok((name, type_))
            })
            .unwrap();

        rows.collect::<Result<Vec<(String, String)>, _>>().unwrap()
    };

    // Print remaining objects for debugging
    println!("Database objects after maintenance:");
    for (name, type_) in &tables_after {
        println!("  {} ({})", name, type_);
    }

    // Helper function to check if an object exists
    let object_exists =
        |name: &str| -> bool { tables_after.iter().any(|(obj_name, _)| obj_name == name) };

    // Non-standard tables should be gone
    assert!(!object_exists("temp_table1"), "temp_table1 should have been removed");
    assert!(!object_exists("custom_data"), "custom_data should have been removed");
    assert!(
        !object_exists("idx_custom_data_name"),
        "idx_custom_data_name should have been removed"
    );

    // KEEP_ tables and indexes should still exist
    assert!(object_exists("KEEP_important_data"), "KEEP_important_data should have been preserved");
    assert!(object_exists("KEEP_idx_important"), "KEEP_idx_important should have been preserved");

    // Check that we can still use the KEEP_ table
    let keep_data_count: i64 =
        conn_after.query_row("SELECT COUNT(*) FROM KEEP_important_data", [], |r| r.get(0)).unwrap();
    assert_eq!(keep_data_count, 1, "Data in KEEP_ table should be preserved");

    // Standard tables should still exist
    assert!(object_exists("command_history"), "command_history should still exist");
    assert!(
        object_exists("idx_command_history_unique"),
        "idx_command_history_unique should still exist"
    );
}
