#!/usr/bin/env bash
set -euo pipefail

# Publish recorded demo GIFs to the gh-pages branch (under demo/) so the
# README can hot-link them without committing binaries to main.
#
# Safe alongside the docs workflow: it deploys rustdoc to gh-pages with
# destination_dir=docs, which leaves sibling paths like demo/ intact.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
WORKTREE="$REPO_ROOT/.worktrees/gh-pages"

die() { echo "error: $*" >&2; exit 1; }

gifs=("$SCRIPT_DIR"/out/*.gif)
[[ -f "${gifs[0]}" ]] || die "no GIFs in demo/out/; run demo/record.sh first"

git -C "$REPO_ROOT" fetch origin gh-pages
if [[ ! -d "$WORKTREE" ]]; then
    mkdir -p "$(dirname "$WORKTREE")"
    git -C "$REPO_ROOT" worktree add "$WORKTREE" gh-pages
fi
git -C "$WORKTREE" pull --ff-only origin gh-pages

mkdir -p "$WORKTREE/demo"
cp "${gifs[@]}" "$WORKTREE/demo/"
git -C "$WORKTREE" add demo
if git -C "$WORKTREE" diff --cached --quiet; then
    echo "gh-pages already up to date"
    exit 0
fi
git -C "$WORKTREE" commit -m "demo: update recorded GIFs"
git -C "$WORKTREE" push origin gh-pages
echo "published: https://chipturner.github.io/pxhist/demo/"
