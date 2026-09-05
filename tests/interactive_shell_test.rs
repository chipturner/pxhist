//! Drive real interactive bash and zsh sessions over a PTY and verify that
//! the pxh hooks record what the user typed.
//!
//! Synchronisation is by prompt, never by sleeping: each rc file sets a
//! sentinel prompt, and `ShellSession::run` waits for it to reappear. Because
//! the hooks run synchronously inside preexec/precmd, the prompt coming back
//! proves the insert and seal for the previous command have both completed.

use std::{env, fs, os::fd::AsRawFd, os::unix::fs::PermissionsExt, path::Path};

use pxh::{Invocation, test_utils::PxhTestHelper};
use rexpect::session::{PtySession, spawn_command};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const PROMPT: &str = "PXHT> ";

#[derive(Clone, Copy, Debug)]
enum Shell {
    Bash,
    Zsh,
}

impl Shell {
    fn name(self) -> &'static str {
        match self {
            Shell::Bash => "bash",
            Shell::Zsh => "zsh",
        }
    }

    fn rc_file(self) -> &'static str {
        match self {
            Shell::Bash => ".bashrc",
            Shell::Zsh => ".zshrc",
        }
    }

    /// The Ctrl-R widget needs `READLINE_LINE` (bash >= 4.0). Stock macOS
    /// ships bash 3.2, where the binding is deliberately not installed, so
    /// the round-trip tests cannot pass there; fail up front with a pointer
    /// instead of timing out waiting for the selected command to run.
    /// Checks `exe` itself, which must be the binary the pty will run.
    fn assert_supported_version(self, exe: &Path) {
        if !matches!(self, Shell::Bash) {
            return;
        }
        let out = std::process::Command::new(exe)
            .args(["-c", "echo ${BASH_VERSINFO[0]}"])
            .output()
            .expect("run bash");
        let major: u32 = String::from_utf8_lossy(&out.stdout).trim().parse().unwrap_or(0);
        assert!(
            major >= 4,
            "bash >= 4 is required for interactive tests (found major version {major}); \
             on macOS: brew install bash"
        );
    }

    /// rc prelude that pins the prompt so tests can synchronise on it.
    ///
    /// rexpect forks the pty with echo off, and GNU readline skips redisplay
    /// entirely on a no-echo terminal -- which would hide the line it
    /// rebuilds after a recall edit. `stty echo` here runs before readline's
    /// first terminal prep, so the setting is seen deterministically.
    fn rc_prelude(self) -> String {
        match self {
            Shell::Bash => format!("stty echo\nPS1='{PROMPT}'\n"),
            Shell::Zsh => {
                format!("stty echo\nPROMPT='{PROMPT}'\nunsetopt zle_bracketed_paste\n")
            }
        }
    }
}

struct ShellSession {
    pty: PtySession,
}

