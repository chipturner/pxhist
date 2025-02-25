# CLAUDE.md - pxh Development Guide

## Build Commands
- Build: `cargo build` or `cargo build --release`
- Run tests: `cargo test`
- Run single test: `cargo test test_name`
- Run integration tests: `cargo test --test integration_tests`
- Format code: `just rustfmt`
- Lint: `cargo clippy`

## Code Style Guidelines
- **Imports**: Group by Std, External, Crate using `imports_granularity=Crate`
- **Formatting**: 4-space indentation, rustfmt with edition=2021
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