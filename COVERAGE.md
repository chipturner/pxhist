# Code Coverage for pxhist

This project uses `cargo-llvm-cov` for code coverage, which properly handles integration tests that shell out to the pxh executable.

## Quick Start

```bash
# Run basic coverage
just coverage

# Run detailed coverage with multiple report formats
just coverage-detailed

# Clean coverage artifacts
just coverage-clean
```

## Coverage Scripts

- `coverage.sh` - Basic coverage with HTML report
- `coverage-detailed.sh` - Detailed coverage with multiple formats (HTML, LCOV, JSON, Cobertura)

## Key Features

1. **Subprocess Coverage**: The `--include-ffi` flag ensures coverage data is collected from the pxh binary when integration tests shell out to it.

2. **Multiple Report Formats**:
   - HTML report with source code highlighting at `./coverage/html/index.html`
   - LCOV format for IDE integration at `./lcov.info`
   - JSON format for programmatic access at `./coverage.json`
   - Cobertura XML for CI tools at `./coverage.xml`

3. **CI Integration**: GitHub Actions workflow automatically runs coverage on push/PR and uploads to Codecov.

## Current Coverage

As of the last run:
- Overall coverage: ~55%
- lib.rs: 62% line coverage
- main.rs: 51% line coverage

## Improving Coverage

To improve coverage:
1. Add more unit tests for uncovered functions
2. Add integration tests for untested command combinations
3. Test error paths and edge cases

## Troubleshooting

If you see "mismatched data" warnings, this is normal when mixing unit and integration tests. The coverage data is still accurate.

To ensure maximum coverage collection:
- Build with `cargo llvm-cov build` before running tests
- Use `--include-ffi` flag for integration tests
- Set `LLVM_PROFILE_FILE` environment variable for subprocess coverage