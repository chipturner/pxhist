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
