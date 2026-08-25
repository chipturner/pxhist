//! `pxh doctor` diagnostics and `--fix` repairs.
//!
//! Every invocation runs under `PxhTestHelper`, i.e. an isolated HOME. That
//! matters more here than anywhere else: `doctor --fix --yes` applies every
//! outstanding repair it finds, and against the developer's real home that
//! means merging and moving `~/.pxh`, or installing hooks into `~/.bashrc`.

use std::{fs, path::Path};

use pxh::test_utils::PxhTestHelper;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const BASH: &str = "/bin/bash";

/// Insert one command at `start_ts` into `db` (defaults to the helper's DB).
fn insert(helper: &PxhTestHelper, db: Option<&Path>, command: &str, start_ts: u64) -> Result<()> {
    let db = db.unwrap_or_else(|| helper.db_path());
    let output = helper
        .command_with_args(&[
            "--db",
            db.to_str().unwrap(),
            "insert",
            "--hostname",
            "test",
            "--shellname",
            "bash",
            "--username",
            "test",
            "--session-id",
            "1",
            "--start-unix-timestamp",
            &start_ts.to_string(),
            command,
        ])
        .output()?;
    assert!(output.status.success(), "insert failed: {}", String::from_utf8_lossy(&output.stderr));
    Ok(())
}

/// A helper whose DB already holds one 1970-era command.
fn seeded() -> Result<PxhTestHelper> {
    let helper = PxhTestHelper::new();
    insert(&helper, None, "echo hello", 1_000_000)?;
    Ok(helper)
}

/// Run `pxh doctor <args>` with `$SHELL` pinned; returns (stdout+stderr, success).
fn doctor(helper: &PxhTestHelper, shell: &str, args: &[&str]) -> Result<(String, bool)> {
    let mut cmd = helper.command_with_args(&["doctor"]);
    cmd.args(args).env("SHELL", shell);
    let output = cmd.output()?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr);
    Ok((format!("{stdout}{stderr}"), output.status.success()))
}

fn mode_of(path: &Path) -> Result<u32> {
    use std::os::unix::fs::PermissionsExt;
    Ok(fs::metadata(path)?.permissions().mode() & 0o777)
}

#[test]
fn doctor_runs_on_fresh_db() -> Result<()> {
    let helper = seeded()?;
    let (out, _) = doctor(&helper, BASH, &["--verbose"])?;
    assert!(out.contains("pxh "), "should show version: {out}");
    assert!(out.contains("Schema version"), "should check schema: {out}");
    assert!(out.contains("SQLite"), "should show SQLite version: {out}");
    Ok(())
}

#[test]
fn doctor_report_produces_markdown() -> Result<()> {
    let helper = seeded()?;
    let (out, _) = doctor(&helper, BASH, &["--report"])?;
    for needle in ["<details>", "pxh version:", "</details>", "SQLite:", "Schema version:"] {
        assert!(out.contains(needle), "report should contain {needle:?}: {out}");
    }
    Ok(())
}

/// Doctor must see the database as it is on disk. Opening it through the
/// normal connection path would silently chmod and migrate it first, leaving
/// nothing to report and nothing for `--fix` to do.
#[test]
fn doctor_reports_and_fixes_loose_permissions() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let helper = seeded()?;
    fs::set_permissions(helper.db_path(), fs::Permissions::from_mode(0o644))?;

    let (out, _) = doctor(&helper, BASH, &[])?;
    assert!(out.contains("Permissions 0644 (should be 0600)"), "{out}");
    assert_eq!(mode_of(helper.db_path())?, 0o644, "a diagnostic run must not change the file");

    let (out, ok) = doctor(&helper, BASH, &["--fix", "--yes"])?;
    assert!(ok, "{out}");
    assert!(out.contains("Fixing: Fix database permissions to 0600"), "{out}");
    assert_eq!(mode_of(helper.db_path())?, 0o600);
    Ok(())
}

