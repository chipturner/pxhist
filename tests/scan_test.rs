mod common;

use common::PxhCaller;

#[test]
fn scan_detects_aws_api_key() {
    let pc = PxhCaller::new();

    // Insert a command containing a fake AWS API key
    pc.call("insert --shellname bash --hostname h --username u --session-id 1 export AWS_KEY=AKIAIOSFODNN7EXAMPLE")
        .assert()
        .success();

    // Scan should find it
    let output = pc.call("scan").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AWS API Key") || stdout.contains("AWS Access Key ID Value"));
    assert!(stdout.contains("AKIAIOSFODNN7EXAMPLE"));
}

#[test]
fn scan_no_secrets_found() {
    let pc = PxhCaller::new();

    // Insert a command with no secrets
    pc.call("insert --shellname bash --hostname h --username u --session-id 1 echo hello world")
        .assert()
        .success();

    // Scan should find nothing
    let output = pc.call("scan").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No potential secrets found"));
}

#[test]
fn scan_json_output() {
    let pc = PxhCaller::new();

    // Insert a command with a fake secret
    pc.call("insert --shellname bash --hostname h --username u --session-id 1 export AWS_KEY=AKIAIOSFODNN7EXAMPLE")
        .assert()
        .success();

    // Scan with JSON output
    let output = pc.call("scan --json").output().unwrap();
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
    let pc = PxhCaller::new();

    // Insert a command with no secrets
    pc.call("insert --shellname bash --hostname h --username u --session-id 1 echo hello world")
        .assert()
        .success();

    // Scan with JSON output
    let output = pc.call("scan --json").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should be an empty array
    let parsed: Result<Vec<serde_json::Value>, _> = serde_json::from_str(&stdout);
    assert!(parsed.is_ok(), "Output should be valid JSON");
    assert!(parsed.unwrap().is_empty(), "Should be empty array");
}

#[test]
fn scan_confidence_low() {
    let pc = PxhCaller::new();

    // Insert a command with a low confidence pattern (AWS API Gateway URL)
    pc.call("insert --shellname bash --hostname h --username u --session-id 1 curl https://abc123.execute-api.us-east-1.amazonaws.com/prod/endpoint")
        .assert()
        .success();

    // High confidence scan should not find it (depends on pattern categorization)
    let output = pc.call("scan --confidence low").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AWS API Gateway"));
}

#[test]
fn scan_confidence_all() {
    let pc = PxhCaller::new();

    // Insert commands with both high and low confidence patterns
    pc.call("insert --shellname bash --hostname h --username u --session-id 1 export AWS_KEY=AKIAIOSFODNN7EXAMPLE")
        .assert()
        .success();
    pc.call("insert --shellname bash --hostname h --username u --session-id 2 curl https://abc123.execute-api.us-east-1.amazonaws.com/prod/endpoint")
        .assert()
        .success();

    // Scanning with --confidence all should find both
    let output = pc.call("scan --confidence all").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should find at least the high confidence AWS key
    assert!(stdout.contains("potential secret"));
}

#[test]
fn scan_invalid_confidence() {
    let pc = PxhCaller::new();

    // Invalid confidence level should fail
    let output = pc.call("scan --confidence invalid").output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Invalid confidence level"));
    assert!(stderr.contains("critical"));
}

#[test]
fn scan_confidence_critical() {
    let pc = PxhCaller::new();

    // Insert a command with a high-confidence AWS key (included in critical)
    pc.call("insert --shellname bash --hostname h --username u --session-id 1 export AWS_KEY=AKIAIOSFODNN7EXAMPLE")
        .assert()
        .success();

    // Insert a command with an S3 bucket URL (high confidence, but NOT in critical)
    pc.call("insert --shellname bash --hostname h --username u --session-id 2 aws s3 cp file.txt s3://mybucket/")
        .assert()
        .success();

    // Default scan (critical) should find the AWS key
    let output = pc.call("scan").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AWS") && stdout.contains("AKIA"));

    // Default scan (critical) should NOT find the S3 bucket
    assert!(!stdout.contains("s3://mybucket"));

    // High confidence scan SHOULD find the S3 bucket
    let output = pc.call("scan --confidence high").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("s3://mybucket") || stdout.contains("S3"));
}

#[test]
fn scan_verbose() {
    let pc = PxhCaller::new();

    pc.call("insert --shellname bash --hostname h --username u --session-id 1 --working-directory /test/dir export AWS_KEY=AKIAIOSFODNN7EXAMPLE")
        .assert()
        .success();

    // Verbose output should show directory
    let output = pc.call("scan --verbose").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Directory:"));
    assert!(stdout.contains("/test/dir"));
}

