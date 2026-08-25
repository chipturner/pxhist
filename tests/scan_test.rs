mod common;

use assert_cmd::assert::OutputAssertExt;
use common::PxhTestHelper;

/// `pxh <args>` against the helper's isolated HOME and DB; `args` is split on
/// single spaces exactly as the old per-file caller helper did.
fn call(helper: &PxhTestHelper, args: &str) -> std::process::Command {
    helper.command_with_args(&args.split(' ').collect::<Vec<_>>())
}

#[test]
fn scan_detects_aws_api_key() {
    let helper = PxhTestHelper::new();

    // Insert a command containing a fake AWS API key
    call(&helper, "insert --shellname bash --hostname h --username u --session-id 1 export AWS_KEY=AKIAIOSFODNN7EXAMPLE")
        .assert()
        .success();

    // Scan should find it
    let output = call(&helper, "scan").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AWS API Key") || stdout.contains("AWS Access Key ID Value"));
    assert!(stdout.contains("AKIAIOSFODNN7EXAMPLE"));
}

#[test]
fn scan_no_secrets_found() {
    let helper = PxhTestHelper::new();

    // Insert a command with no secrets
    call(
        &helper,
        "insert --shellname bash --hostname h --username u --session-id 1 echo hello world",
    )
    .assert()
    .success();

    // Scan should find nothing
    let output = call(&helper, "scan").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No potential secrets found"));
}

#[test]
fn scan_json_output() {
    let helper = PxhTestHelper::new();

    // Insert a command with a fake secret
    call(&helper, "insert --shellname bash --hostname h --username u --session-id 1 export AWS_KEY=AKIAIOSFODNN7EXAMPLE")
        .assert()
        .success();

    // Scan with JSON output
    let output = call(&helper, "scan --json").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should be valid JSON
    let parsed: Result<Vec<serde_json::Value>, _> = serde_json::from_str(&stdout);
    assert!(parsed.is_ok(), "Output should be valid JSON");
    let matches = parsed.unwrap();
    assert!(!matches.is_empty(), "Should find at least one match");
    assert!(matches[0].get("command").is_some());
    assert!(matches[0].get("pattern").is_some());
}

#[test]
fn scan_empty_json_output() {
    let helper = PxhTestHelper::new();

    // Insert a command with no secrets
    call(
        &helper,
        "insert --shellname bash --hostname h --username u --session-id 1 echo hello world",
    )
    .assert()
    .success();

    // Scan with JSON output
    let output = call(&helper, "scan --json").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should be an empty array
    let parsed: Result<Vec<serde_json::Value>, _> = serde_json::from_str(&stdout);
    assert!(parsed.is_ok(), "Output should be valid JSON");
    assert!(parsed.unwrap().is_empty(), "Should be empty array");
}

#[test]
fn scan_confidence_low() {
    let helper = PxhTestHelper::new();

    // Insert a command with a low confidence pattern (AWS API Gateway URL)
    call(&helper, "insert --shellname bash --hostname h --username u --session-id 1 curl https://abc123.execute-api.us-east-1.amazonaws.com/prod/endpoint")
        .assert()
        .success();

    // High confidence scan should not find it (depends on pattern categorization)
    let output = call(&helper, "scan --confidence low").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AWS API Gateway"));
}

#[test]
fn scan_confidence_all() {
    let helper = PxhTestHelper::new();

    // Insert commands with both high and low confidence patterns
    call(&helper, "insert --shellname bash --hostname h --username u --session-id 1 export AWS_KEY=AKIAIOSFODNN7EXAMPLE")
        .assert()
        .success();
    call(&helper, "insert --shellname bash --hostname h --username u --session-id 2 curl https://abc123.execute-api.us-east-1.amazonaws.com/prod/endpoint")
        .assert()
        .success();

    // Scanning with --confidence all should find both
    let output = call(&helper, "scan --confidence all").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should find at least the high confidence AWS key
    assert!(stdout.contains("potential secret"));
}

#[test]
fn scan_invalid_confidence() {
    let helper = PxhTestHelper::new();

    // Invalid confidence level is rejected by clap's ValueEnum validation
    let output = call(&helper, "scan --confidence invalid").output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid value"));
    assert!(stderr.contains("critical"));
}

