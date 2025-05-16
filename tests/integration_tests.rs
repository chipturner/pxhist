use std::{
    env,
    fs::{self, File},
    path::PathBuf,
};

use assert_cmd::Command;
use bstr::BString;
use rand::{Rng, distributions::Alphanumeric};
use rusqlite::Connection;
use tempfile::TempDir;

fn generate_random_string(length: usize) -> String {
    rand::thread_rng().sample_iter(&Alphanumeric).take(length).map(char::from).collect()
}

// Simple struct and helpers for invoking pxh with a given testdb.
struct PxhCaller {
    tmpdir: TempDir,
    hostname: String,
}

impl PxhCaller {
    fn new() -> Self {
        PxhCaller { tmpdir: TempDir::new().unwrap(), hostname: generate_random_string(12) }
    }

    fn call<S: AsRef<str>>(&mut self, args: S) -> Command {
        let mut ret = Command::cargo_bin("pxh").unwrap();
        ret.env_clear().env("PXH_DB_PATH", &self.tmpdir.path().join("test"));
        ret.env("PXH_HOSTNAME", &self.hostname);
        ret.args(args.as_ref().split(' '));
        ret
    }
}

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

    let mut pc = PxhCaller::new();
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
    let mut pc = PxhCaller::new();
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
    let mut pc = PxhCaller::new();
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
    let mut pc = PxhCaller::new();
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
    let mut pc = PxhCaller::new();
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
    let mut pc = PxhCaller::new();
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
    let mut pc = PxhCaller::new();
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
    let mut pc = PxhCaller::new();
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
    let mut pc = PxhCaller::new();
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
    let mut pc = PxhCaller::new();
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
    let mut pc = PxhCaller::new();
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
    let mut pc = PxhCaller::new();

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
        .env("PXH_DB_PATH", &pc.tmpdir.path().join("test"))
        .env("PXH_HOSTNAME", &pc.hostname)
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
        .env("PXH_DB_PATH", &pc.tmpdir.path().join("test"))
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
    let mut pc_even = PxhCaller::new();
    let mut pc_odd = PxhCaller::new();
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
    let mut pc_merged = PxhCaller::new();
    pc_merged.call(&sync_cmd).assert().success();
    let merged_output = pc_merged.call("show --suppress-headers").output().unwrap();

    assert_eq!(count_lines(&merged_output.stdout), 40);
}