#[test]
fn scan_scrub_dry_run() {
    let pc = PxhCaller::new();

    // Insert a command with a secret
    pc.call("insert --shellname bash --hostname h --username u --session-id 1 export AWS_KEY=AKIAIOSFODNN7EXAMPLE")
        .assert()
        .success();

    // Insert a safe command
    pc.call("insert --shellname bash --hostname h --username u --session-id 2 echo hello world")
        .assert()
        .success();

    // Dry-run should show what would be scrubbed but not remove anything
    let output = pc.call("scrub --scan --dry-run").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Dry-run mode"));
    assert!(stdout.contains("AKIAIOSFODNN7EXAMPLE"));

    // Verify the command still exists
    let output = pc.call("scan").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AKIAIOSFODNN7EXAMPLE"));
}

#[test]
fn scan_scrub_removes_secrets() {
    let pc = PxhCaller::new();

    // Insert a command with a secret
    pc.call("insert --shellname bash --hostname h --username u --session-id 1 export AWS_KEY=AKIAIOSFODNN7EXAMPLE")
        .assert()
        .success();

    // Insert a safe command
    pc.call("insert --shellname bash --hostname h --username u --session-id 2 echo hello world")
        .assert()
        .success();

    // Scrub should remove the secret
    let output = pc.call("scrub --scan --yes").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Scrubbed"));
    assert!(stdout.contains("entries from database"));

    // Verify the secret is gone
    let output = pc.call("scan").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No potential secrets found"));

    // Verify the safe command still exists
    let output = pc.call("show").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("echo hello world"));
}

// Note: scan_scrub_json_conflict test removed - with the new design, scan is read-only
// and scrub doesn't have a --json flag, so this conflict no longer exists

#[test]
fn scrub_scan_no_secrets_found() {
    let pc = PxhCaller::new();

    // Insert only safe commands
    pc.call("insert --shellname bash --hostname h --username u --session-id 1 echo hello world")
        .assert()
        .success();

    // Scrub --scan should succeed with no secrets to remove
    let output = pc.call("scrub --scan --yes").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No potential secrets found"));
}

#[test]
fn scrub_scan_with_histfile() {
    use std::{fs, io::Write};

    let pc = PxhCaller::new();

    // Create a bash-style histfile with some commands
    let histfile = pc.tmpdir().join("test_history");
    let mut file = fs::File::create(&histfile).unwrap();
    writeln!(file, "echo hello").unwrap();
    writeln!(file, "export AWS_KEY=AKIAIOSFODNN7EXAMPLE").unwrap();
    writeln!(file, "ls -la").unwrap();
    drop(file);

    // Scan the histfile directly (not the database)
    let histfile_str = histfile.to_str().unwrap();
    let cmd = format!("scan --histfile {histfile_str} --shellname bash");
    let output = pc.call(&cmd).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AKIAIOSFODNN7EXAMPLE"));
    assert!(stdout.contains("AWS"));

    // Scrub the histfile using scrub --scan
    let cmd = format!("scrub --scan --histfile {histfile_str} --shellname bash --yes");
    let output = pc.call(&cmd).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Scrubbed"));
    assert!(stdout.contains(&histfile_str));

    // Verify the histfile was updated
    let contents = fs::read_to_string(&histfile).unwrap();
    assert!(contents.contains("echo hello"));
    assert!(contents.contains("ls -la"));
    assert!(!contents.contains("AKIAIOSFODNN7EXAMPLE"));

    // Verify re-scanning the histfile finds nothing
    let cmd = format!("scan --histfile {histfile_str} --shellname bash");
    let output = pc.call(&cmd).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No potential secrets found"));
}

#[test]
fn scan_zsh_histfile() {
    use std::{fs, io::Write};

    let pc = PxhCaller::new();

    // Create a zsh-style histfile with commands
    // Zsh format: ": timestamp:duration;command"
    let histfile = pc.tmpdir().join("zsh_history");
    let mut file = fs::File::create(&histfile).unwrap();
    writeln!(file, ": 1700000000:0;echo hello").unwrap();
    writeln!(file, ": 1700000001:0;export AWS_KEY=AKIAIOSFODNN7EXAMPLE").unwrap();
    writeln!(file, ": 1700000002:0;ls -la").unwrap();
    drop(file);

    // Scan the zsh histfile
    let histfile_str = histfile.to_str().unwrap();
    let cmd = format!("scan --histfile {histfile_str} --shellname zsh");
    let output = pc.call(&cmd).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AKIAIOSFODNN7EXAMPLE"));
    assert!(stdout.contains("AWS"));
}

