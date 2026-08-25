---
name: verify
description: Build pxh and drive a change end-to-end against an isolated HOME and database -- never the real ~/.local/share/pxh/pxh.db or ~/.config/pxh -- including the recall TUI in tmux.
---

# Verifying pxh changes by running them

The real database holds years of history and the real rc files load the
installed hooks. Every manual run below is isolated through `HOME` and
`PXH_DB_PATH`; nothing touches `~/.local/share/pxh` or `~/.config/pxh`.

Rebuild first: `cargo build`. `cargo nextest`/`clippy` do **not** refresh
`target/debug/pxh`; driving a stale binary looks exactly like a broken change
(`pxh --version` shows `-dirty` when the tree has uncommitted changes).

## Isolated environment

```sh
export SB=$PWD/target/verify-sb; rm -rf $SB; mkdir -p $SB/home
export HOME=$SB/home PXH_DB_PATH=$SB/pxh.db PXH_HOSTNAME=sandbox
P=$PWD/target/debug/pxh
$P import --shellname json --histfile demo/fixture.json   # seeded history
$P show -l 5; $P stats; $P doctor --verbose
```

Shell integration end to end (the hooks record into `$PXH_DB_PATH`):

```sh
cat > $HOME/.zshrc <<EOF
export PATH="$PWD/target/debug:\$PATH"
eval "\$(pxh shell-config zsh)"
EOF
zsh -i      # type a command, exit, then: $P show -l 1
```

## Recall TUI

Needs a pty: use the tmux tool with a dedicated session (`tmux -L pxh-verify`),
export the variables above inside it, run `$P recall`, and capture the pane.
Always send keys with `tmux-cli send` (see CLAUDE.md) -- plain `send-keys`
races the Enter key.

## Never

`pxh install` or `pxh doctor --fix --yes` with the real `HOME`; `pxh scrub`
against the real DB to "see what it does". Prefer a test in `tests/` over any
of the above.