#[test]
fn scan_confidence_critical() {
    let helper = PxhTestHelper::new();

    // Insert a command with a high-confidence AWS key (included in critical)
    call(&helper, "insert --shellname bash --hostname h --username u --session-id 1 export AWS_KEY=AKIAIOSFODNN7EXAMPLE")
        .assert()
        .success();

    // Insert a command with an S3 bucket URL (high confidence, but NOT in critical)
    call(&helper, "insert --shellname bash --hostname h --username u --session-id 2 aws s3 cp file.txt s3://mybucket/")
        .assert()
        .success();

    // Default scan (critical) should find the AWS key
    let output = call(&helper, "scan").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AWS") && stdout.contains("AKIA"));

    // Default scan (critical) should NOT find the S3 bucket
    assert!(!stdout.contains("s3://mybucket"));

    // High confidence scan SHOULD find the S3 bucket
    let output = call(&helper, "scan --confidence high").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("s3://mybucket") || stdout.contains("S3"));
}

#[test]
fn scan_confidence_levels_are_cumulative() {
    let helper = PxhTestHelper::new();

    // Insert a high-confidence-only pattern (S3 bucket -- not in CRITICAL_PATTERN_NAMES)
    call(
        &helper,
        "insert --shellname bash --hostname h --username u --session-id 1 aws s3 cp data.csv s3://my-secret-bucket/uploads/",
    )
    .assert()
    .success();

    // Insert a low-confidence pattern (API Gateway URL)
    call(&helper, "insert --shellname bash --hostname h --username u --session-id 2 curl https://abc123.execute-api.us-east-1.amazonaws.com/prod/endpoint")
        .assert()
        .success();

    // --confidence low should include high-confidence patterns too
    let output = call(&helper, "scan --confidence low").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("s3://my-secret-bucket"),
        "--confidence low should include high-level S3 bucket, got: {stdout}"
    );
    assert!(
        stdout.contains("execute-api"),
        "--confidence low should include low-level API gateway, got: {stdout}"
    );
}

#[test]
fn scan_verbose() {
    let helper = PxhTestHelper::new();

    call(&helper, "insert --shellname bash --hostname h --username u --session-id 1 --working-directory /test/dir export AWS_KEY=AKIAIOSFODNN7EXAMPLE")
        .assert()
        .success();

    // Verbose output should show directory
    let output = call(&helper, "scan --verbose").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Directory:"));
    assert!(stdout.contains("/test/dir"));
}

#[test]
fn scan_scrub_dry_run() {
    let helper = PxhTestHelper::new();

    // Insert a command with a secret
    call(&helper, "insert --shellname bash --hostname h --username u --session-id 1 export AWS_KEY=AKIAIOSFODNN7EXAMPLE")
        .assert()
        .success();

    // Insert a safe command
    call(
        &helper,
        "insert --shellname bash --hostname h --username u --session-id 2 echo hello world",
    )
    .assert()
    .success();

    // Dry-run should show what would be scrubbed but not remove anything
    let output = call(&helper, "scrub --scan --dry-run").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Dry-run mode"));
    assert!(stdout.contains("AKIAIOSFODNN7EXAMPLE"));

    // Verify the command still exists
    let output = call(&helper, "scan").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AKIAIOSFODNN7EXAMPLE"));
}

#[test]
fn scan_scrub_removes_secrets() {
    let helper = PxhTestHelper::new();

    // Insert a command with a secret
    call(&helper, "insert --shellname bash --hostname h --username u --session-id 1 export AWS_KEY=AKIAIOSFODNN7EXAMPLE")
        .assert()
        .success();

    // Insert a safe command
    call(
        &helper,
        "insert --shellname bash --hostname h --username u --session-id 2 echo hello world",
    )
    .assert()
    .success();

    // Scrub should remove the secret
    let output = call(&helper, "scrub --scan --yes").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Scrubbed"));
    assert!(stdout.contains("entries from database"));

    // Verify the secret is gone
    let output = call(&helper, "scan").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No potential secrets found"));

    // Verify the safe command still exists
    let output = call(&helper, "show").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("echo hello world"));
}

// Note: scan_scrub_json_conflict test removed - with the new design, scan is read-only
// and scrub doesn't have a --json flag, so this conflict no longer exists

#[test]
fn scrub_scan_no_secrets_found() {
    let helper = PxhTestHelper::new();

    // Insert only safe commands
    call(
        &helper,
        "insert --shellname bash --hostname h --username u --session-id 1 echo hello world",
    )
    .assert()
    .success();

    // Scrub --scan should succeed with no secrets to remove
    let output = call(&helper, "scrub --scan --yes").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No potential secrets found"));
}

