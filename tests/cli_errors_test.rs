//! Errors reach the user as `error: <message>` on stderr -- never as Rust
//! Debug output (`Error: Os { code: 13, ... }`) and never quoted.

mod common;

use common::PxhTestHelper;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn unopenable_database_is_reported_as_a_plain_error_line() -> Result<()> {
    let helper = PxhTestHelper::new();
    // A directory is never a valid SQLite file, on every platform.
    let dir = helper.home_dir().join("not-a-db");
    std::fs::create_dir_all(&dir)?;

    let output = helper.command_with_args(&["--db", dir.to_str().unwrap(), "stats"]).output()?;
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert!(stderr.starts_with("error: "), "stderr was: {stderr:?}");
    assert!(!stderr.contains("Error:"), "Debug-style prefix leaked: {stderr:?}");
    assert!(!stderr.contains("Os {"), "Debug struct leaked: {stderr:?}");
    assert!(stderr.contains(dir.to_str().unwrap()), "error should name the path: {stderr:?}");
    // The whole chain, not just the outermost context: a user who is told only
    // "open history database <path>" has not been told what went wrong.
    assert!(
        stderr.contains("open history database") && stderr.contains("unable to open database"),
        "cause chain was truncated: {stderr:?}"
    );
    Ok(())
}

#[test]
fn string_errors_are_not_quoted() -> Result<()> {
    let helper = PxhTestHelper::new();
    // `--since` without `--remote` is rejected by SyncCommand with a plain string
    // error (the directory argument is positional and is never reached).
    let output = helper.command_with_args(&["sync", "--since", "1", "/nonexistent"]).output()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.starts_with("error: "), "{stderr:?}");
    assert!(!stderr.starts_with("error: \""), "string error was Debug-quoted: {stderr:?}");
    Ok(())
}

#[test]
fn warnings_use_the_lowercase_word() -> Result<()> {
    let helper = PxhTestHelper::new();
    // A directory of "databases" containing a non-SQLite file fails to open
    // as a database, which directory sync reports as a "skipping ..." warning.
    let dir = helper.home_dir().join("syncdir");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("junk.db"), b"not sqlite")?;
    let output = helper.command_with_args(&["sync", dir.to_str().unwrap()]).output()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("warning: "), "{stderr:?}");
    assert!(!stderr.contains("Warning:"), "{stderr:?}");
    Ok(())
}

/// The shell hooks run `pxh insert` on every prompt with stderr attached to
/// the terminal, so a config pxh refuses must not turn every prompt into a
/// warning. The hook path uses the config it can parse and says nothing.
#[test]
fn insert_stays_quiet_when_the_config_cannot_be_parsed() -> Result<()> {
    let helper = PxhTestHelper::new();
    std::fs::write(
        helper.home_dir().join(".pxh").join("config.toml"),
        "[recall]\nkeymapp = \"vim\"\n",
    )?;

    let output = helper
        .command_with_args(&[
            "insert",
            "--shellname",
            "bash",
            "--hostname",
            "h",
            "--username",
            "u",
            "--session-id",
            "1",
            "echo hi",
        ])
        .output()?;
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success(), "insert failed: {stderr:?}");
    assert!(stderr.is_empty(), "the hook path must stay silent: {stderr:?}");
    Ok(())
}

/// A command a human typed still gets told, once, on one line: the toml
/// snippet belongs in `pxh doctor`, not in the middle of a session.
#[test]
fn a_typed_command_warns_once_about_an_unparseable_config() -> Result<()> {
    let helper = PxhTestHelper::new();
    std::fs::write(
        helper.home_dir().join(".pxh").join("config.toml"),
        "[recall]\nkeymapp = \"vim\"\n",
    )?;

    let output = helper.command_with_args(&["shell-config", "bash"]).output()?;
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success(), "shell-config failed: {stderr:?}");
    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(lines.len(), 1, "one warning, not a toml snippet: {stderr:?}");
    assert!(lines[0].starts_with("warning: failed to parse"), "{stderr:?}");
    assert!(lines[0].contains("keymapp"), "{stderr:?}");
    Ok(())
}
