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

# Tapes reference this path via `Env PXH_DB_PATH`; keep them in sync.
DEMO_DIR=/tmp/pxh-demo
DEMO_DB="$DEMO_DIR/pxh.db"

die() { echo "error: $*" >&2; exit 1; }

command -v vhs >/dev/null || die "vhs not found on PATH"
[[ -x "$REPO_ROOT/target/release/pxh" ]] || die "no release binary; run: cargo build --release"
export PATH="$REPO_ROOT/target/release:$PATH"

rm -rf "$DEMO_DIR"
mkdir -p "$DEMO_DIR" "$SCRIPT_DIR/out"
pxh --db "$DEMO_DB" import --shellname json --histfile "$SCRIPT_DIR/fixture.json"

# Isolated HOME for shell-integration tapes (ctrl-r.tape sets Env HOME to
# this); .zshrc loads the pxh hooks and pins the hostname so live-recorded
# commands stay deterministic.
DEMO_HOME="$DEMO_DIR/home"
mkdir -p "$DEMO_HOME"
cat > "$DEMO_HOME/.zshrc" <<EOF
PROMPT='%F{blue}\$%f '
export PATH="$REPO_ROOT/target/release:\$PATH"
eval "\$(pxh shell-config zsh)"
export PXH_HOSTNAME=apollo
EOF

tapes=()
for tape in "$@"; do
    [[ -f "$tape" ]] || die "not found: $tape"
    tapes+=("$(cd "$(dirname "$tape")" && pwd)/$(basename "$tape")")
done
[[ ${#tapes[@]} -gt 0 ]] || tapes=("$SCRIPT_DIR"/*.tape)

cd "$SCRIPT_DIR"
for tape in "${tapes[@]}"; do
    echo "==> recording $(basename "$tape")"
    vhs "$tape"
done
echo "done: GIFs in $SCRIPT_DIR/out/"