impl ShellSession {
    /// Install pxh into a fresh rc file, spawn the shell, and wait for the
    /// first prompt. Panics (rather than silently passing) if the shell is
    /// missing -- CI images are expected to provide both.
    ///
    /// The shell is resolved on *this* process's PATH and spawned by absolute
    /// path: `shell_command` rebuilds the child's PATH from `getconf PATH`,
    /// which would otherwise pick stock `/bin/bash` 3.2 on macOS over the
    /// Homebrew bash the version check just approved.
    fn spawn(helper: &PxhTestHelper, shell: Shell) -> Result<Self> {
        let exe = which::which(shell.name()).unwrap_or_else(|_| {
            panic!("{} is required for interactive tests but was not found in PATH", shell.name())
        });
        shell.assert_supported_version(&exe);
        fs::write(helper.home_dir().join(shell.rc_file()), shell.rc_prelude())?;
        let install = helper.command_with_args(&["install", shell.name()]).output()?;
        assert!(
            install.status.success(),
            "install {} failed: {}",
            shell.name(),
            String::from_utf8_lossy(&install.stderr)
        );

        let pty = spawn_command(helper.shell_command(&exe), Some(30_000))?;
        // rexpect ptys start 0x0; give the shell (and any TUI it spawns) a
        // real size.
        let master = pty.process().get_file_handle()?;
        let ws = libc::winsize { ws_row: 24, ws_col: 80, ws_xpixel: 0, ws_ypixel: 0 };
        // SAFETY: valid fd and pointer for the duration of the call.
        unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &ws) };

        let mut session = ShellSession { pty };
        session.wait_prompt()?;
        Ok(session)
    }

    /// Block until the sentinel prompt is printed; returns everything
    /// emitted before it.
    fn wait_prompt(&mut self) -> Result<String> {
        Ok(self.pty.exp_string(PROMPT)?)
    }

    /// Type a command, wait for the next prompt, and return the transcript
    /// (echoed command plus output).
    fn run(&mut self, cmd: &str) -> Result<String> {
        self.pty.send_line(cmd)?;
        self.wait_prompt()
    }

    /// Wait for `marker` (something only *executing* a command can print,
    /// never its echo or a TUI rendering of it), then for the next prompt.
    fn expect_output(&mut self, marker: &str) -> Result<()> {
        self.pty.exp_string(marker)?;
        self.wait_prompt()?;
        Ok(())
    }

    fn exit(mut self) -> Result<()> {
        self.pty.send_line("exit")?;
        self.pty.exp_eof()?;
        Ok(())
    }
}

/// The version guard and the pty spawn must agree on *which* bash they
/// mean. `shell_command` rebuilds PATH from `getconf PATH`, which on macOS
/// omits Homebrew, so a bare `bash` there silently resolves to stock 3.2
/// while the guard saw 5.x. Prove the spawned shell is the one found on the
/// test's own PATH by putting a wrapper in front of it.
#[test]
fn spawns_the_bash_found_on_path() -> Result<()> {
    let helper = PxhTestHelper::new();
    let real = which::which("bash")?;
    let bin = helper.home_dir().join("wrapper-bin");
    fs::create_dir_all(&bin)?;
    let wrapper = bin.join("bash");
    fs::write(&wrapper, format!("#!/bin/sh\nPXH_TEST_WRAPPED=1 exec {} \"$@\"\n", real.display()))?;
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755))?;
    // nextest runs each test in its own process, so this is private to us.
    unsafe { env::set_var("PATH", format!("{}:{}", bin.display(), env::var("PATH")?)) };

    let mut session = ShellSession::spawn(&helper, Shell::Bash)?;
    let out = session.run("echo wrapped=$PXH_TEST_WRAPPED")?;
    assert!(out.contains("wrapped=1"), "pty shell was not the bash on PATH:\n{out}");
    session.exit()
}

fn export(helper: &PxhTestHelper) -> Result<Vec<Invocation>> {
    let output = helper.command_with_args(&["export"]).output()?;
    assert!(output.status.success(), "export failed: {}", String::from_utf8_lossy(&output.stderr));
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn commands(helper: &PxhTestHelper) -> Result<Vec<String>> {
    Ok(export(helper)?.into_iter().map(|inv| inv.command.to_string()).collect())
}

fn find<'a>(invocations: &'a [Invocation], needle: &str) -> Option<&'a Invocation> {
    invocations.iter().find(|inv| inv.command.to_string().contains(needle))
}

/// Pre-seed history with a command whose *output* differs from its text, so
/// a test can tell "the shell executed it" apart from "the TUI displayed it".
fn seed_history(helper: &PxhTestHelper, command: &str) -> Result<()> {
    let status = helper
        .command_with_args(&[
            "insert",
            "--shellname",
            "bash",
            "--hostname",
            &helper.hostname,
            "--username",
            &helper.username,
            "--session-id",
            "7",
            "--exit-status",
            "0",
            "--start-unix-timestamp",
            "1700000000",
            command,
        ])
        .status()?;
    assert!(status.success(), "seed insert should succeed");
    Ok(())
}

