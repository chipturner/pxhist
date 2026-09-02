//! `pxh bootstrap` end to end with a scripted `ssh` and a scripted `curl`:
//! the real `install.sh` from this repo runs against a fake GitHub release
//! containing the binary under test, on a "remote" that is a second temp
//! `$HOME` on this machine. No network, no sshd.
//!
//! Bootstrap hands the remote login shell one command string (fetch
//! install.sh, run it with `PXH_INSTALL_DIR`), probes `pxh --version` through
//! the same candidate paths `pxh sync --remote` uses, then runs that sync.
//! All of that runs for real here; only `ssh` (runs the string locally) and
//! `curl` (serves local files) are scripted.

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use pxh::test_utils::{PxhTestHelper, pxh_path};

const HOST: &str = "devbox";
const RELEASE: &str = env!("CARGO_PKG_VERSION");

/// Serves install.sh (from the repo) and release assets (from
/// `$FAKE_DIR/assets/<tag>/`) the way `curl -sSfL URL [-o FILE]` would,
/// 404-ing (exit 22, like `curl -f`) anything else. `$FAKE_DIR/mode` set to
/// `script-404` makes the install.sh fetch itself fail.
const FAKE_CURL: &str = r#"#!/bin/sh
out=; url=; prev=
for a in "$@"; do
    [ "$prev" = "-o" ] && out=$a
    case $a in http*) url=$a ;; esac
    prev=$a
done
echo "$url" >> "$FAKE_DIR/curl.calls"
mode=$(cat "$FAKE_DIR/mode" 2>/dev/null || echo ok)
notfound() { echo "curl: (22) The requested URL returned error: 404" >&2; exit 22; }
case $url in
    */install.sh)
        [ "$mode" = script-404 ] && notfound
        src=$REPO_INSTALL_SH ;;
    */releases/latest/download/*)
        src=$FAKE_DIR/assets/latest/${url##*/} ;;
    */releases/download/*)
        tag=${url%/*}; tag=${tag##*/}
        src=$FAKE_DIR/assets/$tag/${url##*/} ;;
    *) src= ;;
esac
[ -n "$src" ] && [ -f "$src" ] || notfound
if [ -n "$out" ]; then cp "$src" "$out"; else cat "$src"; fi
"#;

struct Fixture {
    helper: PxhTestHelper,
    fake: PathBuf,
    remote_home: PathBuf,
    ssh: String,
}