#[test]
fn doctor_reports_and_fixes_outdated_schema() -> Result<()> {
    let helper = seeded()?;
    {
        let conn = rusqlite::Connection::open(helper.db_path())?;
        conn.pragma_update(None, "user_version", 1)?;
    }

    let (out, _) = doctor(&helper, BASH, &[])?;
    assert!(out.contains("Schema version 1 (expected"), "{out}");

    let (out, ok) = doctor(&helper, BASH, &["--fix", "--yes"])?;
    assert!(ok, "{out}");
    assert!(out.contains("Fixing: Run schema migrations"), "{out}");
    let version: i32 = rusqlite::Connection::open(helper.db_path())?.pragma_query_value(
        None,
        "user_version",
        |r| r.get(0),
    )?;
    assert_eq!(version, pxh::CURRENT_SCHEMA_VERSION);
    Ok(())
}

#[test]
fn doctor_default_hides_passing_checks() -> Result<()> {
    let helper = seeded()?;
    let (out, _) = doctor(&helper, BASH, &[])?;
    assert!(!out.contains("  ok  Schema version"), "default mode hides passing checks: {out}");
    Ok(())
}

#[test]
fn doctor_verbose_shows_passing_checks() -> Result<()> {
    let helper = seeded()?;
    let (out, _) = doctor(&helper, BASH, &["--verbose"])?;
    assert!(out.contains("  ok  Schema version"), "--verbose shows passing checks: {out}");
    Ok(())
}

#[test]
fn doctor_fix_merges_legacy_db() -> Result<()> {
    // The live DB is at the XDG path; a legacy ~/.pxh/pxh.db also exists.
    let helper = PxhTestHelper::new().with_custom_db_path(".local/share/pxh/pxh.db");
    let home = helper.home_dir();
    let legacy_dir = home.join(".pxh");
    let legacy_db = legacy_dir.join("pxh.db");
    insert(&helper, Some(&legacy_db), "legacy_command_123", 1_000_000)?;
    insert(&helper, None, "xdg_command_456", 1_000_000)?;

    let (out, ok) = doctor(&helper, BASH, &["--fix", "--yes"])?;
    assert!(ok, "doctor --fix should succeed: {out}");
    assert!(out.contains("Merged"), "should report merged commands: {out}");

    let export = helper.command_with_args(&["export"]).output()?;
    let export = String::from_utf8_lossy(&export.stdout);
    assert!(export.contains("legacy_command_123"), "XDG db should contain the legacy command");
    assert!(export.contains("xdg_command_456"), "XDG db should keep its own command");

    assert!(!legacy_dir.exists(), "legacy dir should be moved");
    assert!(home.join(".pxh.backup").exists(), "backup should exist");
    Ok(())
}

// -- Broken-state diagnostics ------------------------------------------------

#[test]
fn doctor_warns_when_no_commands_recorded() -> Result<()> {
    let helper = PxhTestHelper::new();
    helper.command_with_args(&["stats"]).output()?; // creates an empty db
    let (out, _) = doctor(&helper, BASH, &["--verbose"])?;
    assert!(out.contains("No commands recorded yet"), "{out}");
    Ok(())
}

#[test]
fn doctor_warns_when_last_command_is_stale() -> Result<()> {
    let helper = seeded()?; // timestamp 1000000 -- 1970
    let (out, _) = doctor(&helper, BASH, &["--verbose"])?;
    assert!(out.contains("shell hooks may not be active"), "{out}");
    assert!(out.contains("days ago"), "{out}");
    Ok(())
}

#[test]
fn doctor_reports_hooks_active_for_recent_command() -> Result<()> {
    let helper = PxhTestHelper::new();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
    insert(&helper, None, "echo recent", now)?;
    let (out, _) = doctor(&helper, BASH, &["--verbose"])?;
    assert!(out.contains("Shell hooks active"), "{out}");
    Ok(())
}

#[test]
fn doctor_warns_when_rc_file_lacks_hooks_and_fix_installs_them() -> Result<()> {
    let helper = seeded()?;
    let zshrc = helper.home_dir().join(".zshrc");
    fs::write(&zshrc, "# nothing here\n")?;

    let (out, _) = doctor(&helper, "/usr/bin/zsh", &["--verbose"])?;
    assert!(out.contains("~/.zshrc does not contain pxh shell-config"), "{out}");
    assert!(out.contains("Run: pxh install zsh"), "{out}");

    let (out, ok) = doctor(&helper, "/usr/bin/zsh", &["--fix", "--yes"])?;
    assert!(ok, "{out}");
    assert!(out.contains("Install pxh shell hooks into ~/.zshrc"), "{out}");
    let contents = fs::read_to_string(&zshrc)?;
    assert!(contents.contains("pxh shell-config zsh"), "fix should install hooks: {contents}");
    assert!(contents.contains("# nothing here"), "fix must preserve existing rc content");
    Ok(())
}