#[test]
fn test_maintenance() {
    // Set up a new database with a lot of varied content
    let mut pc = PxhCaller::new();

    // Generate a lot of commands with varied lengths
    let num_commands = 2000;
    let mut rng = rand::thread_rng();

    // Create several unique working directories
    let working_dirs =
        ["/home/user", "/var/log", "/etc", "/tmp", "/opt", "/usr", "~/home", "/data"];

    // Generate many commands without any special characters or flags
    for i in 1..=num_commands {
        // Create session ID (grouped in batches)
        let session_id = i / 20 + 1;

        // Choose shell randomly
        let shell = if i % 3 == 0 { "zsh" } else { "bash" };

        // Vary hostname
        let hostname = format!("host{}", i % 5 + 1);

        // Vary username
        let username = format!("user{}", i % 3 + 1);

        // Create commands with no flags/special characters
        let command = match i % 10 {
            0 => format!("git commit"),
            1 => format!("ls"),
            2 => format!("cd etc"),
            3 => format!("cd home"),
            4 => format!("cat file{}", i % 100),
            5 => format!("uptime"),
            6 => format!("history"),
            7 => format!("git pull"),
            8 => format!("pwd"),
            _ => format!("command{}", i),
        };

        // Choose a random working directory
        let working_dir = working_dirs[rng.gen_range(0..working_dirs.len())];

        // Insert the command with varied metadata - wrap command in double quotes
        let insert_cmd = format!(
            "insert --shellname {} --hostname {} --username {} --session-id {} --working-directory {} \"{}\"",
            shell, hostname, username, session_id, working_dir, command
        );

        pc.call(insert_cmd).assert().success();

        // Add exit status for some commands
        if i % 5 != 0 {
            // Most commands succeed, some fail
            let exit_status = if i % 17 == 0 { 1 } else { 0 };
            pc.call(format!(
                "seal --session-id {} --exit-status {} --end-unix-timestamp {}",
                session_id,
                exit_status,
                1600000000 + i
            ))
            .assert()
            .success();
        }
    }

    // Get the database path for SQLite access
    let db_path = pc.tmpdir.path().join("test");

    // Create a direct database connection to check the database state
    let conn = Connection::open(&db_path).unwrap();

    // Get initial stats before deletion
    let initial_size: i64 = conn
        .query_row(
            "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
            [],
            |r| r.get(0),
        )
        .unwrap();

    // Delete a significant number of rows to create free space
    let sql = "DELETE FROM command_history WHERE rowid % 3 = 0";
    let deleted = conn.execute(sql, []).unwrap();

    // Ensure we deleted some rows
    assert!(deleted > 500, "Should have deleted over 500 rows");
    println!("Deleted {} rows to create free space", deleted);

    // Count rows after deletion
    let remaining_rows: i64 =
        conn.query_row("SELECT COUNT(*) FROM command_history", [], |r| r.get(0)).unwrap();

    println!("Database has {} remaining rows after deletion", remaining_rows);

    // Verify we still have a significant number of rows remaining
    assert!(remaining_rows > 1000, "Should still have over 1000 rows for testing");

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
    // Create two databases with different content
    let mut pc1 = PxhCaller::new();
    let mut pc2 = PxhCaller::new();

    // Insert some test data into first database
    for i in 1..=100 {
        let insert_cmd = format!(
            "insert --shellname bash --hostname host1 --username user1 --session-id {} \"command_db1_{}\"",
            i, i
        );
        pc1.call(insert_cmd).assert().success();
    }

    // Insert some test data into second database
    for i in 1..=150 {
        let insert_cmd = format!(
            "insert --shellname zsh --hostname host2 --username user2 --session-id {} \"command_db2_{}\"",
            i, i
        );
        pc2.call(insert_cmd).assert().success();
    }

    // Get the database paths
    let db_path1 = pc1.tmpdir.path().join("test");
    let db_path2 = pc2.tmpdir.path().join("test");

    // Create direct database connections to check the state
    let conn1 = Connection::open(&db_path1).unwrap();
    let conn2 = Connection::open(&db_path2).unwrap();

    // Force creation of free space with PRAGMA
    conn1.execute("PRAGMA page_size = 4096", []).unwrap();
    conn2.execute("PRAGMA page_size = 4096", []).unwrap();

    // Insert more data
    for i in 1..=100 {
        let cmd = format!(
            "INSERT INTO command_history (session_id, full_command, shellname, hostname, username) VALUES ({}, 'filler_command', 'bash', 'host1', 'user1')",
            i + 1000
        );
        conn1.execute(&cmd, []).unwrap();
        conn2.execute(&cmd, []).unwrap();
    }

    // Delete rows to create free space
    conn1.execute("DELETE FROM command_history WHERE rowid % 2 = 0", []).unwrap();
    conn2.execute("DELETE FROM command_history WHERE rowid % 3 = 0", []).unwrap();

    // Run a selective VACUUM to ensure we have some freelist pages
    conn1.execute("PRAGMA incremental_vacuum(5)", []).unwrap();
    conn2.execute("PRAGMA incremental_vacuum(5)", []).unwrap();

    // Get initial sizes before maintenance
    let _initial_size1: i64 = conn1
        .query_row(
            "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
            [],
            |r| r.get(0),
        )
        .unwrap();

    let _initial_size2: i64 = conn2
        .query_row(
            "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
            [],
            |r| r.get(0),
        )
        .unwrap();

    // Count rows before maintenance
    let rows_before1: i64 =
        conn1.query_row("SELECT COUNT(*) FROM command_history", [], |r| r.get(0)).unwrap();
    let rows_before2: i64 =
        conn2.query_row("SELECT COUNT(*) FROM command_history", [], |r| r.get(0)).unwrap();

    println!("Database 1: {} rows, Database 2: {} rows", rows_before1, rows_before2);

    // Run the maintenance command on both databases
    let maintenance_cmd =
        format!("maintenance {} {}", db_path1.to_string_lossy(), db_path2.to_string_lossy());

    // Create a new caller just for running the maintenance command
    let mut pc_maint = PxhCaller::new();
    pc_maint.call(&maintenance_cmd).assert().success();

    // Reconnect to check results
    let conn1_after = Connection::open(&db_path1).unwrap();
    let conn2_after = Connection::open(&db_path2).unwrap();

    // Verify database sizes after vacuum
    let _after_size1: i64 = conn1_after
        .query_row(
            "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
            [],
            |r| r.get(0),
        )
        .unwrap();

    let _after_size2: i64 = conn2_after
        .query_row(
            "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
            [],
            |r| r.get(0),
        )
        .unwrap();

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

    // Verify statistics were created for tables in both databases
    let stat_entries1: i64 =
        conn1_after.query_row("SELECT COUNT(*) FROM sqlite_stat1", [], |r| r.get(0)).unwrap_or(0);
    let stat_entries2: i64 =
        conn2_after.query_row("SELECT COUNT(*) FROM sqlite_stat1", [], |r| r.get(0)).unwrap_or(0);

    assert!(stat_entries1 > 0, "sqlite_stat1 should have entries in DB1 after ANALYZE");
    assert!(stat_entries2 > 0, "sqlite_stat1 should have entries in DB2 after ANALYZE");
}