impl Fixture {
    fn new() -> Self {
        let helper = PxhTestHelper::new();
        let fake = helper.home_dir().join("fake");
        let remote_home = helper.home_dir().join("remote");
        fs::create_dir_all(fake.join("bin")).unwrap();
        fs::create_dir_all(&remote_home).unwrap();

        // The "remote" is this machine: run the command string in the remote
        // `$HOME` as sshd would, with only the fakes and system dirs on PATH
        // so a pxh installed on this machine can never satisfy the probe.
        // Every argv is appended to `$FAKE_DIR/ssh.calls`.
        let fake_ssh = format!(
            "#!/bin/sh\n\
             echo \"$*\" >> '{fake}/ssh.calls'\n\
             for a; do last=$a; done\n\
             export HOME='{remote}' PATH='{fake}/bin:/usr/bin:/bin' FAKE_DIR='{fake}'\n\
             export REPO_INSTALL_SH='{repo}/install.sh' PXH_HOSTNAME=remotebox\n\
             unset PXH_DB_PATH\n\
             cd \"$HOME\" && exec sh -c \"$last\"\n",
            fake = fake.display(),
            remote = remote_home.display(),
            repo = env!("CARGO_MANIFEST_DIR"),
        );
        for (name, body) in [("ssh", fake_ssh.as_str()), ("curl", FAKE_CURL)] {
            let path = fake.join("bin").join(name);
            fs::write(&path, body).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let ssh = fake.join("bin/ssh").display().to_string();
        let f = Self { helper, fake, remote_home, ssh };
        f.publish_release(&format!("v{RELEASE}"));
        f
    }

    fn set_mode(&self, mode: &str) {
        fs::write(self.fake.join("mode"), mode).unwrap();
    }

    /// Stage a release under `assets/<tag>/`: the tarball install.sh expects
    /// for this platform, containing the binary under test, plus SHA256SUMS
    /// in the layout release.yml produces (`sha256sum pxh-*.tar.gz`).
    fn publish_release(&self, tag: &str) {
        let assets = self.fake.join("assets").join(tag);
        let stage = assets.join("stage");
        fs::create_dir_all(&stage).unwrap();
        fs::copy(pxh_path(), stage.join("pxh")).unwrap();
        let tarball = format!("pxh-{}.tar.gz", host_target());
        let ok = Command::new("tar")
            .args(["czf", &tarball, "-C", "stage", "pxh"])
            .current_dir(&assets)
            .status()
            .unwrap()
            .success();
        assert!(ok, "tar");
        let ok = Command::new("sh")
            .args([
                "-c",
                "{ command -v sha256sum >/dev/null && sha256sum pxh-*.tar.gz \
                 || shasum -a 256 pxh-*.tar.gz; } > SHA256SUMS",
            ])
            .current_dir(&assets)
            .status()
            .unwrap()
            .success();
        assert!(ok, "checksums");
    }

    /// Three local commands so the auto-sync has something to push.
    fn seed_local_history(&self) {
        for cmd in ["echo one", "echo two", "echo three"] {
            let ok = self
                .helper
                .command_with_args(&[
                    "insert",
                    "--shellname",
                    "bash",
                    "--hostname",
                    "laptop",
                    "--username",
                    "u",
                    "--working-directory",
                    "/tmp",
                    "--session-id",
                    "1",
                    cmd,
                ])
                .status()
                .unwrap()
                .success();
            assert!(ok, "insert {cmd}");
        }
    }

    fn bootstrap(&self, extra: &[&str]) -> Output {
        let mut args = vec!["bootstrap", HOST, "--ssh-cmd", &self.ssh];
        args.extend(extra);
        self.helper.command_with_args(&args).stdin(Stdio::null()).output().unwrap()
    }

    fn ssh_calls(&self) -> Vec<String> {
        fs::read_to_string(self.fake.join("ssh.calls"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn curl_calls(&self) -> String {
        fs::read_to_string(self.fake.join("curl.calls")).unwrap_or_default()
    }

    fn remote_db(&self) -> PathBuf {
        self.remote_home.join(".local/share/pxh/pxh.db")
    }

    /// Every `pxh` file under the remote home -- what the install left.
    fn installed(&self) -> Vec<PathBuf> {
        let mut found = Vec::new();
        walk(&self.remote_home, &mut found);
        found.sort();
        found
    }
}

fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk(&p, found);
        } else if p.file_name().is_some_and(|n| n == "pxh") {
            found.push(p);
        }
    }
}

/// The target triple install.sh derives from `uname` on this machine.
fn host_target() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "x86_64-unknown-linux-musl",
        ("linux", "aarch64") => "aarch64-unknown-linux-musl",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        other => panic!("install.sh does not support {other:?}"),
    }
}

fn count_commands(db: &Path) -> i64 {
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.query_row("SELECT COUNT(*) FROM command_history", [], |r| r.get(0)).unwrap()
}