#[test]
fn doctor_warns_when_rc_file_missing() -> Result<()> {
    let helper = seeded()?;
    let (out, _) = doctor(&helper, BASH, &["--verbose"])?;
    assert!(out.contains("~/.bashrc not found"), "{out}");
    Ok(())
}

#[test]
fn doctor_reports_invalid_config_toml_as_failure() -> Result<()> {
    let helper = seeded()?;
    // The helper seeds ~/.pxh/config.toml; corrupt that one so only a
    // single config file is in play.
    fs::write(helper.home_dir().join(".pxh/config.toml"), "[recall\nkeymap = \n")?;
    let (out, _) = doctor(&helper, BASH, &["--verbose"])?;
    assert!(out.contains("invalid"), "{out}");
    Ok(())
}

#[test]
fn doctor_fix_generates_machine_id_in_config() -> Result<()> {
    let helper = seeded()?;
    let (out, _) = doctor(&helper, BASH, &["--verbose"])?;
    assert!(out.contains("No machine_id in config"), "{out}");

    let (out, ok) = doctor(&helper, BASH, &["--fix", "--yes"])?;
    assert!(ok, "{out}");
    // An existing ~/.pxh takes precedence over the XDG config dir.
    let config = fs::read_to_string(helper.home_dir().join(".pxh/config.toml"))?;
    assert!(config.contains("machine_id"), "fix should persist a machine_id: {config}");
    let (out, _) = doctor(&helper, BASH, &["--verbose"])?;
    assert!(out.contains("machine_id present"), "{out}");
    Ok(())
}

#[test]
fn doctor_warns_when_legacy_and_xdg_configs_both_exist() -> Result<()> {
    let helper = seeded()?;
    let xdg = helper.home_dir().join(".config/pxh");
    fs::create_dir_all(&xdg)?;
    fs::write(xdg.join("config.toml"), "[host]\nhostname = \"x\"\n")?;
    let (out, _) = doctor(&helper, BASH, &["--verbose"])?;
    assert!(out.contains("Both legacy ~/.pxh/config.toml and XDG config exist"), "{out}");
    Ok(())
}

#[test]
fn doctor_fails_cleanly_on_unreadable_database() -> Result<()> {
    let helper = PxhTestHelper::new();
    fs::write(helper.db_path(), "this is not a sqlite database at all")?;
    let (out, ok) = doctor(&helper, BASH, &["--verbose"])?;
    assert!(!ok, "a database that cannot be opened is a failure: {out}");
    assert!(out.contains("Could not open database"), "{out}");
    Ok(())
}

#[test]
fn config_with_unknown_key_fails_doctor() -> Result<()> {
    let helper = seeded()?;
    let config_path = helper.home_dir().join(".pxh").join("config.toml");
    fs::write(&config_path, "[recall]\nkeymapp = \"vim\"\n")?;
    let (out, ok) = doctor(&helper, BASH, &[])?;
    assert!(!ok, "doctor should fail on a config pxh would refuse:\n{out}");
    assert!(out.contains("unknown field `keymapp`"), "{out}");
    Ok(())
}

#[test]
fn json_output_is_machine_readable() -> Result<()> {
    let helper = seeded()?;
    let (out, _) = doctor(&helper, BASH, &["--json"])?;
    let v: serde_json::Value = serde_json::from_str(&out).map_err(|e| format!("{e}: {out}"))?;
    let sections = v["sections"].as_array().ok_or("sections missing")?;
    assert!(sections.iter().any(|s| s["name"] == "Config"), "{v}");
    let check = &sections[0]["checks"][0];
    for key in ["label", "status", "message", "fix"] {
        assert!(check.get(key).is_some(), "check lacks {key}: {check}");
    }
    Ok(())
}
