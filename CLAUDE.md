# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## pxh - Portable Shell History Manager

pxh is a fast, cross-shell history mining tool that uses SQLite to provide powerful search capabilities across shell command history. It supports bash and zsh, tracks rich metadata (directory, host, user, exit codes, durations), and provides bidirectional synchronization across machines.

## Build Commands
- Build: `cargo build` or `cargo build --release`
- Quick validation: `just check` (fmt-check + clippy incl. tests + tests + rustdoc; mirrors the CI gate)
- Perf guard: `just perf` (release build, 550k synthetic rows, ~1 min)
- Run tests: `just test` (cargo-nextest; filter with e.g. `just test sync`)
- Run full suite repeatedly to catch flakes: `just stress` (default 10 runs; uses the `stress` nextest profile: no retries, no fail-fast; nightly CI runs it and files a rolling issue)
- Docker end-to-end suite: `just docker-e2e`
- Mutation testing: `just mutants` (cargo-mutants over `.cargo/mutants.toml` scope; nightly CI, informational)
- Format code: `just fmt`
- Check formatting without modifying (CI-style): `just fmt-check`
- Lint: `cargo clippy -- -D warnings`
- Upgrade dependencies: `just cargo-upgrade`
- Coverage: `just coverage` (summary) or `just coverage-html` (CI enforces an 80% line floor in coverage.yml)
- Clean coverage data: `just coverage-clean`

## Workflow
- After tests pass, run `cargo clippy --all-targets -- -D warnings` to catch any warnings (CI lints tests too)
- After validation and reaching a stopping point, run `cargo build --release` in the background
- When changing user-visible behavior, update `CHANGELOG.md` (Unreleased) in the same commit; call out any `CURRENT_SCHEMA_VERSION` or sync-protocol (`-v2`) change explicitly.

## Architecture Overview

### Core Components
- **`src/main.rs`**: CLI interface using clap with subcommands (Show, Sync, Import, Install, Recall, Scan, etc.)
- **`src/lib.rs`**: Core business logic including database operations, history parsing, shell integration, and the `helpers` and `test_utils` modules
- **`src/base_schema.sql`**: SQLite schema with `command_history` and `settings` tables, plus unique constraint preventing duplicates. Runs every connection (idempotent via `IF NOT EXISTS`); also sets up per-connection in-memory `memdb` tables. Schema migrations are version-tracked via `PRAGMA user_version` in `run_schema_migrations()` in `lib.rs`.
- **`build.rs`**: Generates secret-detection patterns (used by Scan/Scrub) at build time from `src/vendor/rules-stable.yml` (vendored from the secrets-patterns-db submodule; the vendored copy is git-tracked, so CI never checks submodules out), filtered by a curated `CRITICAL_PATTERN_NAMES` allowlist. `src/secrets_patterns.rs` is just an `include!` of the generated code -- edit `build.rs`, not it. Refresh vendored files with `just vendor-update` (requires `git submodule update --init --recursive`). Also emits `PXH_GIT_HASH` (short hash, `-dirty`, or `vX.Y.Z` outside git) shown by `--version` and `doctor --report`.
- **`src/doctor.rs`**: `pxh doctor` diagnostics and auto-fix (`--fix`) for installation/config issues
- **`src/config.rs`**: TOML config (`~/.config/pxh/config.toml`), strict (`deny_unknown_fields`); `DEFAULT_CONFIG` embeds the repo-root `config.toml` template; `config_status()` is the tri-state doctor uses
- **`src/ui.rs`**: diagnostic vocabulary (`warn`/`error`/`hint`, `count`); stdout is data, stderr goes through here
- **`src/shell_configs/`**: Shell integration scripts for bash (`pxh.bash`) and zsh (`pxh.zsh`) using preexec hooks
- **`src/recall/`**: Interactive TUI history search module
  - `mod.rs`: Module exports
  - `command.rs`: RecallCommand struct and FilterMode enum
  - `engine.rs`: SearchEngine with nucleo fuzzy matching, score-based ranking, HistoryEntry struct
  - `tui.rs`: Terminal UI using crossterm (drawing, key handling, vim/emacs modes)

