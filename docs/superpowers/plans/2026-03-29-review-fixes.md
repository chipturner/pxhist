# Review Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix performance regression from migrate_host_settings on hot path, add config parse warnings, add smoke tests, fix escape sanitization gaps, fix TUI flicker, and handle SIGPIPE gracefully.

**Architecture:** Six independent fixes, each in its own commit. No cross-task dependencies.

**Tech Stack:** Rust, rusqlite, crossterm, clap_complete, libc, TOML

**Spec:** `docs/superpowers/specs/2026-03-29-review-fixes-design.md`

---

## Task 1: Move `migrate_host_settings` off the hot path

**Files:**
- Modify: `src/lib.rs` (~line 258, ~line 122)
- Modify: `src/main.rs` (~line 2427 InstallCommand, ~line 2433 ConfigCommand)

### Step 1.1: Make migrate_host_settings public

- [ ] **Edit `src/lib.rs` line 122:**

Change:
```rust
fn migrate_host_settings(conn: &Connection) {
```
To:
```rust
/// Migrate host settings from DB legacy storage to config file.
/// Called from install and config commands, not on every connection.
// TODO: also call from future `pxh doctor` command
pub fn migrate_host_settings(conn: &Connection) {
```

### Step 1.2: Remove from sqlite_connection

- [ ] **Edit `src/lib.rs` ~line 257-258:**

Remove these two lines:
```rust
    // Migrate host settings from DB to config file
    migrate_host_settings(&conn);
```

### Step 1.3: Call from InstallCommand

- [ ] **Edit `src/main.rs` ~line 2427:**

Change:
```rust
        Commands::Install(cmd) => {
            cmd.go()?;
        }
```
To:
```rust
        Commands::Install(cmd) => {
            cmd.go()?;
            // Migrate host settings to config on install (not on every command)
            if let Ok(conn) = make_conn() {
                pxh::migrate_host_settings(&conn);
            }
        }
```

### Step 1.4: Call from ConfigCommand

- [ ] **Edit `src/main.rs` ~line 2433:**

Change:
```rust
        Commands::Config(cmd) => {
            cmd.go()?;
        }
```
To:
```rust
        Commands::Config(cmd) => {
            // Migrate host settings before editing config
            if let Ok(conn) = make_conn() {
                pxh::migrate_host_settings(&conn);
            }
            cmd.go()?;
        }
```

### Step 1.5: Verify and commit

- [ ] Run: `cargo test` -- all pass
- [ ] Run: `cargo clippy -- -D warnings` -- clean
- [ ] Commit:
```bash
git add src/lib.rs src/main.rs
git commit -m "perf: move migrate_host_settings off the hot path to install/config only"
```

---

## Task 2: Config parse errors should warn

**Files:**
- Modify: `src/recall/config.rs` (~line 150)
- Test: `src/recall/config.rs` (existing test module)

### Step 2.1: Add warning on parse failure

- [ ] **Edit `src/recall/config.rs` `load_from_path` (~line 150):**

Change:
```rust
    pub fn load_from_path(path: &PathBuf) -> Option<Self> {
        let content = fs::read_to_string(path).ok()?;
        toml::from_str(&content).ok()
    }
```
To:
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

### Step 2.2: Add test for warning behavior

- [ ] **Add test in `src/recall/config.rs` test module:**

```rust
    #[test]
    fn test_invalid_toml_returns_none() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("bad.toml");
        std::fs::write(&path, "this is not valid [[ toml").unwrap();
        assert!(Config::load_from_path(&path).is_none());
    }

    #[test]
    fn test_wrong_type_returns_none() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("bad_type.toml");
        // result_limit should be a number, not a string
        std::fs::write(&path, "[recall]\nresult_limit = \"not a number\"\n").unwrap();
        assert!(Config::load_from_path(&path).is_none());
    }
```

### Step 2.3: Verify and commit

- [ ] Run: `cargo test` -- all pass
- [ ] Run: `cargo clippy -- -D warnings` -- clean
- [ ] Commit:
```bash
git add src/recall/config.rs
git commit -m "config: warn on parse errors instead of silently falling back to defaults"
```

---

## Task 3: Smoke tests for stats, completions, config

**Files:**
- Modify: `tests/integration_tests.rs`

### Step 3.1: Add smoke tests

- [ ] **Add to `tests/integration_tests.rs`:**