#[test]
fn scrub_scan_zsh_histfile() {
    use std::{fs, io::Write};

    let pc = PxhCaller::new();

    // Create a zsh-style histfile with commands
    let histfile = pc.tmpdir().join("zsh_history");
    let mut file = fs::File::create(&histfile).unwrap();
    writeln!(file, ": 1700000000:0;echo hello").unwrap();
    writeln!(file, ": 1700000001:0;export AWS_KEY=AKIAIOSFODNN7EXAMPLE").unwrap();
    writeln!(file, ": 1700000002:0;ls -la").unwrap();
    drop(file);

    // Scrub the zsh histfile
    let histfile_str = histfile.to_str().unwrap();
    let cmd = format!("scrub --scan --histfile {histfile_str} --shellname zsh --yes");
    let output = pc.call(&cmd).output().unwrap();
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
    let output = pc.call(&cmd).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No potential secrets found"));
}

#[test]
fn scan_histfile_auto_detect_format() {
    use std::{fs, io::Write};

    let pc = PxhCaller::new();

    // Create a zsh-style histfile without specifying --shellname
    let histfile = pc.tmpdir().join("auto_detect_history");
    let mut file = fs::File::create(&histfile).unwrap();
    writeln!(file, ": 1700000000:0;echo hello").unwrap();
    writeln!(file, ": 1700000001:0;export AWS_KEY=AKIAIOSFODNN7EXAMPLE").unwrap();
    drop(file);

    // Scan without --shellname - should auto-detect zsh format
    let histfile_str = histfile.to_str().unwrap();
    let cmd = format!("scan --histfile {histfile_str}");
    let output = pc.call(&cmd).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AKIAIOSFODNN7EXAMPLE"));
}

#[test]
fn scrub_interactive_histfile_requires_contraband() {
    use std::{fs, io::Write};

    let pc = PxhCaller::new();

    // Create a histfile
    let histfile = pc.tmpdir().join("test_history");
    let mut file = fs::File::create(&histfile).unwrap();
    writeln!(file, "echo hello").unwrap();
    drop(file);

    // Interactive mode with --histfile but no contraband should fail
    let histfile_str = histfile.to_str().unwrap();
    let cmd = format!("scrub --histfile {histfile_str}");
    let output = pc.call(&cmd).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Interactive mode with --histfile requires specifying the string to scrub")
    );
}

#[test]
fn scan_empty_histfile() {
    use std::fs;

    let pc = PxhCaller::new();

    // Create an empty histfile
    let histfile = pc.tmpdir().join("empty_history");
    fs::File::create(&histfile).unwrap();

    let histfile_str = histfile.to_str().unwrap();
    let cmd = format!("scan --histfile {histfile_str}");
    let output = pc.call(&cmd).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No potential secrets found"));
}

#[test]
fn scan_histfile_auto_detect_bash_timestamped() {
    use std::{fs, io::Write};

    let pc = PxhCaller::new();

    // Create a bash-style timestamped histfile
    let histfile = pc.tmpdir().join("bash_ts_history");
    let mut file = fs::File::create(&histfile).unwrap();
    writeln!(file, "#1700000000").unwrap();
    writeln!(file, "echo hello").unwrap();
    writeln!(file, "#1700000001").unwrap();
    writeln!(file, "export AWS_KEY=AKIAIOSFODNN7EXAMPLE").unwrap();
    drop(file);

    // Scan without --shellname - should auto-detect bash format
    let histfile_str = histfile.to_str().unwrap();
    let cmd = format!("scan --histfile {histfile_str}");
    let output = pc.call(&cmd).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("AKIAIOSFODNN7EXAMPLE"));
}

#[test]
fn scrub_scan_multiple_patterns_same_command() {
    let pc = PxhCaller::new();

    // Insert a command that matches multiple patterns (AWS key)
    pc.call("insert --shellname bash --hostname h --username u --session-id 1 export AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE AWS_SECRET=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY")
        .assert()
        .success();

    // Verify it shows up in scan
    let output = pc.call("scan --confidence all").output().unwrap();
    assert!(output.status.success());

    // Scrub should only delete it once
    let output = pc.call("scrub --scan --yes").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Scrubbed 1 entries"));

    // Verify it's gone
    let output = pc.call("show --suppress-headers").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("AKIAIOSFODNN7EXAMPLE"));
}