#[test]
fn scrub_scan_with_histfile() {
    use std::{fs, io::Write};

    let helper = PxhTestHelper::new();

    // Create a bash-style histfile with some commands
    let histfile = helper.home_dir().join("test_history");
    let mut file = fs::File::create(&histfile).unwrap();
    writeln!(file, "echo hello").unwrap();
    writeln!(file, "export AWS_KEY=AKIAIOSFODNN7EXAMPLE").unwrap();
    writeln!(file, "ls -la").unwrap();
    drop(file);

    // Scan the histfile directly (not the database)
    let histfile_str = histfile.to_str().unwrap();
    let cmd = format!("scan --histfile {histfile_str} --shellname bash");
    let output = call(&helper, &cmd).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AKIAIOSFODNN7EXAMPLE"));
    assert!(stdout.contains("AWS"));

    // Scrub the histfile using scrub --scan
    let cmd = format!("scrub --scan --histfile {histfile_str} --shellname bash --yes");
    let output = call(&helper, &cmd).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Scrubbed"));
    assert!(stdout.contains(histfile_str));

    // Verify the histfile was updated
    let contents = fs::read_to_string(&histfile).unwrap();
    assert!(contents.contains("echo hello"));
    assert!(contents.contains("ls -la"));
    assert!(!contents.contains("AKIAIOSFODNN7EXAMPLE"));

    // Verify re-scanning the histfile finds nothing
    let cmd = format!("scan --histfile {histfile_str} --shellname bash");
    let output = call(&helper, &cmd).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No potential secrets found"));
}

#[test]
fn scan_zsh_histfile() {
    use std::{fs, io::Write};

    let helper = PxhTestHelper::new();

    // Create a zsh-style histfile with commands
    // Zsh format: ": timestamp:duration;command"
    let histfile = helper.home_dir().join("zsh_history");
    let mut file = fs::File::create(&histfile).unwrap();
    writeln!(file, ": 1700000000:0;echo hello").unwrap();
    writeln!(file, ": 1700000001:0;export AWS_KEY=AKIAIOSFODNN7EXAMPLE").unwrap();
    writeln!(file, ": 1700000002:0;ls -la").unwrap();
    drop(file);

    // Scan the zsh histfile
    let histfile_str = histfile.to_str().unwrap();
    let cmd = format!("scan --histfile {histfile_str} --shellname zsh");
    let output = call(&helper, &cmd).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AKIAIOSFODNN7EXAMPLE"));
    assert!(stdout.contains("AWS"));
}

#[test]
fn scrub_scan_zsh_histfile() {
    use std::{fs, io::Write};

    let helper = PxhTestHelper::new();

    // Create a zsh-style histfile with commands
    let histfile = helper.home_dir().join("zsh_history");
    let mut file = fs::File::create(&histfile).unwrap();
    writeln!(file, ": 1700000000:0;echo hello").unwrap();
    writeln!(file, ": 1700000001:0;export AWS_KEY=AKIAIOSFODNN7EXAMPLE").unwrap();
    writeln!(file, ": 1700000002:0;ls -la").unwrap();
    drop(file);

    // Scrub the zsh histfile
    let histfile_str = histfile.to_str().unwrap();
    let cmd = format!("scrub --scan --histfile {histfile_str} --shellname zsh --yes");
    let output = call(&helper, &cmd).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Scrubbed"));

    // Verify the histfile was updated - secret line should be gone
    let contents = fs::read_to_string(&histfile).unwrap();
    assert!(contents.contains("echo hello"));
    assert!(contents.contains("ls -la"));
    assert!(!contents.contains("AKIAIOSFODNN7EXAMPLE"));

    // Verify re-scanning finds nothing
    let cmd = format!("scan --histfile {histfile_str} --shellname zsh");
    let output = call(&helper, &cmd).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No potential secrets found"));
}

#[test]
fn scan_histfile_bash_noop_not_misdetected_as_zsh() {
    use std::{fs, io::Write};

    let helper = PxhTestHelper::new();

    // Create a bash histfile that starts with `: ${VAR:=x}; cmd`
    // This should NOT be misdetected as zsh
    let histfile = helper.home_dir().join("bash_noop_history");
    let mut file = fs::File::create(&histfile).unwrap();
    writeln!(file, ": ${{PATH:=/usr/bin}}; echo setup").unwrap();
    writeln!(file, "export AWS_KEY=AKIAIOSFODNN7EXAMPLE").unwrap();
    drop(file);

    // Scan without --shellname - should detect as bash, not zsh
    let histfile_str = histfile.to_str().unwrap();
    let cmd = format!("scan --histfile {histfile_str}");
    let output = call(&helper, &cmd).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // If misdetected as zsh, the zsh parser would skip `export AWS_KEY=...`
    // because it lacks the `: timestamp:duration;` prefix
    assert!(
        stdout.contains("AKIA"),
        "bash histfile with `: $VAR; cmd` should not be misdetected as zsh, got: {stdout}"
    );
}