### Command Structure
All commands follow the pattern `PxhArgs -> Commands enum -> XxxCommand struct`. Key commands:
- **Show/Search**: Query history with regex patterns, directory filters, session filters. Alias: `pxhs` (symlink/rename binary to invoke `pxh show` directly)
- **Recall**: Interactive TUI history search bound to Ctrl-R. Supports vim/emacs keymaps, preview pane, quick-select (Alt-1-9), and configurable via `~/.config/pxh/config.toml`. Uses nucleo fuzzy matching with `-` and `*` normalized to spaces (acting as word separators)
- **Sync**: Bidirectional sync via SSH or shared directories with optional `--since` filtering
- **Bootstrap**: Install pxh on a remote host over SSH via `install.sh` (this binary's version by default), probe `pxh --version` at the install path (proves the install) and again through the candidate paths `build_remote_pxh_command` uses (proves plain sync will pick it, not an older pxh earlier in that order), then run a first sync through the install path (`--no-sync` skips). Pure pieces (`install_command`, `remote_pxh_path`, `parse_probe_output`, `bootstrap_report`, `findability_report`) live in `pxh::helpers`
- **Insert/Seal**: Internal commands called by shell hooks to record command start/end. Insert, Seal, ShellConfig, and Autosuggest are `hide = true`: they stay callable but never appear in `pxh --help`
- **Import**: Bulk import from existing shell history files (bash, zsh, or JSON export)
- **Export**: Export full history as JSON
- **Scan**: Detect potential secrets in command history using built-in patterns
- **Scrub**: Remove sensitive commands from history (supports `--patterns-from-scan`, `--dir`, `--remote`)
- **Maintenance**: ANALYZE and VACUUM operations, cleans up non-standard tables/indexes
- **Doctor**: Diagnose and optionally fix (`--fix`) installation/config issues; `--json` for scripts, `--report` for bug reports; fixes are a `Fix` enum on `CheckResult`, never label matching
- **Autosuggest**: Internal command backing the zsh-autosuggestions strategy in `pxh.zsh`
- **Mangen**: hidden; `pxh mangen <dir>` / `just man` for packagers

### Database Design
- SQLite database at `~/.local/share/pxh/pxh.db` by default (configurable via `--db` or `PXH_DB_PATH`)
- `command_history` table stores commands as BLOBs to handle non-UTF8 data
- `settings` table stores key-value pairs (e.g., `original_hostname` for sync identification)
- Unique index prevents duplicates based on command + timestamp + shellname + COALESCE'd context fields
- Uses WAL journal mode, MEMORY temp store, and busy timeout for concurrent access
- In-memory `memdb.show_results` table for efficient query result handling

### Sync Architecture
Two sync modes:
1. **Directory sync**: Merges all `.db` files in a shared directory (Dropbox, etc.)
2. **Remote sync**: Direct SSH connection with stdin/stdout protocol for real-time sync

The sync implementation uses `create_filtered_db_copy()` to handle `--since` filtering and `merge_database_from_file()` for deduplication via `INSERT OR IGNORE`.

## Code Style Guidelines
- **Imports**: Group by Std, External, Crate
- **Formatting**: `cargo fmt` (via `just fmt`), config in rustfmt.toml
- **Naming**: Command structs end with "Command" (e.g., `ShowCommand`)
- **Error Handling**: Library and command code return `Result<T, Box<dyn std::error::Error>>` with `?`. Add context where a bare OS error would be unhelpful with `anyhow::Context::with_context(...)` (it converts through `?`). `main()` returns `()` and prints the whole `source()` chain via `pxh::ui::error(&error_chain(&*e))` -- never `-> Result` on `main()`.
- **Diagnostics**: everything printed for a human that is not data goes through `pxh::ui::{warn, error, hint}` (lowercase `warning:` / `error:` / `hint:` on stderr; `anstream` honors `NO_COLOR`). Counted nouns use `ui::count(n, "secret")`, never `secret(s)`.
- **Types**:
  - `BString` from bstr for binary strings/non-UTF8 data
  - `uzers` crate for user information (security-updated fork of `users`)

## Testing Guidelines

### Test Structure
- **`tests/integration_tests.rs`**: End-to-end command testing using shell history import/export
- **`tests/sync_test.rs`**: Comprehensive sync functionality tests (directory, remote SSH, stdin/stdout)
- **`tests/ssh_sync_test.rs`**: SSH-specific sync testing
- **`tests/bootstrap_test.rs`**: `pxh bootstrap` end to end: a scripted `ssh` runs the remote command string in a second temp `HOME`, a scripted `curl` serves the repo's real `install.sh` and a fake release tarball built from the binary under test; no network
- **`tests/recall_test.rs`**: Interactive TUI recall functionality tests
- **`tests/scan_test.rs`**: Secret scanning and pattern detection tests
- **`tests/unit.rs`**: Unit tests for core functionality
- **`tests/interactive_shell_test.rs`**: Interactive shell session testing with rexpect
- **`tests/shell_integration_simple_test.rs`**: Simple shell integration tests
- **`tests/shell_hooks_test.rs`**: Shell hook (preexec/precmd) testing
- **`tests/doctor_test.rs`**: Doctor command diagnostics tests
- **`tests/perf_test.rs`**: Recall-latency guard, `#[ignore]`d by default. Times the hot paths (recall load in each mode, TUI init, insert, seal, autosuggest) against 50k- and 500k-row databases and fails if any scales with table size. Run with `just perf` (release build); CI runs it in the `perf` job.
- **`tests/property_test.rs`**: proptest properties for byte-level paths (zsh unmetafy, continuation-line joining, JSON import round trip of arbitrary bytes)
- **`tests/docker/`**: Clean-machine user journey on stock Debian: fresh HOME, `pxh install`, commands typed into real interactive bash and zsh via `script(1)`, then search/import/export/scan/scrub/sync (run via `just docker-e2e`)
- **`tests/cli_errors_test.rs`**: error/warning output contract (`error: <chain>` on stderr, lowercase prefixes, hook-path commands stay quiet on a bad config)
- **`tests/docs_drift_test.rs`**: README TOML examples must parse as the strict `Config`; `main.rs` tests pin that every non-hidden subcommand is named in the README and run clap's `debug_assert`.
- **`tests/common/mod.rs`**: Shared test utilities (re-exports `pxh::test_utils`)
- **`tests/resources/`**: Sample histfiles for import testing (bash simple/timestamped, zsh incl. malformed/multiline)

### Test Conventions
- **No sleeps.** Interactive tests synchronise on a sentinel prompt (`ShellSession` in `interactive_shell_test.rs`): the rc file pins `PS1`/`PROMPT`, and `run()` waits for it, which proves the synchronous hooks finished. When a recall selection is involved, wait for a marker only *execution* can print (e.g. `echo pxh-ran-$((6*7))` -> `pxh-ran-42`), never for the command text, which the TUI and readline also echo.
- **Missing tools fail, never skip.** bash, zsh, and `sqlite3` are required; a silent early return is a green test that tested nothing.
- **No network.** SSH sync tests pass a stub script via `--ssh-cmd` that records argv.
- **Pin complexity with `EXPLAIN QUERY PLAN`.** Hot-path SQL lives in named consts/builders (`SEAL_SQL`, `AUTOSUGGEST_SQL`, `SearchEngine::recall_query`) so plan tests exercise the exact production SQL via `test_utils::explain_query_plan`. Recall/autosuggest must walk `history_start_time` with no `TEMP B-TREE`; seal must be a covering-index seek.
- **Readline needs echo.** rexpect forks ptys with echo off and GNU readline skips redisplay on a no-echo terminal; shell rc preludes run `stty echo`.
- `PxhTestHelper` seeds `~/.pxh/config.toml` with `ignore_patterns = []`, so trivial commands (`false`, `cd`, ...) are recorded in tests but not by default.
- **Retries hide flakes.** The default nextest profile retries the pty suites twice; when hunting a flake use `just stress` / `--profile stress`.

### Test Helpers
Located in `pxh::test_utils` (src/lib.rs) and `tests/common/mod.rs`:

- **`PxhTestHelper`**: Primary test helper providing isolated test environment with:
  - Temporary directory and database path
  - Randomized hostname for isolation
  - `command()` / `command_with_args()` for pxh invocation
  - `shell_command()` for interactive shell testing
  - Coverage environment variable propagation
- **`pxh_command()`**: bare `pxh` process with coverage env propagated (for tests that build their own environment)
- **`pxh_path()`**: Resolves path to built pxh binary
- **`insert_test_command(db_path, command, days_ago)`**: Creates test commands using pxh binary
- **`count_commands(db_path)`**: Direct SQLite query for command counting
- **`spawn_sync_processes()`**: Sets up cross-connected processes for stdin/stdout sync testing

### Testing Sync
Use stdin/stdout mode with `--stdin-stdout` flag for testing sync without SSH overhead. The `spawn_sync_processes()` helper creates bidirectionally connected pxh processes.

### Testing TUI Components
Manual runs go through the `verify` skill (`.claude/skills/verify/SKILL.md`): isolated `HOME`/`PXH_DB_PATH`, never the real database.

For testing interactive TUI components (like `pxh recall`), use tmux to capture and validate screen output.

**Important:** When interacting with tmux panes, ALWAYS use `tmux-cli send` instead of plain `tmux send-keys`. Plain tmux commands are unreliable because they send text and Enter simultaneously without any delay, causing race conditions where the Enter key is lost before the target application can process the text input.

Use the `tmux-cli` skill for TUI validation workflows.

## Key Implementation Details

### Shell Integration
Uses preexec/precmd hooks to capture command start/end. The `bash-preexec` library (vendored at `src/vendor/bash-preexec.sh`) provides bash compatibility with zsh-style hooks. Shell configs are embedded via `include_str!` and output via the `shell-config` command.

### Binary Data Handling
Commands are stored as BLOBs to handle arbitrary shell data. Use `BString` from the bstr crate for binary string operations.

### Helper Modules
- **`pxh::helpers`**: Utilities for SSH command parsing, remote path resolution, and `pxhs` alias detection
- **`pxh::test_utils`**: Test infrastructure (`PxhTestHelper`) for isolated test environments

### Performance Considerations
- SQLite with bundled feature for consistency
- WAL journal mode and busy timeout (5s) for concurrent access
- Prepared statements for repeated queries
- Unique indexes for deduplication performance
- In-memory temporary tables for complex queries (`memdb.show_results`)
- Custom REGEXP function using bytes regex for non-UTF8 support
- VACUUM operations in maintenance for space reclamation

### Sync Protocol
Remote sync uses a simple binary protocol over stdin/stdout:
1. Client sends mode string ("send", "receive", or "bidirectional") followed by newline
2. Send database size as 8-byte little-endian u64
3. Stream database contents
4. Bidirectional exchange for full sync
5. `INSERT OR IGNORE` with ATTACH DATABASE for deduplication

### Configuration
pxh supports a TOML configuration file at `~/.config/pxh/config.toml`:
- **`[host]`** section: `hostname`, `machine_id`, `aliases` for sync identity
- **`[shell]`** section: `disable_ctrl_r` to skip the Ctrl-R binding
- **`[history]`** section: `ignore_patterns` (regexes for commands to skip recording)
- **`[recall]`** section:
  - `keymap`: "emacs" (default) or "vim"
  - `show_preview`: boolean to show/hide preview pane
  - `result_limit`: max number of results to load (default: 5000)
- **`[recall.preview]`** section:
  - `show_directory`, `show_timestamp`, `show_exit_status`, `show_duration`, `show_hostname`: booleans to control preview pane fields

The config is loaded via `Config::load()` in `src/config.rs` with sensible defaults if the file doesn't exist. Unknown keys are rejected (the file is ignored with a warning until fixed); the commented template at the repo root must stay equal to `Config::default()` (unit test `template_parses_to_defaults`).