fn text(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

#[test]
fn bootstrap_installs_this_release_confirms_it_and_syncs() {
    let f = Fixture::new();
    f.seed_local_history();
    let out = f.bootstrap(&[]);
    let all = text(&out);
    assert!(out.status.success(), "{all}");

    let bin = f.remote_home.join(".local/bin/pxh");
    assert_eq!(f.installed(), vec![bin.clone()], "{all}");
    assert!(fs::metadata(&bin).unwrap().permissions().mode() & 0o111 != 0, "executable");
    assert!(
        f.curl_calls().contains(&format!("/releases/download/v{RELEASE}/pxh-")),
        "{}",
        f.curl_calls()
    );

    assert!(all.contains(&format!("installed pxh {RELEASE} on {HOST}")), "{all}");
    assert!(all.contains("matches this machine"), "{all}");

    // install, probe, sync: three ssh round trips, the last one a real sync
    // through the freshly installed binary.
    let calls = f.ssh_calls();
    assert_eq!(calls.len(), 3, "{calls:#?}");
    assert!(calls[2].contains("sync --server"), "{calls:#?}");
    assert_eq!(count_commands(&f.remote_db()), 3, "{all}");
    assert!(all.contains(&format!("ssh {HOST} pxh install")), "{all}");
}

#[test]
fn bootstrap_no_sync_stops_after_the_probe_and_points_at_sync() {
    let f = Fixture::new();
    f.seed_local_history();
    let out = f.bootstrap(&["--no-sync"]);
    let all = text(&out);
    assert!(out.status.success(), "{all}");
    assert_eq!(f.ssh_calls().len(), 2, "{:#?}", f.ssh_calls());
    assert!(!f.remote_db().exists(), "{all}");
    assert!(all.contains(&format!("pxh sync --remote {HOST}")), "{all}");
}

#[test]
fn bootstrap_release_latest_takes_the_newest_published_build() {
    let f = Fixture::new();
    f.publish_release("latest");
    let out = f.bootstrap(&["--release", "latest", "--no-sync"]);
    let all = text(&out);
    assert!(out.status.success(), "{all}");
    assert!(f.curl_calls().contains("/releases/latest/download/pxh-"), "{}", f.curl_calls());
    assert!(all.contains(&format!("installed pxh (latest) on {HOST}")), "{all}");
}

#[test]
fn bootstrap_fails_when_the_release_has_no_published_build() {
    let f = Fixture::new();
    let out = f.bootstrap(&["--release", "9.9.9"]);
    let all = text(&out);
    assert!(!out.status.success(), "{all}");
    assert!(all.contains("remote install failed"), "{all}");
    assert!(all.contains("--release latest"), "{all}");
    assert!(f.installed().is_empty(), "{all}");
    assert_eq!(f.ssh_calls().len(), 1, "no probe or sync after a failed install");
}

#[test]
fn bootstrap_fails_when_install_sh_cannot_be_fetched() {
    let f = Fixture::new();
    f.set_mode("script-404");
    let out = f.bootstrap(&[]);
    let all = text(&out);
    assert!(!out.status.success(), "{all}");
    assert!(all.contains("could not fetch install.sh"), "{all}");
    assert!(!all.contains("installed pxh"), "{all}");
    assert!(f.installed().is_empty(), "{all}");
}

/// A relative `--install-dir` is relative to the remote home, not to the
/// scratch directory install.sh unpacks in (which it deletes on exit).
#[test]
fn bootstrap_relative_install_dir_is_relative_to_the_remote_home() {
    let f = Fixture::new();
    let out = f.bootstrap(&["--install-dir", "bin", "--no-sync"]);
    let all = text(&out);
    assert!(out.status.success(), "{all}");
    assert_eq!(f.installed(), vec![f.remote_home.join("bin/pxh")], "{all}");
    assert!(all.contains("matches this machine"), "{all}");
}

/// An install dir `pxh sync` does not probe: bootstrap says so, tells the
/// user the `--remote-pxh` to pass from now on, and still syncs this once
/// through the explicit path.
#[test]
fn bootstrap_install_dir_off_the_sync_path_warns_and_syncs_explicitly() {
    let f = Fixture::new();
    f.seed_local_history();
    let out = f.bootstrap(&["--install-dir", "opt/pxh"]);
    let all = text(&out);
    assert!(out.status.success(), "{all}");
    assert_eq!(f.installed(), vec![f.remote_home.join("opt/pxh/pxh")], "{all}");
    assert!(all.contains("--remote-pxh opt/pxh/pxh"), "{all}");
    let calls = f.ssh_calls();
    assert!(calls.last().unwrap().contains("opt/pxh/pxh --db"), "{calls:#?}");
    assert_eq!(count_commands(&f.remote_db()), 3, "{all}");
}
