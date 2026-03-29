# Review Fixes: Performance, UX, Security, Tests, Flicker

## Context

Code review of the feature batch identified performance regressions, silent error handling, missing test coverage, a terminal escape sanitization gap, and a TUI redraw inefficiency.

---

## Fix 1 (P0): Move `migrate_host_settings` out of `sqlite_connection`

### Problem
`migrate_host_settings` runs inside `sqlite_connection()`, which is called for every command (insert, seal, show, etc.). It loads config from disk, queries the DB, and may write the config file -- all on the hot path of `pxh insert` and `pxh seal` which execute on every shell command.

### Fix
Remove the `migrate_host_settings(&conn)` call from `sqlite_connection()` (src/lib.rs ~line 258). Call it only from:
- `InstallCommand::go()` (after creating the DB connection)
- `ConfigCommand::go()` (after creating the DB connection)

Add comment: `// TODO: also call from future pxh doctor command`

### Files
- `src/lib.rs`: remove call from `sqlite_connection()`
- `src/main.rs`: add call in `InstallCommand::go()` and `ConfigCommand::go()`

---

## Fix 2 (P1): Config parse errors should warn

### Problem
`Config::load_from_path()` uses `.ok()` on `toml::from_str()`, silently discarding parse errors. A typo in config.toml causes all settings to silently revert to defaults.

### Fix
Change `load_from_path()` to log a warning on parse failure:

```rust
pub fn load_from_path(path: &PathBuf) -> Option<Self> {
    let content = fs::read_to_string(path).ok()?;
    match toml::from_str(&content) {
        Ok(config) => Some(config),
        Err(e) => {
            eprintln!("pxh: warning: failed to parse {}: {e}", path.display());
            eprintln!("pxh: using default configuration");
            None
        }
    }
}
```

`fs::read_to_string` failure stays silent (file not found is expected on fresh installs).

### Files
- `src/recall/config.rs`

---

## Fix 3 (P1): Smoke tests for stats, completions, config

### Tests to add

**stats_command**: Run `pxh stats` with an empty DB, verify exit 0 and output contains recognizable headers (e.g., "Commands" or "Total").

**completions_command_bash**: Run `pxh completions bash`, verify exit 0 and output contains `_pxh` or `complete` (bash completion patterns).

**completions_command_zsh**: Run `pxh completions zsh`, verify exit 0 and output contains `#compdef` or `_pxh` (zsh completion patterns).

**config_command**: Run `pxh config` in a fresh temp dir, verify it creates a config file at the expected path and exits successfully.

### Files
- `tests/integration_tests.rs`

---

## Fix 4 (P2): Fix OSC/DCS in `sanitize_for_display`

### Problem
`sanitize_for_display` only strips CSI sequences (`\x1b[...letter`). OSC (`\x1b]...BEL`) and DCS (`\x1bP...ST`) sequences pass through, which could affect terminal state if a command string contains them.

### Fix
Extend the ESC handler in `sanitize_for_display`:

```rust
'\x1b' => {
    match chars.peek() {
        // CSI: \x1b[ ... letter
        Some(&'[') => {
            chars.next();
            while let Some(&c) = chars.peek() {
                chars.next();
                if c.is_ascii_alphabetic() { break; }
            }
        }
        // OSC: \x1b] ... (BEL or ST)
        Some(&']') => {
            chars.next();
            while let Some(c) = chars.next() {
                if c == '\x07' { break; }
                if c == '\x1b' && chars.peek() == Some(&'\\') {
                    chars.next();
                    break;
                }
            }
        }
        // DCS: \x1bP ... ST
        Some(&'P') => {
            chars.next();
            while let Some(c) = chars.next() {
                if c == '\x1b' && chars.peek() == Some(&'\\') {
                    chars.next();
                    break;
                }
            }
        }
        // Any other ESC + char: skip one char (SS2, SS3, etc.)
        Some(_) => { chars.next(); }
        None => {}
    }
}
```

### Files
- `src/recall/tui.rs`

---

## Fix 5: Recall TUI flicker (skip redundant redraws)

### Problem
The TUI redraws the entire screen every 100ms even when nothing changed. Causes visible flicker, especially noticeable with no input.

### Fix
Add `needs_redraw: bool` to `RecallTui`. Initialize to `true`. Set to `true` on:
- Any key event (after `handle_key`)
- Terminal resize (if detected)
- Status message state change (expiry check)

In `run()`, only call `draw()` when `needs_redraw` is true. Reset to `false` after drawing.

The 100ms poll timeout stays (needed for status_message/flash expiry) but the actual draw is skipped when nothing changed.

### Files
- `src/recall/tui.rs`

---

## Fix 6: Handle SIGPIPE gracefully

### Problem
`pxh show | head` triggers SIGPIPE when the pipe closes. Rust's default SIGPIPE handler converts this to a `BrokenPipe` io::Error, which propagates up as a non-zero exit code. This is noisy and incorrect -- piping to head is normal usage.

### Fix
Reset SIGPIPE to the default OS behavior (terminate silently) at the top of `main()`:

```rust
// Reset SIGPIPE to default behavior so piping to head/grep exits cleanly.
unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL); }
```

No new dependency needed -- `libc` is already a transitive dependency via rusqlite. Add it as a direct dependency if not already present, or use the nightly `unix_sigpipe` attribute alternative. Check Cargo.toml.

### Files
- `src/main.rs`
- Possibly `Cargo.toml` (add `libc` if not already direct)

---

## Verification

- `cargo test` -- all tests pass
- `cargo clippy -- -D warnings` -- clean
- `cargo build --release`