/// Generate a `#[test]` per shell for a body that takes a `Shell`.
macro_rules! for_each_shell {
    ($body:ident) => {
        mod $body {
            use super::*;
            #[test]
            fn bash() -> Result<()> {
                super::$body(Shell::Bash)
            }
            #[test]
            fn zsh() -> Result<()> {
                super::$body(Shell::Zsh)
            }
        }
    };
}

fn records_basic_commands(shell: Shell) -> Result<()> {
    let helper = PxhTestHelper::new();
    let mut session = ShellSession::spawn(&helper, shell)?;

    let transcript = session.run("echo PXH_DB_PATH=$PXH_DB_PATH")?;
    assert!(
        transcript.contains(&format!("PXH_DB_PATH={}", helper.db_path().display())),
        "shell should see the test DB path: {transcript}"
    );
    let transcript = session.run("echo 'Hello from interactive shell'")?;
    assert!(transcript.contains("Hello from interactive shell"));
    session.run("pwd")?;
    session.run("ls /tmp > /dev/null 2>&1")?;
    session.run("false")?;
    session.exit()?;

    let recorded = commands(&helper)?;
    for expected in
        ["echo 'Hello from interactive shell'", "pwd", "ls /tmp > /dev/null 2>&1", "false"]
    {
        assert!(recorded.iter().any(|c| c == expected), "missing {expected:?} in {recorded:?}");
    }
    Ok(())
}
for_each_shell!(records_basic_commands);

fn records_exit_status(shell: Shell) -> Result<()> {
    let helper = PxhTestHelper::new();
    let mut session = ShellSession::spawn(&helper, shell)?;
    session.run("true")?;
    session.run("false")?;
    session.run("sh -c 'exit 42'")?;
    session.exit()?;

    let invocations = export(&helper)?;
    let status = |cmd: &str| invocations.iter().find(|i| i.command == cmd).map(|i| i.exit_status);
    assert_eq!(status("true"), Some(Some(0)));
    assert_eq!(status("false"), Some(Some(1)));
    assert_eq!(status("sh -c 'exit 42'"), Some(Some(42)));
    Ok(())
}
for_each_shell!(records_exit_status);

fn records_working_directory(shell: Shell) -> Result<()> {
    let helper = PxhTestHelper::new();
    let dir1 = helper.home_dir().join("test1");
    let dir2 = helper.home_dir().join("test2");
    fs::create_dir(&dir1)?;
    fs::create_dir(&dir2)?;

    let mut session = ShellSession::spawn(&helper, shell)?;
    session.run(&format!("cd {}", dir1.display()))?;
    session.run("echo 'in test1'")?;
    session.run(&format!("cd {}", dir2.display()))?;
    session.run("echo 'in test2'")?;
    session.exit()?;

    let invocations = export(&helper)?;
    for (marker, dir) in [("in test1", "test1"), ("in test2", "test2")] {
        let inv = find(&invocations, marker).expect(marker);
        let cwd = inv.working_directory.as_ref().map(ToString::to_string).unwrap_or_default();
        assert!(cwd.ends_with(dir), "{marker} recorded in {cwd}, expected {dir}");
    }
    Ok(())
}
for_each_shell!(records_working_directory);

fn records_timing(shell: Shell) -> Result<()> {
    let helper = PxhTestHelper::new();
    let mut session = ShellSession::spawn(&helper, shell)?;
    session.run("sleep 1")?;
    session.exit()?;

    let invocations = export(&helper)?;
    let inv = find(&invocations, "sleep 1").expect("sleep should be recorded");
    let start = inv.start_unix_timestamp.expect("start timestamp");
    let end = inv.end_unix_timestamp.expect("seal should record an end timestamp");
    assert!(end > start, "sleep 1 should take at least a second: {start}..{end}");
    Ok(())
}
for_each_shell!(records_timing);