#[test]
fn scan_histfile_auto_detect_format() {
    use std::{fs, io::Write};

    let helper = PxhTestHelper::new();

    // Create a zsh-style histfile without specifying --shellname
    let histfile = helper.home_dir().join("auto_detect_history");
    let mut file = fs::File::create(&histfile).unwrap();
    writeln!(file, ": 1700000000:0;echo hello").unwrap();
    writeln!(file, ": 1700000001:0;export AWS_KEY=AKIAIOSFODNN7EXAMPLE").unwrap();
    drop(file);

    // Scan without --shellname - should auto-detect zsh format
    let histfile_str = histfile.to_str().unwrap();
    let cmd = format!("scan --histfile {histfile_str}");
    let output = call(&helper, &cmd).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AKIAIOSFODNN7EXAMPLE"));
}

#[test]
fn scrub_interactive_histfile_requires_contraband() {
    use std::{fs, io::Write};

    let helper = PxhTestHelper::new();

    // Create a histfile
    let histfile = helper.home_dir().join("test_history");
    let mut file = fs::File::create(&histfile).unwrap();
    writeln!(file, "echo hello").unwrap();
    drop(file);

    // Interactive mode with --histfile but no contraband should fail
    let histfile_str = histfile.to_str().unwrap();
    let cmd = format!("scrub --histfile {histfile_str}");
    let output = call(&helper, &cmd).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Interactive mode with --histfile requires specifying the string to scrub")
    );
}

#[test]
fn scan_empty_histfile() {
    use std::fs;

    let helper = PxhTestHelper::new();

    // Create an empty histfile
    let histfile = helper.home_dir().join("empty_history");
    fs::File::create(&histfile).unwrap();

    let histfile_str = histfile.to_str().unwrap();
    let cmd = format!("scan --histfile {histfile_str}");
    let output = call(&helper, &cmd).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No potential secrets found"));
}

#[test]
fn scan_histfile_auto_detect_bash_timestamped() {
    use std::{fs, io::Write};

    let helper = PxhTestHelper::new();

    // Create a bash-style timestamped histfile
    let histfile = helper.home_dir().join("bash_ts_history");
    let mut file = fs::File::create(&histfile).unwrap();
    writeln!(file, "#1700000000").unwrap();
    writeln!(file, "echo hello").unwrap();
    writeln!(file, "#1700000001").unwrap();
    writeln!(file, "export AWS_KEY=AKIAIOSFODNN7EXAMPLE").unwrap();
    drop(file);

    // Scan without --shellname - should auto-detect bash format
    let histfile_str = histfile.to_str().unwrap();
    let cmd = format!("scan --histfile {histfile_str}");
    let output = call(&helper, &cmd).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AKIAIOSFODNN7EXAMPLE"));
}

#[test]
fn scrub_scan_multiple_patterns_same_command() {
    let helper = PxhTestHelper::new();

    // Insert a command that matches multiple patterns (AWS key)
    call(&helper, "insert --shellname bash --hostname h --username u --session-id 1 export AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE AWS_SECRET=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY")
        .assert()
        .success();

    // Verify it shows up in scan
    let output = call(&helper, "scan --confidence all").output().unwrap();
    assert!(output.status.success());

    // Scrub should only delete it once
    let output = call(&helper, "scrub --scan --yes").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Scrubbed 1 entries"));

    // Verify it's gone
    let output = call(&helper, "show --suppress-headers").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("AKIAIOSFODNN7EXAMPLE"));
}

#[test]
fn scan_zsh_histfile_multiline_secret() {
    use std::{fs, io::Write};

    let helper = PxhTestHelper::new();

    // Create a zsh histfile where a secret is on a continuation line
    let histfile = helper.home_dir().join("zsh_multiline_history");
    let mut file = fs::File::create(&histfile).unwrap();
    writeln!(file, ": 1700000000:0;echo hello").unwrap();
    // Multi-line curl with bearer token on continuation line
    write!(
        file,
        ": 1700000001:0;curl http://api.example.com \\\n-H \"Authorization: Bearer ghp_abc123def456ghi789jkl012mno345pqr678\"\n"
    )
    .unwrap();
    writeln!(file, ": 1700000002:0;ls -la").unwrap();
    drop(file);

    // Scan should detect the bearer token on the continuation line
    let histfile_str = histfile.to_str().unwrap();
    let cmd = format!("scan --histfile {histfile_str} --shellname zsh --confidence all");
    let output = call(&helper, &cmd).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("potential secret"),
        "scan should detect secret on continuation line, got: {stdout}"
    );
}
