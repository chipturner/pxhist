use std::path::PathBuf;

use assert_cmd::Command;
use tempfile::TempDir;

// Simple struct and helpers for invoking pxh with a given testdb.
struct PxhCaller {
    tmpdir: TempDir,
}

impl PxhCaller {
    fn new() -> Self {
        PxhCaller {
            tmpdir: TempDir::new().unwrap(),
        }
    }

    fn call<S: AsRef<str>>(&mut self, args: S) -> Command {
        let mut ret = Command::cargo_bin("pxh").unwrap();
        ret.env_clear()
            .env("PXH_DB_PATH", &self.tmpdir.path().join("test"));
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
        .assert()
        .success();

    let mut pc = PxhCaller::new();
    pc.call("insert --shellname zsh --hostname testhost --username testuser --session-id 12345678 test_command")
        .assert()
        .success();

    pc.call("show --output-format json").assert().success();
}

// Basic round trip test of inserting/sealing, then verify with json export.
#[test]
fn test_insert_seal_roundtrip() {
    let mut pc = PxhCaller::new();
    let commands = vec!["df", "sleep 1", "uptime"];
    for command in &commands {
        pc.call(format!(
	    "insert --shellname zsh --hostname testhost --username testuser --session-id 12345678 --start-unix-timestamp 1653573011 {}",
	    command
	))
	    .assert()
	    .success();

        pc.call("seal --session-id 12345678 --exit-status 0 --end-unix-timestamp 1653573011")
            .assert()
            .success();
    }

    let output = pc.call("show").output().unwrap();

    assert!(output.stdout.len() > 0);
    assert_eq!(
        output.stdout.iter().filter(|&ch| *ch == b'\n').count(),
        commands.len() + 1
    );

    let json_output = pc.call("show --output-format json").output().unwrap();
    let invocations: Vec<pxh::Invocation> =
        serde_json::from_slice(json_output.stdout.as_slice()).unwrap();
    assert_eq!(invocations.len(), commands.len());
    for (idx, val) in invocations.iter().enumerate() {
        assert_eq!(val.command.to_string_lossy(), commands[idx]);
    }
}

// Verify a given invocation list matches what we expect.  The data is
// a bit of a torture test of non-utf8 data, spaces, etc.
fn matches_expected_history(invocations: &[pxh::Invocation]) {
    let expected = vec![
        pxh::BinaryStringHelper::Readable(r#"echo $'this "is" \'a\' \\n test\n\nboo'"#.to_string()),
        pxh::BinaryStringHelper::Readable("fd zsh".to_string()),
        pxh::BinaryStringHelper::Encoded(
            [
                101, 99, 104, 111, 32, 0xf0, 0xce, 0xb1, 0xce, 0xa5, 0xef, 0xbd, 0xa9,
            ]
            .to_vec(),
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

    let output = pc.call("show").output().unwrap();

    assert!(output.stdout.len() > 0);
    assert_eq!(output.stdout.iter().filter(|&ch| *ch == b'\n').count(), 4);

    let json_output = pc.call("show --output-format json").output().unwrap();
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

    let output = pc.call("show").output().unwrap();

    assert!(output.stdout.len() > 0);
    assert_eq!(output.stdout.iter().filter(|&ch| *ch == b'\n').count(), 4);

    let json_output = pc.call("show --output-format json").output().unwrap();
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
        resources
            .join("timestamped_bash_histfile")
            .to_string_lossy()
    ))
    .assert()
    .success();

    let output = pc.call("show").output().unwrap();

    assert!(output.stdout.len() > 0);
    assert_eq!(output.stdout.iter().filter(|&ch| *ch == b'\n').count(), 4);

    let json_output = pc.call("show --output-format json").output().unwrap();
    let invocations: Vec<pxh::Invocation> =
        serde_json::from_slice(json_output.stdout.as_slice()).unwrap();
    matches_expected_history(&invocations);
}