fn records_pipelines(shell: Shell) -> Result<()> {
    let helper = PxhTestHelper::new();
    let mut session = ShellSession::spawn(&helper, shell)?;
    session.run("echo 'hello world' | grep hello")?;
    session.run("printf 'b\\na\\nc\\n' | sort | head -1")?;
    session.run("echo 'test output' | cat > /dev/null")?;
    session.exit()?;

    let recorded = commands(&helper)?;
    assert!(recorded.iter().any(|c| c == "echo 'hello world' | grep hello"), "{recorded:?}");
    assert!(recorded.iter().any(|c| c.contains("sort | head -1")), "{recorded:?}");
    assert!(recorded.iter().any(|c| c.contains("cat > /dev/null")), "{recorded:?}");
    Ok(())
}
for_each_shell!(records_pipelines);

fn records_compound_commands(shell: Shell) -> Result<()> {
    let helper = PxhTestHelper::new();
    let mut session = ShellSession::spawn(&helper, shell)?;
    assert!(session.run("true && echo 'and succeeded'")?.contains("and succeeded"));
    assert!(session.run("false || echo 'or fallback'")?.contains("or fallback"));
    session.run("echo 'first'; echo 'second'")?;
    session.exit()?;

    let recorded = commands(&helper)?;
    for expected in [
        "true && echo 'and succeeded'",
        "false || echo 'or fallback'",
        "echo 'first'; echo 'second'",
    ] {
        assert!(recorded.iter().any(|c| c == expected), "missing {expected:?} in {recorded:?}");
    }
    Ok(())
}
for_each_shell!(records_compound_commands);

fn records_multiline_commands(shell: Shell) -> Result<()> {
    let helper = PxhTestHelper::new();
    let mut session = ShellSession::spawn(&helper, shell)?;
    // Backslash continuations: only the last line returns to the prompt.
    session.pty.send_line("echo 'line1' \\")?;
    session.pty.send_line("'line2' \\")?;
    let transcript = session.run("'line3'")?;
    assert!(transcript.contains("line1 line2 line3"), "{transcript}");

    session.pty.send_line("cat << 'ENDMARKER'")?;
    session.pty.send_line("heredoc content")?;
    let transcript = session.run("ENDMARKER")?;
    assert!(transcript.contains("heredoc content"), "{transcript}");
    session.exit()?;

    let recorded = commands(&helper)?;
    assert!(
        recorded.iter().any(|c| c.contains("line1") && c.contains("line2") && c.contains("line3")),
        "multiline echo should be recorded as one entry: {recorded:?}"
    );
    assert!(
        recorded.iter().any(|c| c.contains("ENDMARKER") && c.contains("heredoc content")),
        "heredoc should be recorded with its body: {recorded:?}"
    );
    Ok(())
}
for_each_shell!(records_multiline_commands);

fn records_background_commands(shell: Shell) -> Result<()> {
    let helper = PxhTestHelper::new();
    let mut session = ShellSession::spawn(&helper, shell)?;
    session.run("sleep 0.2 &")?;
    assert!(session.run("echo 'foreground'")?.contains("foreground"));
    session.run("wait")?;
    session.exit()?;

    let recorded = commands(&helper)?;
    assert!(recorded.iter().any(|c| c == "sleep 0.2 &"), "{recorded:?}");
    assert!(recorded.iter().any(|c| c == "echo 'foreground'"), "{recorded:?}");
    Ok(())
}
for_each_shell!(records_background_commands);

fn records_subshells_and_substitution(shell: Shell) -> Result<()> {
    let helper = PxhTestHelper::new();
    let mut session = ShellSession::spawn(&helper, shell)?;
    assert!(session.run("(cd /tmp && pwd)")?.contains("/tmp"));
    session.run("echo \"today is $(date +%Y)\"")?;
    session.exit()?;

    let recorded = commands(&helper)?;
    assert!(recorded.iter().any(|c| c.contains("today is $(date +%Y)")), "{recorded:?}");
    let subshell_recorded = recorded.iter().any(|c| c.contains("(cd /tmp"));
    match shell {
        // KNOWN LIMITATION: bash-preexec does not fire for a bare
        // parenthesised subshell. Pin it so a change in behaviour is noticed.
        Shell::Bash => assert!(!subshell_recorded, "bash-preexec now captures subshells?"),
        Shell::Zsh => assert!(subshell_recorded, "{recorded:?}"),
    }
    Ok(())
}
for_each_shell!(records_subshells_and_substitution);

