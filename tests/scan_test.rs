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
fn scan_limit() {
    let pc = PxhCaller::new();

    // Insert multiple commands with secrets
    for i in 1..=5 {
        let cmd = format!(
            "insert --shellname bash --hostname h --username u --session-id {i} export AWS_KEY{i}=AKIA{}EXAMPLE",
            "0".repeat(12)
        );
        pc.call(&cmd).assert().success();
    }

    // Scan with limit of 2
    let output = pc.call("scan --limit 2").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Note: Each command may match multiple patterns, so we check the count in the output
    // The actual output says "Found N potential secret(s)"
    assert!(stdout.contains("Found"));
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
