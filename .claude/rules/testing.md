---
paths:
  - "tests/**"
---

# Testing

## Test Structure
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

## Test Conventions
- **No sleeps.** Interactive tests synchronise on a sentinel prompt (`ShellSession` in `interactive_shell_test.rs`): the rc file pins `PS1`/`PROMPT`, and `run()` waits for it, which proves the synchronous hooks finished. When a recall selection is involved, wait for a marker only *execution* can print (e.g. `echo pxh-ran-$((6*7))` -> `pxh-ran-42`), never for the command text, which the TUI and readline also echo.
- **Missing tools fail, never skip.** bash, zsh, and `sqlite3` are required; a silent early return is a green test that tested nothing.
- **No network.** SSH sync tests pass a stub script via `--ssh-cmd` that records argv.
- **Pin complexity with `EXPLAIN QUERY PLAN`.** Hot-path SQL lives in named consts/builders (`SEAL_SQL`, `AUTOSUGGEST_SQL`, `SearchEngine::recall_query`) so plan tests exercise the exact production SQL via `test_utils::explain_query_plan`. Recall/autosuggest must walk `history_start_time` with no `TEMP B-TREE`; seal must be a covering-index seek.
- **Readline needs echo.** rexpect forks ptys with echo off and GNU readline skips redisplay on a no-echo terminal; shell rc preludes run `stty echo`.
- `PxhTestHelper` seeds `~/.pxh/config.toml` with `ignore_patterns = []`, so trivial commands (`false`, `cd`, ...) are recorded in tests but not by default.
- **Retries hide flakes.** The default nextest profile retries the pty suites twice; when hunting a flake use `just stress` / `--profile stress`.

## Test Helpers
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

## Testing Sync
Use stdin/stdout mode with `--stdin-stdout` flag for testing sync without SSH overhead. The `spawn_sync_processes()` helper creates bidirectionally connected pxh processes.

## Testing TUI Components
Manual runs go through the `verify` skill (`.claude/skills/verify/SKILL.md`): isolated `HOME`/`PXH_DB_PATH`, never the real database.

For testing interactive TUI components (like `pxh recall`), use tmux to capture and validate screen output.

**Important:** When interacting with tmux panes, ALWAYS use `tmux-cli send` instead of plain `tmux send-keys`. Plain tmux commands are unreliable because they send text and Enter simultaneously without any delay, causing race conditions where the Enter key is lost before the target application can process the text input.

Use the `tmux-cli` skill for TUI validation workflows.