fn records_special_characters(shell: Shell) -> Result<()> {
    let helper = PxhTestHelper::new();
    let mut session = ShellSession::spawn(&helper, shell)?;
    assert!(session.run("echo 'single quoted $VAR'")?.contains("single quoted $VAR"));
    assert!(session.run("VAR=test; echo \"double quoted $VAR\"")?.contains("double quoted test"));
    assert!(session.run("echo \"quotes: \\\"nested\\\"\"")?.contains("quotes: \"nested\""));
    assert!(session.run("echo 'asterisk * and question ?'")?.contains("asterisk * and question ?"));
    session.exit()?;

    let recorded = commands(&helper)?;
    for expected in [
        "echo 'single quoted $VAR'",
        "VAR=test; echo \"double quoted $VAR\"",
        "echo \"quotes: \\\"nested\\\"\"",
        "echo 'asterisk * and question ?'",
    ] {
        assert!(recorded.iter().any(|c| c == expected), "missing {expected:?} in {recorded:?}");
    }
    Ok(())
}
for_each_shell!(records_special_characters);

fn records_control_structures(shell: Shell) -> Result<()> {
    let helper = PxhTestHelper::new();
    let mut session = ShellSession::spawn(&helper, shell)?;
    session.run("for i in 1 2 3; do echo $i; done")?;
    assert!(session.run("if true; then echo 'condition met'; fi")?.contains("condition met"));
    session.run("x=0; while [ $x -lt 2 ]; do echo $x; x=$((x+1)); done")?;
    session.exit()?;

    let recorded = commands(&helper)?;
    for expected in [
        "for i in 1 2 3; do echo $i; done",
        "if true; then echo 'condition met'; fi",
        "x=0; while [ $x -lt 2 ]; do echo $x; x=$((x+1)); done",
    ] {
        assert!(recorded.iter().any(|c| c == expected), "missing {expected:?} in {recorded:?}");
    }
    Ok(())
}
for_each_shell!(records_control_structures);

#[test]
fn concurrent_sessions_get_distinct_session_ids() -> Result<()> {
    let helper = PxhTestHelper::new();
    let mut first = ShellSession::spawn(&helper, Shell::Bash)?;
    // Second spawn re-runs install; it is idempotent.
    let mut second = ShellSession::spawn(&helper, Shell::Bash)?;
    first.run("echo 'Hello from session 1'")?;
    second.run("echo 'Hello from session 2'")?;
    first.exit()?;
    second.exit()?;

    let invocations = export(&helper)?;
    let s1 = find(&invocations, "session 1").expect("session 1 recorded").session_id;
    let s2 = find(&invocations, "session 2").expect("session 2 recorded").session_id;
    assert_ne!(s1, s2, "each interactive shell must get its own session id");
    Ok(())
}

// -- Ctrl-R round trip -----------------------------------------------------
//
// These exercise the full contract between `pxh recall --shell-mode` and the
// shell-side widgets: the TUI prints `run:`/`edit:` prefixed output and the
// widget turns it into an executed or editable command line.
//
// The seeded command's *output* (`pxh-ran-42`) differs from its text, so
// tests can tell "the shell executed it" apart from "the TUI displayed it" or
// "readline echoed it".

const CTRL_R: &str = "\x12";
const CTRL_C: &str = "\x03";
const ENTER: &str = "\r";
const TAB: &str = "\t";
const LEAVE_ALT_SCREEN: &str = "\x1b[?1049l";
/// Part of the recall status bar; its appearance means the TUI is up.
const TUI_READY: &str = "Enter Run";
const SEED: &str = "echo pxh-ran-$((6*7))";
const SEED_OUTPUT: &str = "pxh-ran-42";

