#!/bin/bash
set -eou pipefail

passed=0
failed=0

step() {
    echo "=== $1 ==="
}

check() {
    local desc="$1"
    shift
    if "$@" >/dev/null 2>&1; then
        echo "  PASS: ${desc}"
        passed=$((passed + 1))
    else
        echo "  FAIL: ${desc}"
        failed=$((failed + 1))
    fi
}

check_output() {
    local desc="$1"
    local expected="$2"
    shift 2
    local output
    output=$("$@" 2>&1) || true
    if echo "${output}" | grep -q "${expected}"; then
        echo "  PASS: ${desc}"
        passed=$((passed + 1))
    else
        echo "  FAIL: ${desc} (expected '${expected}' in output)"
        echo "  Got: ${output}"
        failed=$((failed + 1))
    fi
}

# Setup
export HOME=/root
DB_DIR="${HOME}/.local/share/pxh"
DB_PATH="${DB_DIR}/pxh.db"
mkdir -p "${DB_DIR}"
export PXH_DB_PATH="${DB_PATH}"

# -- 1. Install -----------------------------------------------------------
step "Install"
touch "${HOME}/.bashrc" "${HOME}/.zshrc"
check "install bash" pxh install bash
check "install zsh" pxh install zsh
check ".bashrc contains pxh" grep -q "pxh shell-config" "${HOME}/.bashrc"
check ".zshrc contains pxh" grep -q "pxh shell-config" "${HOME}/.zshrc"
check "install bash idempotent" pxh install bash

# -- 2. Record commands (bash hooks) --------------------------------------
step "Record commands (bash)"
export PXH_SESSION_ID=100
export PXH_HOSTNAME=e2e-host
ts=$(date +%s)
pxh --db "${PXH_DB_PATH}" insert \
    --working-directory /tmp \
    --hostname "${PXH_HOSTNAME}" --shellname bash \
    --username root --session-id "${PXH_SESSION_ID}" \
    --start-unix-timestamp "${ts}" "ls -la"

pxh --db "${PXH_DB_PATH}" seal \
    --session-id "${PXH_SESSION_ID}" \
    --end-unix-timestamp "$((ts + 1))" --exit-status 0

pxh --db "${PXH_DB_PATH}" insert \
    --working-directory /home \
    --hostname "${PXH_HOSTNAME}" --shellname bash \
    --username root --session-id "${PXH_SESSION_ID}" \
    --start-unix-timestamp "$((ts + 2))" "cd /home"

pxh --db "${PXH_DB_PATH}" seal \
    --session-id "${PXH_SESSION_ID}" \
    --end-unix-timestamp "$((ts + 3))" --exit-status 0

pxh --db "${PXH_DB_PATH}" insert \
    --working-directory /tmp \
    --hostname "${PXH_HOSTNAME}" --shellname bash \
    --username root --session-id "${PXH_SESSION_ID}" \
    --start-unix-timestamp "$((ts + 4))" "exit 1"

pxh --db "${PXH_DB_PATH}" seal \
    --session-id "${PXH_SESSION_ID}" \
    --end-unix-timestamp "$((ts + 5))" --exit-status 1

echo "  PASS: recorded 3 bash commands"
passed=$((passed + 1))

# -- 3. Record commands (zsh hooks) ---------------------------------------
step "Record commands (zsh)"
export PXH_SESSION_ID=200
pxh --db "${PXH_DB_PATH}" insert \
    --working-directory /var \
    --hostname "${PXH_HOSTNAME}" --shellname zsh \
    --username root --session-id "${PXH_SESSION_ID}" \
    --start-unix-timestamp "$((ts + 10))" "echo zsh-test"

pxh --db "${PXH_DB_PATH}" seal \
    --session-id "${PXH_SESSION_ID}" \
    --end-unix-timestamp "$((ts + 11))" --exit-status 0

echo "  PASS: recorded 1 zsh command"
passed=$((passed + 1))

