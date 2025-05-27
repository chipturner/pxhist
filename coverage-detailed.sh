#!/bin/bash

set -e

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
RED='\033[0;31m'
NC='\033[0m' # No Color

echo -e "${GREEN}=== Advanced Code Coverage for pxhist ===${NC}"
echo

# Check if cargo-llvm-cov is installed
if ! command -v cargo-llvm-cov &> /dev/null; then
    echo -e "${YELLOW}Installing cargo-llvm-cov...${NC}"
    cargo install cargo-llvm-cov
fi

# Clean previous coverage data
echo -e "${BLUE}Cleaning previous coverage data...${NC}"
cargo llvm-cov clean --workspace
rm -rf coverage/
rm -f lcov.info *.profraw *.profdata

# Set up environment for subprocess coverage
export CARGO_BUILD_TARGET_DIR="target/llvm-cov-target"
export LLVM_PROFILE_FILE="pxhist-%p-%m.profraw"
export RUST_LOG=debug

# Run all tests with coverage instrumentation
echo -e "${BLUE}Running all tests with coverage instrumentation...${NC}"
cargo llvm-cov test \
    --all-features \
    --workspace \
    --include-ffi \
    --no-report

# Generate detailed reports
echo -e "${BLUE}Generating coverage reports...${NC}"

# Generate LCOV report
cargo llvm-cov report --lcov --output-path lcov.info

# Generate HTML report with source code
cargo llvm-cov report --html --output-dir coverage

# Generate JSON report for CI integration
cargo llvm-cov report --json --output-path coverage.json

# Generate Cobertura XML for GitLab/Jenkins
cargo llvm-cov report --cobertura --output-path coverage.xml

# Display detailed summary
echo
echo -e "${GREEN}=== Coverage Summary ===${NC}"
cargo llvm-cov report --color always

# Show uncovered functions
echo
echo -e "${YELLOW}=== Functions with No Coverage ===${NC}"
cargo llvm-cov report 2>/dev/null | grep -E "0.00%" | grep -v "TOTAL" | head -10 || echo "All functions have some coverage!"

# Show files sorted by coverage
echo
echo -e "${YELLOW}=== Files by Coverage (lowest first) ===${NC}"
cargo llvm-cov report 2>/dev/null | grep -E "\.rs\s+" | sort -k6 -n | head -5

# Calculate coverage percentage (check if jq is available)
if command -v jq &> /dev/null; then
    COVERAGE=$(cargo llvm-cov report --json --summary-only | jq -r '.data[0].totals.lines.percent // 0')
else
    # Fallback: extract percentage from text output
    COVERAGE=$(cargo llvm-cov report | grep "TOTAL" | awk '{print $6}' | sed 's/%//')
fi
COVERAGE_INT=${COVERAGE%.*}

echo
echo -e "${GREEN}Overall Line Coverage: ${COVERAGE}%${NC}"

# Coverage threshold check
THRESHOLD=50
if [ "$COVERAGE_INT" -lt "$THRESHOLD" ]; then
    echo -e "${RED}⚠️  Coverage is below ${THRESHOLD}% threshold!${NC}"
else
    echo -e "${GREEN}✓ Coverage meets ${THRESHOLD}% threshold${NC}"
fi

echo
echo -e "${GREEN}Reports generated:${NC}"
echo "  - HTML:      ./coverage/html/index.html"
echo "  - LCOV:      ./lcov.info"
echo "  - JSON:      ./coverage.json"
echo "  - Cobertura: ./coverage.xml"

# Open the report in browser if on macOS
if [[ "$OSTYPE" == "darwin"* ]]; then
    echo
    echo -e "${GREEN}Opening coverage report in browser...${NC}"
    open coverage/html/index.html
fi