/// Debian-family `/etc/zsh/zshrc` runs `compinit` for every interactive
/// shell on Ubuntu hosts (GitHub runners included). When `compaudit` flags
/// an insecure `fpath` directory there, compinit stops to ask "[y] or abort
/// [n]?" and the session never reaches its prompt. The helper must opt out
/// via the documented `skip_global_compinit` knob.
#[test]
fn zsh_session_skips_global_compinit() -> Result<()> {
    let helper = PxhTestHelper::new();
    let mut session = ShellSession::spawn(&helper, Shell::Zsh)?;
    let transcript =
        session.run("echo skip=$skip_global_compinit compinit=${+functions[compinit]}")?;
    assert!(
        transcript.contains("skip=1 compinit=0"),
        "global compinit should be skipped: {transcript}"
    );
    session.exit()
}

fn open_recall(session: &mut ShellSession) -> Result<()> {
    session.pty.send(CTRL_R)?;
    session.pty.flush()?;
    session.pty.exp_string(TUI_READY)?;
    Ok(())
}

fn press(session: &mut ShellSession, key: &str) -> Result<()> {
    session.pty.send(key)?;
    session.pty.flush()?;
    Ok(())
}

fn ctrl_r_enter_runs_selected_command(shell: Shell) -> Result<()> {
    let helper = PxhTestHelper::new();
    seed_history(&helper, SEED)?;
    let mut session = ShellSession::spawn(&helper, shell)?;

    open_recall(&mut session)?;
    press(&mut session, ENTER)?;
    session.expect_output(SEED_OUTPUT)?;
    session.exit()?;

    let invocations = export(&helper)?;
    let runs: Vec<_> = invocations.iter().filter(|i| i.command == SEED).collect();
    assert_eq!(runs.len(), 2, "seed plus the recalled execution: {invocations:?}");
    assert!(runs.iter().any(|i| i.shellname == shell.name()), "recalled run recorded via hooks");
    Ok(())
}
for_each_shell!(ctrl_r_enter_runs_selected_command);

fn ctrl_r_tab_edits_without_running(shell: Shell) -> Result<()> {
    let helper = PxhTestHelper::new();
    seed_history(&helper, SEED)?;
    let mut session = ShellSession::spawn(&helper, shell)?;

    open_recall(&mut session)?;
    press(&mut session, TAB)?;
    // The shell redraws its line with the selection in place, unexecuted.
    session.pty.exp_string(LEAVE_ALT_SCREEN)?;
    session.pty.exp_string(SEED)?;
    // Append to prove the buffer is editable, then run it.
    session.pty.send_line("; echo pxh-edited")?;
    session.pty.exp_string(SEED_OUTPUT)?;
    session.expect_output("pxh-edited")?;
    session.exit()?;

    let recorded = commands(&helper)?;
    assert!(recorded.iter().any(|c| c == &format!("{SEED}; echo pxh-edited")), "{recorded:?}");
    Ok(())
}
for_each_shell!(ctrl_r_tab_edits_without_running);

fn ctrl_r_ctrl_c_leaves_line_untouched(shell: Shell) -> Result<()> {
    let helper = PxhTestHelper::new();
    seed_history(&helper, SEED)?;
    let mut session = ShellSession::spawn(&helper, shell)?;

    open_recall(&mut session)?;
    // Ctrl-C rather than Esc: a lone ESC byte immediately followed by typed
    // text is indistinguishable from an Alt-chord to the TUI's input parser.
    press(&mut session, CTRL_C)?;
    // Wait for the TUI to release the terminal so the next keystrokes reach
    // the shell rather than a still-exiting recall process.
    session.pty.exp_string(LEAVE_ALT_SCREEN)?;
    session.pty.send_line("echo pxh-ok-$((1+1))")?;
    session.expect_output("pxh-ok-2")?;
    session.exit()?;

    let recorded = commands(&helper)?;
    assert_eq!(recorded.iter().filter(|c| c.as_str() == SEED).count(), 1, "{recorded:?}");
    Ok(())
}
for_each_shell!(ctrl_r_ctrl_c_leaves_line_untouched);