# -- 4. Show / Search -----------------------------------------------------
step "Show / Search"
check_output "show lists commands" "ls -la" pxh --db "${PXH_DB_PATH}" show
check_output "show with pattern" "ls -la" pxh --db "${PXH_DB_PATH}" show "ls"
check_output "show --here (from /tmp)" "ls -la" \
    env PWD=/tmp pxh --db "${PXH_DB_PATH}" show --here
check_output "show --failed" "exit 1" pxh --db "${PXH_DB_PATH}" show --failed
check_output "show --limit 1" "zsh-test" pxh --db "${PXH_DB_PATH}" show --limit 1

# -- 5. Import ------------------------------------------------------------
step "Import"
histfile=$(mktemp)
echo "imported-cmd-1" > "${histfile}"
echo "imported-cmd-2" >> "${histfile}"
check "import bash histfile" pxh --db "${PXH_DB_PATH}" import --shellname bash --histfile "${histfile}"
check_output "imported commands visible" "imported-cmd-1" pxh --db "${PXH_DB_PATH}" show
rm "${histfile}"

# -- 6. Export -------------------------------------------------------------
step "Export"
export_file=$(mktemp)
pxh --db "${PXH_DB_PATH}" export > "${export_file}"
check_output "export is valid JSON" "command" cat "${export_file}"
rm "${export_file}"

# -- 7. Stats --------------------------------------------------------------
step "Stats"
check_output "stats shows count" "Commands:" pxh --db "${PXH_DB_PATH}" stats

# -- 8. Scan ---------------------------------------------------------------
step "Scan"
pxh --db "${PXH_DB_PATH}" insert \
    --working-directory /tmp \
    --hostname "${PXH_HOSTNAME}" --shellname bash \
    --username root --session-id "${PXH_SESSION_ID}" \
    --start-unix-timestamp "$((ts + 20))" \
    "curl -H 'Authorization: Bearer AKIAIOSFODNN7EXAMPLE'"

pxh --db "${PXH_DB_PATH}" seal \
    --session-id "${PXH_SESSION_ID}" \
    --end-unix-timestamp "$((ts + 21))" --exit-status 0

check_output "scan detects secret" "AKIA" pxh --db "${PXH_DB_PATH}" scan

# -- 9. Scrub --------------------------------------------------------------
step "Scrub"
before_count=$(sqlite3 "${PXH_DB_PATH}" "SELECT count(*) FROM command_history")
check "scrub with scan patterns" pxh --db "${PXH_DB_PATH}" scrub --scan --yes
after_count=$(sqlite3 "${PXH_DB_PATH}" "SELECT count(*) FROM command_history")
if [ "${after_count}" -lt "${before_count}" ]; then
    echo "  PASS: scrub removed commands (${before_count} -> ${after_count})"
    passed=$((passed + 1))
else
    echo "  FAIL: scrub did not remove commands (${before_count} -> ${after_count})"
    failed=$((failed + 1))
fi

# -- 10. Maintenance -------------------------------------------------------
step "Maintenance"
check "maintenance runs" pxh --db "${PXH_DB_PATH}" maintenance

# -- 11. Sync (directory mode) ---------------------------------------------
step "Sync (directory mode)"
sync_dir=$(mktemp -d)
db2="${sync_dir}/other.db"
pxh --db "${db2}" insert \
    --working-directory /opt \
    --hostname other-host --shellname bash \
    --username root --session-id 300 \
    --start-unix-timestamp "$((ts + 30))" "remote-only-cmd"
pxh --db "${db2}" seal \
    --session-id 300 \
    --end-unix-timestamp "$((ts + 31))" --exit-status 0

# Copy main db to sync dir for merging
cp "${PXH_DB_PATH}" "${sync_dir}/main.db"
check "sync directory mode" pxh --db "${PXH_DB_PATH}" sync "${sync_dir}"
check_output "synced command visible" "remote-only-cmd" pxh --db "${PXH_DB_PATH}" show
rm -rf "${sync_dir}"

# -- Summary ---------------------------------------------------------------
echo ""
echo "================================"
echo "Results: ${passed} passed, ${failed} failed"
echo "================================"

if [ "${failed}" -gt 0 ]; then
    exit 1
fi
