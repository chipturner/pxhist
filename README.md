# pxhist
Portable, extensible history manager for shells and other REPL tools (pxh for short).

pxh's job is to unobtrusively and obsessively become the persistence
engine for tracking one of the most valuable knowledge vaults you have
-- your shell histories.  It uses this storage (backed by SQLite) to
make your history easily searchable you can quickly find useful
commands, even from years ago.  pxh can import your existing history
files to give you a head start.

pxh tags history commands with the source host and user, meaning you
can store **all** of your history, not just your primary computer or
laptop's.

pxh works by using a database of every command along with available
context such as timestamps, command duration, etc.  The database is
updated in real-time and remains consistent across multiple concurrent
shells.

Currently pxh supports bash and zsh.

## Getting Started

- Install the pxh helper: `pxh install YOUR_SHELL_NAME`
- Import your history
  - zsh: `pxh import --shellname zsh --histfile ~/.zsh_histfile`
  - bash: `pxh import --shellname bash --histfile ~/.bash_history`
  - Optional: pull from another computer: `pxh import --shellname zsh --hostname HOST --username root --histfile <(ssh root@HOST cat /root/.zsh_histfile)`

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
  overhead is very small, and it is easier to build portable,
  performant complex tooling such as TUIs, complex search, analytics,
  etc.

This tool embeds the very useful
[Bash-Preexec](https://github.com/rcaloras/bash-preexec) utility which
provides very zsh-like extensions for Bash to track when commands
begin and end.

## Architecture / How it Works

## Hacking on pxh

## TODO

### Usability / UX
- P0: output commands "around" a timestamp (before, after,
  bracketing), like grep -C; bracket by time and number of commands,
  but be session aware
- P1: expose column names as a `show` option to control output fields
  and order
- P2: colorize output?  parts where regex matches in addition to columns
- P3: special handling of ctrl-z when displaying shell
  history... annoying, need signal number, find a crate?
- P3: optional pretty ncurses-style interface?

### Core Features
- P0: some kind of directory-based workflow to import/export in as a
  rendezvous to easily sync across devices (e.g. a Dropbox folder)
- P1: Add more complex filtering to `show` to select history entries
  restricted to the host, user, etc.
- P3: stats subcommand to show some interesting data

### Extensions
- P1: more shell support
  - P1: fish?
  - P2: and then non-shells like mysql, python, gdb, sqlite_history
    ...
- P3: explore using pxh for interactive shell incremental history
  search
- P3: create and document workflow for incremental updates,
  particularly for shells that don't support updating real-time
  (e.g. backfill from mysql history periodically)

### Misc
- P2: better code documentation, particularly around helper classes
- P2: document architecture and implementation details
- P3: some way to expunge things like passwords accidentally in
  history files w/o resorting to sqlite?  also prevent re-importing
  somehow?