```rust
#[test]
fn stats_command() {
    let caller = PxhTestHelper::new();
    insert_test_command(caller.db_path(), "echo hello", 1);
    let output = caller.command_with_args(&["stats"]).output().unwrap();
    assert!(output.status.success(), "stats should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Total"), "stats output should contain 'Total'");
}

#[test]
fn completions_command_bash() {
    let caller = PxhTestHelper::new();
    let output = caller.command_with_args(&["completions", "bash"]).output().unwrap();
    assert!(output.status.success(), "completions bash should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("complete") || stdout.contains("_pxh"), "bash completions should contain shell patterns");
}

#[test]
fn completions_command_zsh() {
    let caller = PxhTestHelper::new();
    let output = caller.command_with_args(&["completions", "zsh"]).output().unwrap();
    assert!(output.status.success(), "completions zsh should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("#compdef") || stdout.contains("_pxh"), "zsh completions should contain shell patterns");
}

#[test]
fn config_command_creates_default() {
    let caller = PxhTestHelper::new();
    let config_path = caller.home_dir().join(".pxh/config.toml");
    // Remove the test-default config so pxh config creates a fresh one
    let _ = std::fs::remove_file(&config_path);
    assert!(!config_path.exists(), "config should not exist before test");

    // --path just prints the path without opening an editor
    let output = caller.command_with_args(&["config", "--path"]).output().unwrap();
    assert!(output.status.success(), "config --path should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("config.toml"), "should print config path");
}
```

### Step 3.2: Verify and commit

- [ ] Run: `cargo test` -- all pass
- [ ] Run: `cargo clippy -- -D warnings` -- clean
- [ ] Commit:
```bash
git add tests/integration_tests.rs
git commit -m "test: add smoke tests for stats, completions, and config commands"
```

---

## Task 4: Fix OSC/DCS in `sanitize_for_display`

**Files:**
- Modify: `src/recall/tui.rs` (~line 52, sanitize_for_display function)

### Step 4.1: Write failing tests

- [ ] **Add to existing test module in `src/recall/tui.rs`:**

```rust
    #[test]
    fn test_sanitize_strips_osc_sequences() {
        // OSC title change: \x1b]0;title\x07
        assert_eq!(sanitize_for_display("\x1b]0;My Title\x07visible"), "visible");
        // OSC with ST terminator: \x1b]0;title\x1b\\
        assert_eq!(sanitize_for_display("\x1b]2;Title\x1b\\visible"), "visible");
        // OSC 52 clipboard: \x1b]52;c;base64\x07
        assert_eq!(sanitize_for_display("\x1b]52;c;SGVsbG8=\x07after"), "after");
    }

    #[test]
    fn test_sanitize_strips_dcs_sequences() {
        // DCS: \x1bP...\x1b\\
        assert_eq!(sanitize_for_display("\x1bPsome data\x1b\\visible"), "visible");
    }

    #[test]
    fn test_sanitize_strips_other_esc_sequences() {
        // SS2 (single shift 2): \x1bN
        assert_eq!(sanitize_for_display("\x1bNvisible"), "visible");
        // SS3 (single shift 3): \x1bO
        assert_eq!(sanitize_for_display("\x1bOvisible"), "visible");
    }
```

### Step 4.2: Run tests to verify they fail

- [ ] Run: `cargo test test_sanitize_strips_osc -- --nocapture`
- [ ] Expected: FAIL

### Step 4.3: Fix sanitize_for_display

- [ ] **Edit `src/recall/tui.rs` sanitize_for_display (~line 52):**

Replace the entire function:

```rust
fn sanitize_for_display(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\x1b' => {
                match chars.peek() {
                    // CSI: \x1b[ ... letter
                    Some(&'[') => {
                        chars.next();
                        while let Some(&c) = chars.peek() {
                            chars.next();
                            if c.is_ascii_alphabetic() {
                                break;
                            }
                        }
                    }
                    // OSC: \x1b] ... (BEL or ST)
                    Some(&']') => {
                        chars.next();
                        while let Some(c) = chars.next() {
                            if c == '\x07' {
                                break;
                            }
                            if c == '\x1b' && chars.peek() == Some(&'\\') {
                                chars.next();
                                break;
                            }
                        }
                    }
                    // DCS: \x1bP ... ST (\x1b\\)
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
                    Some(_) => {
                        chars.next();
                    }
                    None => {}
                }
            }
            '\n' | '\r' => result.push(' '),
            '\x00'..='\x08' | '\x0b'..='\x0c' | '\x0e'..='\x1f' | '\x7f' => {}
            '\t' => result.push(' '),
            _ => result.push(c),
        }
    }

    result
}
```

### Step 4.4: Run tests to verify they pass

- [ ] Run: `cargo test test_sanitize` -- all pass

### Step 4.5: Verify existing tests still pass

- [ ] Check that `test_sanitize_handles_incomplete_escape_sequences` still passes -- the new code handles bare `\x1b` at end of string via `None => {}`.

