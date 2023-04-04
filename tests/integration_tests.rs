use std::{
    env,
    fs::File,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
};

use assert_cmd::Command;
use bstr::BString;
use tempfile::{NamedTempFile, TempDir};

// Simple struct and helpers for invoking pxh with a given testdb.
struct PxhCaller {
    tmpdir: TempDir,
}

impl PxhCaller {
    fn new() -> Self {
        PxhCaller { tmpdir: TempDir::new().unwrap() }
    }

    fn call<S: AsRef<str>>(&mut self, args: S) -> Command {
        let mut ret = Command::cargo_bin("pxh").unwrap();
        ret.env_clear().env("PXH_DB_PATH", &self.tmpdir.path().join("test"));
        ret.args(args.as_ref().split(' '));
        ret
    }
}

#[test]
fn test_trivial_invocation() {
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
    assert_eq!(output.stdout.iter().filter(|&ch| *ch == b'\n').count(), 2);

    let output = pc.call("show --suppress-headers non-matching-regex").output().unwrap();
    assert_eq!(output.stdout.iter().filter(|&ch| *ch == b'\n').count(), 0);

    let output = pc.call("show --suppress-headers test").output().unwrap();
    assert_eq!(output.stdout.iter().filter(|&ch| *ch == b'\n').count(), 2);

    // Make sure we properly filter by joining regexes (which would then not match)
    let output = pc.call("show --suppress-headers command_1 command_2").output().unwrap();
    assert_eq!(output.stdout.iter().filter(|&ch| *ch == b'\n').count(), 0);
}

#[test]
fn test_show_with_here() {
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
        let cmd = format!("insert --shellname s --hostname h --username u --session-id 1 --working-directory /dir{i} test_command_{i}");
        pc.call(cmd).assert().success();
    }
    let cmd = format!("insert --shellname s --hostname h --username u --session-id 1 --working-directory {} test_command_cwd", env::current_dir().unwrap_or_default().to_string_lossy());
    pc.call(cmd).assert().success();

    // Now make sure we only see the relevant results when --here is
    // provided, both with and without --working-directory
    let output = pc.call("show --suppress-headers --here").output().unwrap();
    assert_eq!(output.stdout.iter().filter(|&ch| *ch == b'\n').count(), 1);

    for i in 1..=3 {
        let cmd =
            format!("show --suppress-headers --here --working-directory /dir{i} test_command_{i}");
        let output = pc.call(cmd).output().unwrap();
        assert_eq!(output.stdout.iter().filter(|&ch| *ch == b'\n').count(), 1);
    }
}

#[test]
fn test_show_with_loosen() {
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
    assert_eq!(output.stdout.iter().filter(|&ch| *ch == b'\n').count(), 3);

    // Now verify we see none if we invert the order
    let output = pc.call("show --suppress-headers xyz test").output().unwrap();
    assert_eq!(output.stdout.iter().filter(|&ch| *ch == b'\n').count(), 0);

    // Finally, the real test: loosen makes them show back up again
    let output = pc.call("show --suppress-headers --loosen xyz test").output().unwrap();
    assert_eq!(output.stdout.iter().filter(|&ch| *ch == b'\n').count(), 3);
}

#[test]
fn test_show_with_session_id() {
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
    assert_eq!(output.stdout.iter().filter(|&ch| *ch == b'\n').count(), 4);

    // Now two in session 1
    let output = pc.call("show --suppress-headers --session 1").output().unwrap();
    assert_eq!(output.stdout.iter().filter(|&ch| *ch == b'\n').count(), 2);

    // Finally, one in sessions 2 and 3
    for i in 2..=3 {
        let cmd = format!("show --suppress-headers --session {i}");
        let output = pc.call(cmd).output().unwrap();
        assert_eq!(output.stdout.iter().filter(|&ch| *ch == b'\n').count(), 1);
    }
}

#[test]
fn test_show_with_limit() {
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
    assert_eq!(output.stdout.iter().filter(|&ch| *ch == b'\n').count(), 50);

    // Verify explicit limit 0 gives all results
    let output = pc.call("show --suppress-headers --limit 0").output().unwrap();
    assert_eq!(output.stdout.iter().filter(|&ch| *ch == b'\n').count(), 100);
}

// Basic round trip test of inserting/sealing, then verify with json export.
#[test]
fn test_insert_seal_roundtrip() {
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
    assert_eq!(output.stdout.iter().filter(|&ch| *ch == b'\n').count(), commands.len());

    // Trivial regexp
    let output = pc.call("show --suppress-headers u....Z?e").output().unwrap();
    assert_eq!(output.stdout.iter().filter(|&ch| *ch == b'\n').count(), 1,);

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
fn test_zsh_import_roundtrip() {
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
    assert_eq!(output.stdout.iter().filter(|&ch| *ch == b'\n').count(), 3);

    let json_output = pc.call("export").output().unwrap();
    let invocations: Vec<pxh::Invocation> =
        serde_json::from_slice(json_output.stdout.as_slice()).unwrap();
    matches_expected_history(&invocations);
}

#[test]
fn test_bash_import_roundtrip() {
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
    assert_eq!(output.stdout.iter().filter(|&ch| *ch == b'\n').count(), 3);

    let json_output = pc.call("export").output().unwrap();
    let invocations: Vec<pxh::Invocation> =
        serde_json::from_slice(json_output.stdout.as_slice()).unwrap();
    matches_expected_history(&invocations);
}

#[test]
fn test_timestamped_bash_import_roundtrip() {
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
    assert_eq!(output.stdout.iter().filter(|&ch| *ch == b'\n').count(), 3);

    let json_output = pc.call("export").output().unwrap();
    let invocations: Vec<pxh::Invocation> =
        serde_json::from_slice(json_output.stdout.as_slice()).unwrap();
    matches_expected_history(&invocations);
}

#[test]
fn test_scrub_command() {
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
    assert_eq!(output.stdout.iter().filter(|&ch| *ch == b'\n').count(), 10);

    // Scrub `test_command_10`
    let _output = pc.call("scrub test_command_10").output().unwrap();

    // Verify we have 9 rows now.
    let output = pc.call("show --suppress-headers").output().unwrap();
    assert_eq!(output.stdout.iter().filter(|&ch| *ch == b'\n').count(), 9);

    // Scrub the rest
    let _output = pc.call("scrub test_command_").output().unwrap();

    // Verify we have none.
    let output = pc.call("show --suppress-headers").output().unwrap();
    assert_eq!(output.stdout.iter().filter(|&ch| *ch == b'\n').count(), 0);
}

#[test]
fn test_atomic_line_remove() {
    let mut tmpfile = NamedTempFile::new().unwrap();
    writeln!(tmpfile, "line1").unwrap();
    writeln!(tmpfile, "line2").unwrap();
    writeln!(tmpfile, "line3").unwrap();

    let (_, path) = tmpfile.keep().unwrap();
    pxh::atomically_remove_lines_from_file(&path, "line2").unwrap();
    let fh = File::open(path).unwrap();

    let reader = BufReader::new(fh);
    reader.lines().zip(["line1", "line3"]).for_each(|(left, right)| {
        assert!(left.unwrap().as_str() == right);
    });
}
