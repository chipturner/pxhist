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
- Format code: `just rustfmt`
- Lint: `cargo clippy`
- Upgrade dependencies: `just cargo-upgrade`

## Architecture Overview

### Core Components
- **`src/main.rs`**: CLI interface using clap with subcommands (Show, Sync, Import, Install, etc.)
- **`src/lib.rs`**: Core business logic including database operations, history parsing, and shell integration
- **`src/base_schema.sql`**: SQLite schema with `command_history` table and unique constraint preventing duplicates
- **`src/shell_configs/`**: Shell integration scripts for bash and zsh using preexec hooks

### Command Structure
All commands follow the pattern `PxhArgs -> Commands enum -> XxxCommand struct`. Key commands:
- **Show/Search**: Query history with regex patterns, directory filters, session filters
- **Sync**: Bidirectional sync via SSH or shared directories with optional `--since` filtering
- **Insert/Seal**: Internal commands called by shell hooks to record command start/end
- **Import**: Bulk import from existing shell history files

### Database Design
- SQLite database at `~/.pxh/pxh.db` by default
- `command_history` table stores commands as BLOBs to handle non-UTF8 data
- Unique index prevents duplicates based on command + timestamp + context
- Uses transactions and prepared statements for performance and consistency

### Sync Architecture
Two sync modes:
1. **Directory sync**: Merges all `.db` files in a shared directory (Dropbox, etc.)
2. **Remote sync**: Direct SSH connection with stdin/stdout protocol for real-time sync

The sync implementation uses `create_filtered_db_copy()` to handle `--since` filtering and `merge_database_from_file()` for deduplication via `INSERT OR IGNORE`.

## Code Style Guidelines
- **Imports**: Group by Std, External, Crate using `imports_granularity=Crate`
- **Formatting**: 4-space indentation, rustfmt with edition=2024
- **Naming**: 
  - `snake_case` for variables, functions, methods
  - `CamelCase` for types, structs, enums
  - Command structs end with "Command" (e.g., `ShowCommand`)
- **Error Handling**: Use `Result<T, Box<dyn std::error::Error>>` with `?` operator
- **Types**:
  - `BString` from bstr for binary strings/non-UTF8 data
  - `PathBuf` and `Path` for file paths
  - `Option<T>` for values that might not exist
- **Documentation**: Document complex logic with detailed comments

## Testing Guidelines

### Test Structure
- **`tests/integration_tests.rs`**: End-to-end command testing using shell history import/export
- **`tests/sync_test.rs`**: Comprehensive sync functionality tests (directory, remote SSH, stdin/stdout)
- **`tests/ssh_sync_test.rs`**: SSH-specific sync testing
- **`tests/unit.rs`**: Unit tests for core functionality
- **`tests/common/mod.rs`**: Shared test utilities (pxh binary path resolution)

### Test Helpers
- `insert_test_command(db_path, command, days_ago)`: Creates test commands using pxh binary
- `create_test_db_with_commands()`: Creates database with multiple commands
- `create_test_db_pair()`: Creates client/server database pairs for sync testing
- `spawn_sync_processes()`: Sets up cross-connected processes for stdin/stdout sync testing
- `count_commands()`: Direct SQLite query for command counting

### Testing Sync
Use stdin/stdout mode with `--stdin-stdout` flag for testing sync without SSH overhead. The `spawn_sync_processes()` helper creates bidirectionally connected pxh processes.

## Key Implementation Details

### Shell Integration
Uses preexec/precmd hooks to capture command start/end. The `bash-preexec` library provides bash compatibility with zsh-style hooks.

### Binary Data Handling
Commands are stored as BLOBs to handle arbitrary shell data. Use `BString` from the bstr crate for binary string operations.

### Performance Considerations
- SQLite with bundled feature for consistency
- Prepared statements for repeated queries
- Unique indexes for deduplication performance
- In-memory temporary tables for complex queries
- VACUUM operations in maintenance for space reclamation

### Sync Protocol
Remote sync uses a simple binary protocol over stdin/stdout:
1. Send database size as little-endian bytes
2. Stream database contents
3. Bidirectional exchange for full sync
4. `INSERT OR IGNORE` for deduplication