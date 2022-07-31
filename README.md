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

## TODO

### Usability / UX
- P3: optional pretty ncurses-style interface?
- P1: better command line help
- P1: output commands "around" a timestamp (before, after,
  bracketing), like grep -C
- P2: special handling of ctrl-z when displaying shell
  history... annoying, need signal number, find a crate?
- P0: re-evaluate fields from `pxh show`... maybe roll user/host/dir
  into the cwd field cleanly?
	  - P1: also make field output in "show" fully parameterized by
	    column names
- P3: colorize output?

### Core Features
- P0: support regular expressions in the show command, not just
  substrings
- P3: stats subcommand to show some interesting data
- P3: create and document workflow for incremental updates,
  particularly for shells that don't support updating realtime
  (e.g. backfill from mysql history periodically)
- P1: teach `show` to display history entries restricted to the
  current directory, host, user, etc

### Extensions
- P0: support bash like zsh
  - P3: and fish?
  - P1: and then non-shells like mysql, python, gdb, sqlite_history
    ...
- P3: explore using pxh for interactive shell incremental history
  search

### Misc
- P2: better code documentation, particularly around helper classes
- P3: document architecture and implementation details
- P3: some way to expunge things like passwords accidentally in
  history files w/o resorting to sqlite?  also prevent re-importing
  somehow?
