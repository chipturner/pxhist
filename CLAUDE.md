# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## pxh - Portable Shell History Manager

pxh is a fast, cross-shell history mining tool that uses SQLite to provide powerful search capabilities across shell command history. It supports bash and zsh, tracks rich metadata (directory, host, user, exit codes, durations), and provides bidirectional synchronization across machines.

## Build Commands
- Build: `cargo build` or `cargo build --release`
- Run tests: `cargo test`
- Run single test: `cargo test test_name`
- Run integration tests: `cargo test --test integration_tests`
- Run specific test file: `cargo test --test sync_test`
- Format code: `just fmt`
- Lint: `cargo clippy -- -D warnings`
- Upgrade dependencies: `just cargo-upgrade`
- Coverage: `just coverage` or `just coverage-detailed`

## Workflow
- After tests pass, run `cargo clippy -- -D warnings` to catch any warnings
- After validation and reaching a stopping point, run `cargo build --release` in the background

## Architecture Overview

### Core Components
- **`src/main.rs`**: CLI interface using clap with subcommands (Show, Sync, Import, Install, etc.)
- **`src/lib.rs`**: Core business logic including database operations, history parsing, shell integration, and the `helpers` and `test_utils` modules
- **`src/base_schema.sql`**: SQLite schema with `command_history` and `settings` tables, plus unique constraint preventing duplicates
- **`src/schema_migration.sql`**: Migration script for schema changes (deduplication with COALESCE-based unique index)
- **`src/shell_configs/`**: Shell integration scripts for bash (`pxh.bash`) and zsh (`pxh.zsh`) using preexec hooks

### Command Structure
All commands follow the pattern `PxhArgs -> Commands enum -> XxxCommand struct`. Key commands:
- **Show/Search**: Query history with regex patterns, directory filters, session filters. Alias: `pxhs` (symlink/rename binary to invoke `pxh show` directly)
- **Sync**: Bidirectional sync via SSH or shared directories with optional `--since` filtering
- **Insert/Seal**: Internal commands called by shell hooks to record command start/end
- **Import**: Bulk import from existing shell history files (bash, zsh, or JSON export)
- **Export**: Export full history as JSON
- **Scrub**: Remove sensitive commands from history
- **Maintenance**: ANALYZE and VACUUM operations, cleans up non-standard tables/indexes

### Database Design
- SQLite database at `~/.pxh/pxh.db` by default (configurable via `--db` or `PXH_DB_PATH`)
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
- **Naming**:
  - `snake_case` for variables, functions, methods
  - `CamelCase` for types, structs, enums
  - Command structs end with "Command" (e.g., `ShowCommand`)
- **Error Handling**: Use `Result<T, Box<dyn std::error::Error>>` with `?` operator
- **Types**:
  - `BString` from bstr for binary strings/non-UTF8 data
  - `PathBuf` and `Path` for file paths
  - `Option<T>` for values that might not exist
  - `uzers` crate for user information (security-updated fork of `users`)

## Testing Guidelines

### Test Structure
- **`tests/integration_tests.rs`**: End-to-end command testing using shell history import/export
- **`tests/sync_test.rs`**: Comprehensive sync functionality tests (directory, remote SSH, stdin/stdout)
- **`tests/ssh_sync_test.rs`**: SSH-specific sync testing
- **`tests/unit.rs`**: Unit tests for core functionality
- **`tests/interactive_shell_test.rs`**: Interactive shell session testing with rexpect
- **`tests/shell_integration_simple_test.rs`**: Simple shell integration tests
- **`tests/shell_hooks_test.rs`**: Shell hook (preexec/precmd) testing
- **`tests/common/mod.rs`**: Shared test utilities and compatibility wrappers
- **`tests/resources/`**: Sample histfiles for import testing (bash simple/timestamped, zsh)

### Test Helpers
Located in `pxh::test_utils` (src/lib.rs) and `tests/common/mod.rs`:

- **`PxhTestHelper`**: Primary test helper providing isolated test environment with:
  - Temporary directory and database path
  - Randomized hostname for isolation
  - `command()` / `command_with_args()` for pxh invocation
  - `shell_command()` for interactive shell testing
  - Coverage environment variable propagation
- **`PxhCaller`**: Legacy compatibility wrapper around `PxhTestHelper`
- **`pxh_path()`**: Resolves path to built pxh binary
- **`insert_test_command(db_path, command, days_ago)`**: Creates test commands using pxh binary
- **`count_commands(db_path)`**: Direct SQLite query for command counting
- **`spawn_sync_processes()`**: Sets up cross-connected processes for stdin/stdout sync testing

### Testing Sync
Use stdin/stdout mode with `--stdin-stdout` flag for testing sync without SSH overhead. The `spawn_sync_processes()` helper creates bidirectionally connected pxh processes.

### Testing TUI Components
For testing interactive TUI components (like `pxh recall`), use tmux to capture and validate screen output.

**Important:** When interacting with tmux panes, ALWAYS use `tmux-cli send` instead of plain `tmux send-keys`. Plain tmux commands are unreliable because they send text and Enter simultaneously without any delay, causing race conditions where the Enter key is lost before the target application can process the text input.

Use the `tmux-cli` skill for TUI validation workflows.

## Key Implementation Details

### Shell Integration
Uses preexec/precmd hooks to capture command start/end. The `bash-preexec` library (bundled in `src/shell_configs/bash-preexec/`) provides bash compatibility with zsh-style hooks. Shell configs are embedded via `include_str!` and output via the `shell-config` command.

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