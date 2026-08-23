#!/bin/bash
# Clean-machine user journey: a fresh HOME, `pxh install`, then commands typed
# into real interactive bash and zsh sessions (driven through script(1) so the
# shells have a pty and their preexec/precmd hooks fire exactly as for a user).
set -eou pipefail

passed=0
failed=0

step() { echo "=== $1 ==="; }

pass() { echo "  PASS: $1"; passed=$((passed + 1)); }
fail() { echo "  FAIL: $1"; failed=$((failed + 1)); }

check() {
    local desc="$1"; shift
    if "$@" >/dev/null 2>&1; then pass "$desc"; else fail "$desc"; fi
}

check_output() {
    local desc="$1" expected="$2"; shift 2
    local output
    output=$("$@" 2>&1) || true
    if echo "${output}" | grep -q -- "${expected}"; then
        pass "$desc"
    else
        fail "$desc (expected '${expected}' in output)"
        echo "  Got: ${output}"
    fi
}

# Type lines into an interactive shell that has a pty.
drive_shell() {
    local shell="$1"; shift
    printf '%s\n' "$@" 'exit' | script -qec "${shell} -i" /dev/null >/dev/null 2>&1 || true
}

# Fresh HOME, as a new user would have. Do NOT set PXH_DB_PATH: the shell
# hooks must discover the default location themselves.
export HOME=/root
rm -rf "${HOME}/.local/share/pxh" "${HOME}/.config/pxh" "${HOME}/.pxh"
touch "${HOME}/.bashrc" "${HOME}/.zshrc"
export SHELL=/bin/bash
DB="${HOME}/.local/share/pxh/pxh.db"

# -- 1. Install ---------------------------------------------------------------
step "Install"
check "install bash" pxh install bash
check "install zsh" pxh install zsh
check ".bashrc sources shell-config" grep -q "pxh shell-config bash" "${HOME}/.bashrc"
check ".zshrc sources shell-config" grep -q "pxh shell-config zsh" "${HOME}/.zshrc"
pxh install bash >/dev/null
if [ "$(grep -c 'pxh shell-config bash' "${HOME}/.bashrc")" = "1" ]; then
    pass "install is idempotent"
else
    fail "install duplicated the shell-config line"
fi

# -- 2. Record through real shells -------------------------------------------
step "Record via interactive bash"
drive_shell bash "echo e2e-bash-marker" "sh -c 'exit 3'" "cd /tmp" "echo in-tmp"
check "database created at XDG default" test -f "${DB}"
check_output "bash command recorded" "e2e-bash-marker" pxh show
check_output "bash exit status recorded" "sh -c 'exit 3'" pxh show --failed
check_output "bash working directory recorded" "in-tmp" env PWD=/tmp pxh show --here

step "Record via interactive zsh"
drive_shell zsh "echo e2e-zsh-marker" "sh -c 'exit 4'"
check_output "zsh command recorded" "e2e-zsh-marker" pxh show
check_output "zsh exit status recorded" "sh -c 'exit 4'" pxh show --failed
if [ "$(sqlite3 "${DB}" "SELECT count(DISTINCT shellname) FROM command_history")" = "2" ]; then
    pass "both shells recorded with their own shellname"
else
    fail "expected entries from both bash and zsh"
fi
if [ "$(sqlite3 "${DB}" "SELECT count(*) FROM command_history WHERE end_unix_timestamp IS NULL")" = "0" ]; then
    pass "every recorded command was sealed"
else
    fail "unsealed commands remain"
fi

# -- 3. Search -----------------------------------------------------------------
step "Search"
check_output "show with pattern" "e2e-bash-marker" pxh show bash-marker
check_output "show --limit 1 returns newest" "exit 4" pxh show --limit 1
check_output "recall --print lists history" "e2e-zsh-marker" pxh recall --print -q zsh-marker
check_output "autosuggest completes prefix" "echo e2e-zsh-marker" pxh autosuggest -- "echo e2e-z"

# -- 4. Import / export ------------------------------------------------------
step "Import / Export"
histfile=$(mktemp)
printf 'imported-cmd-1\nimported-cmd-2\n' > "${histfile}"
check "import bash histfile" pxh import --shellname bash --histfile "${histfile}"
check_output "imported commands visible" "imported-cmd-1" pxh show
export_file=$(mktemp)
pxh export > "${export_file}"
check "export is a JSON array" sh -c "head -c1 '${export_file}' | grep -q '\['"
check_output "export round trips through json import" "imported" \
    sh -c "pxh --db /tmp/rt.db import --shellname json --histfile '${export_file}' && pxh --db /tmp/rt.db show"
rm -f "${histfile}" "${export_file}" /tmp/rt.db

# -- 5. Stats / Doctor -------------------------------------------------------
step "Stats / Doctor"
check_output "stats shows count" "Commands:" pxh stats
check "doctor runs" pxh doctor --verbose
check_output "doctor sees installed hooks" "contains pxh shell-config" pxh doctor --verbose
check_output "doctor report format" "<details>" pxh doctor --report

# -- 6. Scan / Scrub ---------------------------------------------------------
step "Scan / Scrub"
drive_shell bash "curl -H 'Authorization: Bearer AKIAIOSFODNN7EXAMPLE'"
check_output "scan detects secret typed in a real shell" "AKIA" pxh scan
before=$(sqlite3 "${DB}" "SELECT count(*) FROM command_history")
check "scrub with scan patterns" pxh scrub --scan --yes
after=$(sqlite3 "${DB}" "SELECT count(*) FROM command_history")
if [ "${after}" -lt "${before}" ]; then
    pass "scrub removed commands (${before} -> ${after})"
else
    fail "scrub did not remove commands (${before} -> ${after})"
fi
check "maintenance runs" pxh maintenance

# -- 7. Sync (directory mode) -------------------------------------------------
step "Sync (directory mode)"
sync_dir=$(mktemp -d)
pxh --db "${sync_dir}/other.db" insert \
    --working-directory /opt --hostname other-host --shellname bash \
    --username root --session-id 300 --start-unix-timestamp "$(date +%s)" "remote-only-cmd"
check "sync directory mode" pxh sync "${sync_dir}"
check_output "synced command visible" "remote-only-cmd" pxh show
check "our db published to the sync dir" sh -c "ls '${sync_dir}'/*.db | grep -v other.db"
rm -rf "${sync_dir}"

# -- Summary ------------------------------------------------------------------
echo ""
echo "================================"
echo "Results: ${passed} passed, ${failed} failed"
echo "================================"
[ "${failed}" -eq 0 ]
