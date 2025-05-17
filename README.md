# pxh
Portable, extensible history manager for interactive shells and other
REPL tools (pxh for short).

pxh's job is to be a reliable and unobtrusive the persistence and
search engine for tracking one of the most valuable knowledge vaults
you have -- your shell histories.  _pxh_ can import your existing
history files to give you a head start and provides consistent,
on-demand synchronizing across computers.

Key features:
- _pxh_ is _fast and unobtrusive_; once installed, you should never notice
  it except when you want to conduct a search.
- _pxh_ tags history commands with _additional context_ such as the
directory, host, and user which lets you store **all** of your
history, not just your primary computer's.  It also tracks exit codes
and durations.
- _pxh_ _supports flexible_ searching to quickly surface relevant and
useful entries such as all commands run in a specific directory or
commands issued in a given shell session.
- _pxh_ provides _easy, on-demand synchronization_ across computers to
allow for an "eventually consistent" global view of your interactive
shell history.

Quick links:
* [Basic Usage](#basic-usage)
* [Getting started](#getting-started)
* [How it works](#how-it-works)

Currently _pxh_ supports bash and zsh.

## Basic Usage

The _pxh_ workflow involves searching history.  _pxh_ aims to provide
better performance and ergonomics than `history | grep SOME_COMMAND`
in addition to ensuring it never misses a history entry.  To search
history, use `pxh show REGEX` or `pxh s REGEX`.

### Use case: remembering complex commands

`ffmpeg` is a great tool but I never quite remember how to use it.
Fortunately my shell history does:

``` bash
$ pxh s ffmpeg
...
 2022-06-04 07:54:04  ffmpeg -encoders | grep '^ V'
 2022-06-07 23:17:33  ffmpeg -y -r 30 -f concat -safe 0 -i <(sed 's/^/file /' /tmp/files) -c:v libx264rgb -preset veryslow -crf 21 -vf fps=30 /tmp/combined.mp4
 2022-08-03 10:39:11  ffmpeg -i cropped.mp4 -vf "pad=width=430:height=430:x=215:y=0:color=black" cropped.gif
...
```

### Use case: seeing examples of history grepping pxh can simplify

Since _pxh_ was created to simplify history searching, if you tended
to perform sequences of `history | grep ...` it may be useful to map
those solutions to _pxh_ invocations:

``` bash
$ pxh s -v history grep
 Start                Command
 2011-08-24 14:51:48  history | grep port | grep install
 2012-11-04 09:52:24  history | tail -50 | grep rm
 2013-02-02 23:45:06  history | grep ec2-descr
 2013-03-27 17:00:42  history | grep qemu
 2020-04-02 10:12:52  history | grep gphotos-sync | grep pip
 2021-03-29 09:14:34  history | grep squashfuse | grep -i release
```

### Use case: exploring project-relative history commands

Since _pxh_ tracks the directory you issue a command in, and since
directories often are a bit of localized context (i.e. when you are
working on an open source project), filtering by directory can
sometimes be useful.  For instance, commands run while hacking on
_pxh_:

``` bash
$ pxh s --here
 Start                Command
 2023-02-06 22:43:16  cargo test
 2023-02-06 22:43:21  cargo build --release
 2023-02-06 22:43:25  pxh sync ~/Dropbox/pxh/
 2023-02-06 22:44:28  git diff
 2023-02-06 22:44:36  cargo clippy
 2023-02-06 22:44:42  git commit -a -m 'clippy fixes'
 2023-02-06 22:44:44  git push
 ...
```

### Use case: seeing more details such as execution time

You can view additional details such as the host, directory, duration,
and exit code with the `-v` flag

``` bash
$ pxh s -v cargo build
 Start                Duration  Session       Context                                           Command
...
 2023-02-06 22:10:20  1s        116ef63fc226  .                                                 cargo build --release
 2023-02-06 22:28:00  2s        116ef63fc226  .                                                 cargo build --release
 2023-02-07 05:50:02  0s        ee6e1989f3da  /home/chip                                        cargo build --release
 2023-02-07 06:32:04  37s       ee6e1989f3da  .                                                 cargo build --release
...
```

### Use case: ergonomic, intuitive search

_pxh_ does the intuitive thing when given multiple search filters: it
finds results that match each filter in consecutive order as separate
words (basically creating a regex by joining the supplied patterns
with `.*\s.*`, which is a bit unwieldy to type):

``` bash
$ pxh s git pull
 Start                Command
...
 2023-01-31 11:11:36  git pull --rebase
 2023-02-03 08:18:34  fd -t d -d 1 -x git -C {} pull --rebase
 2023-02-03 10:44:02  git pull --rebase
 ...
```

### Use case: synchronizing across computers

Finally, sharing history across time and space is easy. You have two main options:

#### Direct SSH synchronization

For the simplest setup, you can sync directly between computers over SSH without requiring any shared storage:

``` bash
# Bidirectional sync (default)
$ pxh sync --remote homebase
Syncing with homebase...
Sync completed successfully

# Send local history to remote only
$ pxh sync --remote homebase --send-only
Syncing with homebase...
Send completed successfully

# Receive remote history only
$ pxh sync --remote homebase --receive-only
Syncing with homebase...
Received database: considered 314181 entries, added 1502 entries
```

This works seamlessly with SSH configurations from your `~/.ssh/config`, respects SSH keys and agents, and supports custom SSH commands via the `-e` flag (similar to rsync).

#### Shared directory synchronization

Alternatively, you can use a shared storage system like Dropbox, OneDrive, or CIFS. On each computer, just run `pxh sync $DIR`:

First computer (`nomad`):
``` bash
$ pxh sync ~/Dropbox/pxh/
Syncing from /home/chip/Dropbox/pxh/homebase.db...done, considered 314181 rows and added 5
Saved merged database to /home/chip/Dropbox/pxh/nomad.db
```

Second computer (`homebase`):

``` bash
$ pxh sync ~/Dropbox/pxh/
Syncing from /Users/chip/Dropbox/pxh/nomad.db...done, considered 314236 rows and added 55
Saved merged database to /Users/chip/Dropbox/pxh/homebase.db
```

Note both methods can also act as a backup method (as can `cp` on the `pxh` database file).

More advanced usage and flags can be explored via `pxh help`.

## Getting Started

- Install the _pxh_ binary
- Install the _pxh_ shell helper: `pxh install YOUR_SHELL_NAME`
  (e.g. `zsh`).
  - _pxh_ will be active on future shells.  To activate for this an
    existing session, run `source <(pxh shell-config YOUR_SHELLNAME)`
- Import your history:
  - zsh: `pxh import --shellname zsh --histfile ~/.zsh_histfile`
  - bash: `pxh import --shellname bash --histfile ~/.bash_history`
  - Optional: pull from another computer: `pxh import --shellname zsh --hostname HOST --username root --histfile <(ssh root@HOST cat /root/.zsh_histfile)`
- Periodically synchronize with databases from other systems with two options:
  - Via shared storage (NextCloud, Dropbox, CIFS, etc):
    - `pxh sync ~/Dropbox/pxh/` which merges from all db files in that
      directory and writes a new file with the merged output
  - Via SSH connection (no shared storage required):
    - `pxh sync --remote user@host` bidirectional sync (default)
    - `pxh sync --remote user@host --send-only` pushes local history to remote only
    - `pxh sync --remote user@host --receive-only` pulls remote history from local only
    - Use `--remote-db` to specify non-default remote database path
    - Use `--ssh-cmd` to specify custom SSH command (like rsync's -e option)

## How it Works

_pxh_ uses SQLite to make your history easily searchable you can
quickly find useful commands, even from years ago.  SQLite is fast,
and _pxh_ attempts to use it as efficiently as possible.  It is
unacceptable if _pxh_ adds noticeable latency to interactive shells
and searching for simple cases should be instantaneous.

pxh works using shell helpers to call it before and after every
command to log the command, time, exit status, and other useful
context.  The database is updated in real-time and remains consistent
across multiple concurrent shells thanks to SQLite.

The database file is stored, by default, in `~/.pxh/pxh.db`.  You can
`cp` this file and examine it with the `sqlite3` command line tool.

### Credits, Inspiration, and Similar Tools

This tool was originally inspired by
[bash-history-sqlite](https://github.com/thenewwazoo/bash-history-sqlite)
and [zsh-histdb](https://github.com/larkery/zsh-histdb).  These tools,
and similar ones, are excellent, but I found myself wanting to extend
the concepts further:

- It seems redundant to build a tool per shell; _pxh_ is meant to be a
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

## TODO / Ideas

### Usability / UX
- P1: expose column names as a `show` option to control output fields
  and order
- P2: colorize output?  parts where regex matches in addition to columns
- P3: special handling of ctrl-z when displaying shell
  history... annoying, need signal number, find a crate?
- P3: optional pretty ncurses-style interface?

### Core Features
- P1: Add more complex filtering to `show` to select history entries
  restricted to the host, user, etc.
- P3: stats subcommand to show some interesting data

### Extensions
- P2: more shell support
  - P2: and then non-shells like mysql, python, gdb, sqlite_history
    ...
- P3: explore using _pxh_ for interactive shell incremental history
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
