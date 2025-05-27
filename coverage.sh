#!/bin/bash

set -e

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}Setting up code coverage for pxhist...${NC}"

# Check if cargo-llvm-cov is installed
if ! command -v cargo-llvm-cov &> /dev/null; then
    echo -e "${YELLOW}Installing cargo-llvm-cov...${NC}"
    cargo install cargo-llvm-cov
fi

# Clean previous coverage data
echo -e "${GREEN}Cleaning previous coverage data...${NC}"
cargo llvm-cov clean --workspace

# Run tests with coverage instrumentation
# --include-ffi enables coverage for subprocess tests
echo -e "${GREEN}Running tests with coverage instrumentation...${NC}"
RUST_LOG=debug cargo llvm-cov test \
    --all-features \
    --workspace \
    --include-ffi \
    --lcov \
    --output-path lcov.info

# Generate HTML report
echo -e "${GREEN}Generating HTML coverage report...${NC}"
cargo llvm-cov report \
    --html \
    --output-dir coverage

# Display summary
echo -e "${GREEN}Coverage summary:${NC}"
cargo llvm-cov report

echo -e "${GREEN}HTML report generated in ./coverage/html/index.html${NC}"

# Open the report in browser if on macOS
if [[ "$OSTYPE" == "darwin"* ]]; then
    echo -e "${GREEN}Opening coverage report in browser...${NC}"
    open coverage/html/index.html
fi