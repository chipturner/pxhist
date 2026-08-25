#!/usr/bin/env bash
set -euo pipefail

# Record demo GIFs from vhs tapes against a seeded fixture database.
#
# Usage:
#   demo/record.sh [tape...]    # default: every .tape in this directory
#
# Requires: vhs on PATH (https://github.com/charmbracelet/vhs) and a
# release build of pxh (cargo build --release). GIFs land in demo/out/.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"

DEMO_DIR=/tmp/pxh-demo
DEMO_DB="$DEMO_DIR/pxh.db"
DEMO_HOME="$DEMO_DIR/home"

die() { echo "error: $*" >&2; exit 1; }

command -v vhs >/dev/null || die "vhs not found on PATH"
[[ -x "$REPO_ROOT/target/release/pxh" ]] || die "no release binary; run: cargo build --release"
export PATH="$REPO_ROOT/target/release:$PATH"

# vhs inherits this environment, so tapes need no Env lines of their own.
export HOME="$DEMO_HOME"
export PXH_DB_PATH="$DEMO_DB"
export PXH_HOSTNAME=apollo

# Fresh fixture DB and HOME. Run before every tape so each records against
# the same state regardless of what earlier tapes inserted (ctrl-r.tape
# records live commands into the DB).
reset_env() {
    rm -rf "$DEMO_DIR"
    mkdir -p "$DEMO_HOME"
    pxh import --shellname json --histfile "$SCRIPT_DIR/fixture.json"
    cat > "$DEMO_HOME/.zshrc" <<EOF
PROMPT='%F{blue}\$%f '
export PATH="$REPO_ROOT/target/release:\$PATH"
eval "\$(pxh shell-config zsh)"
EOF
}

mkdir -p "$SCRIPT_DIR/out"

tapes=()
for tape in "$@"; do
    [[ -f "$tape" ]] || die "not found: $tape"
    tapes+=("$(cd "$(dirname "$tape")" && pwd)/$(basename "$tape")")
done
[[ ${#tapes[@]} -gt 0 ]] || tapes=("$SCRIPT_DIR"/*.tape)

cd "$SCRIPT_DIR"
for tape in "${tapes[@]}"; do
    echo "==> recording $(basename "$tape")"
    reset_env
    vhs "$tape"
done
echo "done: GIFs in $SCRIPT_DIR/out/"
