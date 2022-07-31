# pxhist
Portable, eXtensible History database for command line tools.

pxhist's job is to unobtrusively and obsessively become the
persistence engine for tracking one of the most valuable knowledge
vaults you have -- your shell histories.  It does this by creating a
database of every command along with available context such as
timestamps, command duration, etc.

pxhist tags history commands with the sourec host and user, meaning
you can store **all** of your history, not just your primary computer
or laptop's.

pxhist makes your history easily searchable as well so you can quickly
find useful commands, even from years ago.

## Getting Started

- import
  - zsh: `pxh import --shellname zsh --histfile ~/.zsh_histfile`
  - bash: `pxh import --shellname bash --histfile ~/.bash_history`
  - fun trick, from another computer: `pxh import --shellname zsh --hostname HOST --username root --histfile <(ssh root@HOST cat /root/.zsh_histfile)`
- shell helpers
  - zsh: `source <(pxh shell-config zsh)`
  - zsh: `source <(pxh shell-config bash)`
  - everyting else tbd
- incremental sync
- export/import: `pxh show --output-format json` and `pxh import --shellname json --histfile $JSON_PATH`

## Inspiration and Similar Tools

This tool was originally inspired by
[bash-history-sqlite](https://github.com/thenewwazoo/bash-history-sqlite)
and [zsh-histdb](https://github.com/larkery/zsh-histdb).  These tools,
and similar ones, are excellent, but I found myself wanting to extend
the concepts further:

- It seems redundant to build a tool per shell; pxh is meant to be a
  solution for all shells (as well as shell-like REPL environments
  that track history like `mysql`, `python`, etc).
- Those tools rely on shell invocation of the sqlite CLI.  This
  works... until it doesn't.  It requires precision in quoting and,
  unfortunately, is somewhat prone to race conditions when shells
  start in close proximity.
- I wanted highly efficient tooling that was easy to extend.  By going
  with a native language like Rust, the per-command invocation
  overhead is very small, and it is easier to build portable complex
  tooling such as TUIs, complex search, analytics, etc.

This tool embeds the very useful
[Bash-Preexec](https://github.com/rcaloras/bash-preexec) utility which
provides very zsh-like extensions for Bash to track when commands
begin and end.

## Architecture / How it Works

## Hacking on pxhist

## TODO

### Usability / UX
- P0: re-evaluate fields from `pxh show`... maybe roll user/host/dir
  into the cwd field cleanly?
  - P1: also make field output in "show" fully parameterized bycolumn
    names
- P1: better command line help
- P1: output commands "around" a timestamp (before, after,
  bracketing), like grep -C
- P2: special handling of ctrl-z when displaying shell
  history... annoying, need signal number, find a crate?
- P3: optional pretty ncurses-style interface?
- P3: colorize output?  parts where regex matches in addition to columns

### Core Features
- P1: teach `show` to display history entries restricted to the
  current directory, host, user, etc.  Maybe `--here` to simplify the
  filter?
- P2: create and document workflow for incremental updates,
  particularly for shells that don't support updating realtime
  (e.g. backfill from mysql history periodically)
- P3: stats subcommand to show some interesting data

### Extensions
- P1: more shell support
  - P1: and then non-shells like mysql, python, gdb, sqlite_history
    ...
  - P3: fish?
- P3: explore using pxh for interactive shell incremental history
  search

### Misc
- P2: better code documentation, particularly around helper classes
- P2: document architecture and implementation details
- P3: some way to expunge things like passwords accidentally in
  history files w/o resorting to sqlite?  also prevent re-importing
  somehow?