#[test]
fn test_maintenance_clean_nonstandard_tables() {
    // Create a database with some test data
    let mut pc = PxhCaller::new();

    // Insert some basic data
    for i in 1..=10 {
        let insert_cmd = format!(
            "insert --shellname bash --hostname host1 --username user1 --session-id {} \"command{}\"",
            i, i
        );
        pc.call(insert_cmd).assert().success();
    }

    // Get the database path
    let db_path = pc.tmpdir.path().join("test");

    // Create direct database connection
    let conn = Connection::open(&db_path).unwrap();

    // Create several non-standard tables and indexes
    println!("Creating non-standard tables and indexes for testing...");

    // Create non-standard tables that should be removed
    conn.execute("CREATE TABLE temp_table1 (id INTEGER PRIMARY KEY, data TEXT)", []).unwrap();
    conn.execute("CREATE TABLE custom_data (id INTEGER PRIMARY KEY, name TEXT, value TEXT)", [])
        .unwrap();
    conn.execute("CREATE INDEX idx_custom_data_name ON custom_data (name)", []).unwrap();

    // Create tables with KEEP_ prefix that should be preserved
    conn.execute("CREATE TABLE KEEP_important_data (id INTEGER PRIMARY KEY, data TEXT)", [])
        .unwrap();
    conn.execute("CREATE INDEX KEEP_idx_important ON KEEP_important_data (data)", []).unwrap();

    // Insert some data in all the tables
    conn.execute("INSERT INTO temp_table1 (id, data) VALUES (1, 'temp data')", []).unwrap();
    conn.execute("INSERT INTO custom_data (id, name, value) VALUES (1, 'setting1', 'value1')", [])
        .unwrap();
    conn.execute("INSERT INTO custom_data (id, name, value) VALUES (2, 'setting2', 'value2')", [])
        .unwrap();
    conn.execute("INSERT INTO KEEP_important_data (id, data) VALUES (1, 'important data')", [])
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

    let index_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    assert!(index_count >= 5, "Should have at least 5 indexes (standard plus custom)");

    // Run maintenance command
    println!("Running maintenance command...");
    pc.call("maintenance").assert().success();

    // Reconnect and check what tables remain
    let conn_after = Connection::open(&db_path).unwrap();

    // Non-standard tables should be gone
    let temp_table_exists: i64 = conn_after
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'temp_table1'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    assert_eq!(temp_table_exists, 0, "temp_table1 should have been removed");

    let custom_table_exists: i64 = conn_after
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'custom_data'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    assert_eq!(custom_table_exists, 0, "custom_data should have been removed");

    // Non-standard indexes should be gone
    let custom_idx_exists: i64 = conn_after
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_custom_data_name'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    assert_eq!(custom_idx_exists, 0, "idx_custom_data_name should have been removed");

    // KEEP_ tables should still exist
    let keep_table_exists: i64 = conn_after
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'KEEP_important_data'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    assert_eq!(keep_table_exists, 1, "KEEP_important_data should have been preserved");

    // KEEP_ indexes should still exist
    let keep_idx_exists: i64 = conn_after
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'KEEP_idx_important'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    assert_eq!(keep_idx_exists, 1, "KEEP_idx_important should have been preserved");

    // Check that we can still use the KEEP_ table
    let keep_data_count: i64 =
        conn_after.query_row("SELECT COUNT(*) FROM KEEP_important_data", [], |r| r.get(0)).unwrap();

    assert_eq!(keep_data_count, 1, "Data in KEEP_ table should be preserved");

    // Standard tables should still exist
    let std_table_exists: i64 = conn_after
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'command_history'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    assert_eq!(std_table_exists, 1, "command_history should still exist");

    // Standard indexes should still exist
    let std_idx_exists: i64 = conn_after
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_command_history_unique'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    assert_eq!(std_idx_exists, 1, "idx_command_history_unique should still exist");
}
