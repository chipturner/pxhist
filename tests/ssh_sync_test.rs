//! SSH-mode sync argument handling. No real `ssh` is invoked: a stub script
//! passed via `--ssh-cmd` records what pxh asked for and fails like a
//! resolution error would, so the tests need neither network nor DNS.

use std::{fs, os::unix::fs::PermissionsExt, path::Path};

use pxh::test_utils::PxhTestHelper;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn failing(helper: &PxhTestHelper, args: &[&str]) -> String {
    let output = helper.command_with_args(args).output().unwrap();
    assert!(!output.status.success(), "expected failure for {args:?}");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Install a fake `ssh` that logs its argv and exits 255 with an OpenSSH-style
/// resolution error. Returns (script path, argv log path).
fn stub_ssh(helper: &PxhTestHelper) -> Result<(String, std::path::PathBuf)> {
    let log = helper.home_dir().join("ssh-argv.log");
    let script = helper.home_dir().join("fake-ssh");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n\
             echo \"ssh: Could not resolve hostname $1: Name or service not known\" >&2\n\
             exit 255\n",
            log.display()
        ),
    )?;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755))?;
    Ok((script.display().to_string(), log))
}

fn argv_lines(log: &Path) -> Vec<String> {
    fs::read_to_string(log).unwrap().lines().map(str::to_owned).collect()
}

#[test]
fn test_ssh_sync_command_help() {
    let helper = PxhTestHelper::new();
    let output = helper.command_with_args(&["sync", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for flag in
        ["--remote", "--send-only", "--receive-only", "--remote-db", "--remote-pxh", "--ssh-cmd"]
    {
        assert!(stdout.contains(flag), "help should list {flag}");
    }
}

/// The commonest first-run failure: pxh is not installed on the remote. ssh
/// relays the shell's error and exits 127 without ever reading stdin, so the
/// client's database write hits a closed pipe.
#[test]
fn test_remote_without_pxh_fails_with_its_exit_status_not_sigpipe() -> Result<()> {
    for mode in [None, Some("--send-only"), Some("--receive-only")] {
        let helper = PxhTestHelper::new();
        // Enough history that the snapshot cannot fit in the pipe buffer.
        helper.command_with_args(&["stats"]).output()?; // creates the db
        {
            let conn = rusqlite::Connection::open(helper.db_path())?;
            let tx = conn.unchecked_transaction()?;
            for i in 0..4000 {
                tx.execute(
                    "INSERT INTO command_history (session_id, full_command, shellname,
                                                  start_unix_timestamp)
                     VALUES (1, CAST(? AS blob), 'bash', ?)",
                    rusqlite::params![format!("echo {i} {}", "x".repeat(1000)), 1_700_000_000 + i],
                )?;
            }
            tx.commit()?;
        }
        let script = helper.home_dir().join("fake-ssh");
        fs::write(&script, "#!/bin/sh\necho 'sh: pxh: command not found' >&2\nexit 127\n")?;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755))?;
        let script = script.display().to_string();

        let mut args = vec!["sync", "--ssh-cmd", &script, "--remote", "somehost"];
        args.extend(mode);
        let output = helper.command_with_args(&args).output()?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            output.status.code(),
            Some(1),
            "{mode:?}: must exit normally, not die of SIGPIPE: {:?}\n{stderr}",
            output.status
        );
        assert!(stderr.contains("Remote sync failed"), "{mode:?}: {stderr}");
        assert!(
            stderr.contains("127"),
            "{mode:?}: remote exit status should be reported: {stderr}"
        );
        assert!(
            stderr.contains("pxh bootstrap somehost"),
            "{mode:?}: exit 127 should point at bootstrap: {stderr}"
        );
    }
    Ok(())
}

#[test]
fn test_ssh_failure_is_surfaced_for_each_mode() -> Result<()> {
    for mode in [None, Some("--send-only"), Some("--receive-only")] {
        let helper = PxhTestHelper::new();
        let (ssh, _) = stub_ssh(&helper)?;
        let mut args = vec!["sync", "--ssh-cmd", &ssh, "--remote", "nonexistent-host"];
        args.extend(mode);
        let stderr = failing(&helper, &args);
        assert!(stderr.contains("Could not resolve hostname"), "{mode:?}: {stderr}");
    }
    Ok(())
}

#[test]
fn test_ssh_invocation_targets_host_and_remote_server_mode() -> Result<()> {
    let helper = PxhTestHelper::new();
    let (ssh, log) = stub_ssh(&helper)?;
    failing(&helper, &["sync", "--ssh-cmd", &ssh, "--remote", "example-host", "--since", "7"]);

    let argv = argv_lines(&log);
    assert_eq!(argv[0], "example-host", "first ssh arg is the host: {argv:?}");
    let remote = argv.last().unwrap();
    assert!(remote.contains("sync --server --since 7"), "remote command: {remote}");
    assert!(remote.contains("--db"), "remote command should pass a db path: {remote}");
    Ok(())
}

#[test]
fn test_ssh_cmd_options_and_explicit_remote_paths_are_passed_through() -> Result<()> {
    let helper = PxhTestHelper::new();
    let (ssh, log) = stub_ssh(&helper)?;
    let ssh_cmd = format!("{ssh} -p 2222 -o 'StrictHostKeyChecking no'");
    failing(
        &helper,
        &[
            "sync",
            "--ssh-cmd",
            &ssh_cmd,
            "--remote",
            "example-host",
            "--remote-pxh",
            "/opt/bin/pxh",
            "--remote-db",
            "/srv/pxh.db",
        ],
    );

    let argv = argv_lines(&log);
    assert_eq!(argv[..5], ["-p", "2222", "-o", "StrictHostKeyChecking no", "example-host"]);
    assert_eq!(argv[5], "/opt/bin/pxh --db /srv/pxh.db sync --server", "{argv:?}");
    Ok(())
}

#[test]
fn test_directory_sync() {
    let helper = PxhTestHelper::new();
    let sync_dir = helper.home_dir().join("sync");
    let output = helper.command_with_args(&["sync", sync_dir.to_str().unwrap()]).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn test_sync_without_path_or_remote() {
    let helper = PxhTestHelper::new();
    let stderr = failing(&helper, &["sync"]);
    assert!(stderr.contains("Directory path is required for directory-based sync"), "{stderr}");
}

#[test]
fn test_send_or_receive_only_require_remote() {
    for flag in ["--send-only", "--receive-only"] {
        let helper = PxhTestHelper::new();
        let stderr = failing(&helper, &["sync", flag]);
        assert!(
            stderr.contains(
                "--send-only and --receive-only flags require --remote or --stdin-stdout"
            ),
            "{flag}: {stderr}"
        );
    }
}

#[test]
fn test_remote_with_directory() {
    let helper = PxhTestHelper::new();
    let sync_dir = helper.home_dir().join("sync");
    let stderr = failing(&helper, &["sync", "--remote", "localhost", sync_dir.to_str().unwrap()]);
    assert!(stderr.contains("Cannot specify both --remote and a directory path"), "{stderr}");
}