### Step 4.6: Verify and commit

- [ ] Run: `cargo test` -- all pass
- [ ] Run: `cargo clippy -- -D warnings` -- clean
- [ ] Commit:
```bash
git add src/recall/tui.rs
git commit -m "security: strip OSC/DCS escape sequences in recall display"
```

---

## Task 5: Fix recall TUI flicker

**Files:**
- Modify: `src/recall/tui.rs` (RecallTui struct, new(), run(), and draw-triggering methods)

### Step 5.1: Add `needs_redraw` field

- [ ] **Edit `src/recall/tui.rs` RecallTui struct (~line 242):**

Add after `status_message`:
```rust
    needs_redraw: bool,
```

Initialize to `true` in `RecallTui::new()` (find the struct literal, add `needs_redraw: true`).

### Step 5.2: Update run() to skip redundant draws

- [ ] **Edit `src/recall/tui.rs` run() (~line 496):**

Replace:
```rust
    pub fn run(&mut self) -> Result<Option<String>, Box<dyn std::error::Error>> {
        loop {
            self.draw()?;

            // Poll with timeout for responsive cancellation and future async features
            if !event::poll(Duration::from_millis(100))? {
                continue;
            }

            if let Event::Key(key) = event::read()? {
                let action = self.handle_key(key)?;
```

With:
```rust
    pub fn run(&mut self) -> Result<Option<String>, Box<dyn std::error::Error>> {
        loop {
            // Check if timed status messages have expired
            if let Some((_, until)) = self.status_message {
                if Instant::now() >= until {
                    self.status_message = None;
                    self.needs_redraw = true;
                }
            }
            if let Some(until) = self.flash_until {
                if Instant::now() >= until {
                    self.flash_until = None;
                    self.needs_redraw = true;
                }
            }

            if self.needs_redraw {
                self.draw()?;
                self.needs_redraw = false;
            }

            if !event::poll(Duration::from_millis(100))? {
                continue;
            }

            if let Event::Key(key) = event::read()? {
                self.needs_redraw = true;
                let action = self.handle_key(key)?;
```

### Step 5.3: Replace `if let Key` with `match` to also handle Resize

- [ ] **Edit `src/recall/tui.rs` run():**

Replace the `if let Event::Key(key) = event::read()? {` block (from Step 5.2) with a `match` that also catches Resize:
```rust
            match event::read()? {
                Event::Key(key) => {
                    self.needs_redraw = true;
                    let action = self.handle_key(key)?;
                    match action {
                        KeyAction::Continue => continue,
                        KeyAction::Select | KeyAction::Edit | KeyAction::EditBeginning => {
                            self.cleanup()?;
                            if !self.shell_mode {
                                self.print_entry_details();
                                return Ok(None);
                            }
                            let prefix = match action {
                                KeyAction::Select => "run",
                                KeyAction::EditBeginning => "edit-a",
                                _ => "edit",
                            };
                            let result =
                                self.get_selected_command().map(|cmd| format!("{prefix}:{cmd}"));
                            return Ok(result);
                        }
                        KeyAction::Cancel => {
                            self.cleanup()?;
                            return Ok(None);
                        }
                    }
                }
                Event::Resize(_, _) => {
                    self.needs_redraw = true;
                }
                _ => {}
            }
```

### Step 5.4: Verify and commit

- [ ] Run: `cargo test` -- all pass
- [ ] Run: `cargo clippy -- -D warnings` -- clean
- [ ] Commit:
```bash
git add src/recall/tui.rs
git commit -m "recall: skip redundant redraws when no input or state change"
```

---

## Task 6: Handle SIGPIPE gracefully

**Files:**
- Modify: `Cargo.toml` (add libc dependency)
- Modify: `src/main.rs` (top of main())

### Step 6.1: Add libc dependency

- [ ] **Edit `Cargo.toml` dependencies section:**

Add:
```toml
libc = "0.2"
```

### Step 6.2: Reset SIGPIPE handler

- [ ] **Edit `src/main.rs` main() (~line 2393):**

Add at the very top of `main()`, before `env_logger::init()`:

```rust
    // Reset SIGPIPE to default OS behavior so piping to head/grep exits cleanly
    // instead of producing a BrokenPipe error.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
```

### Step 6.3: Verify and commit

- [ ] Run: `cargo test` -- all pass
- [ ] Run: `cargo clippy -- -D warnings` -- clean
- [ ] Test manually: `cargo run -- show -l 0 | head -1` should exit 0
- [ ] Commit:
```bash
git add Cargo.toml src/main.rs
git commit -m "fix: handle SIGPIPE gracefully for piped output"
```
