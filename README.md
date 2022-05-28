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
  - everyting else tbd
- incremental sync
- export/import: `pxh show --output-format json` and `pxh import --shellname json --histfile $JSON_PATH`

## Inspiration and Similar Tools

## Architecture / How it Works

## Hacking on pxhist

- `cargo sqlx prepare -- --bin pxh`

## TODO

### Usability / UX
- better command line help
- special handling of ctrl-z when displaying shell history
- re-evaluate fields from `pxh show`... maybe roll user/host/dir into
  the cwd field cleanly?

### Core Features
- support regular expressions in the show command, not just substrings
- stats subcommand to show some interesting data
- create and document workflow for incremental updates, particularly
  for shells that don't support updating realtime
- teach `show` to display history entries restricted to the current
  directory, host, user, etc

### Extensions
- support bash like zsh
  - and fish?
  - and then non-shells like mysql, python, gdb, sqlite_history ...
- explore using for interactive shell incremental history search

### Misc
- better code documentation, particularly around helper classes and
  magic for sqlx workflows (prepare etc)
- document architecture and implementation details
- some way to expunge things like passwords accidentally in history
  files w/o resorting to sqlite?  also prevent re-importing somehow?
