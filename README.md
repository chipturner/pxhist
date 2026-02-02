# pxh

Portable, extensible history manager for bash and zsh.

pxh is a fast, reliable, and unobtrusive persistence and search engine for one of your most valuable knowledge vaults: your shell history. Import your existing history files to get started, then enjoy powerful search, synchronization across machines, and security scanning.

## Features

- **Fast and unobtrusive** - Once installed, you'll never notice it except when searching
- **Rich metadata** - Tags commands with directory, host, user, exit codes, and durations
- **Powerful search** - Regex patterns, directory filtering, session filtering, and more
- **Interactive browser** - TUI for exploring history with vim/emacs keybindings
- **Cross-machine sync** - No cloud service or account required; uses SSH or shared directories
- **Privacy-focused** - Contains zero networking code; all sync happens through SSH or filesystem
- **Security scanning** - Detect and remove secrets from your history
- **Configuration** - Customize behavior via `~/.pxh/config.toml`

Currently supports **bash** and **zsh**.

## Table of Contents

- [Installation](#installation)
- [Usage](#usage)
  - [Interactive Browser](#interactive-browser-pxh-recall)
  - [Searching History](#searching-history-pxh-show)
  - [Synchronizing](#synchronizing-history-pxh-sync)
  - [Security](#security-scanning-and-scrubbing)
  - [Other Commands](#other-commands)
- [Configuration](#configuration)
- [Design Tradeoffs](#design-tradeoffs)
- [Tips and Tricks](#tips-and-tricks)
- [Credits](#credits)
- [How it Works](#how-it-works)

## Installation

### From source

```bash
git clone https://github.com/chipturner/pxhist.git
cd pxhist
cargo build --release
cp target/release/pxh ~/.local/bin/  # or somewhere in your PATH
```

### Shell setup

After installing the binary, set up shell integration and import your existing history:

```bash
# Install shell hooks (modifies your .bashrc or .zshrc)
pxh install bash  # or: pxh install zsh

# Import your existing history - this is important!
pxh import --shellname bash --histfile ~/.bash_history
# or for zsh:
pxh import --shellname zsh --histfile ~/.zsh_history

# Activate in current session without restarting
source <(pxh shell-config bash)  # or: source <(pxh shell-config zsh)
```

From now on, pxh will automatically record your commands with rich metadata.

## Usage

### Interactive Browser (pxh recall)

The `pxh recall` command opens an interactive TUI for browsing your history. When used standalone, pressing Enter displays detailed information about the selected command.

```bash
pxh recall           # Open history browser
pxh recall --here    # Limit to current directory
pxh recall -q "git"  # Start with a pre-filled query
```

When you press **Ctrl-R** in your shell, pxh recall opens in shell integration mode where Enter executes the selected command directly.

**Keybindings:**

| Key | Action |
|-----|--------|
| Type | Filter results incrementally |
| ↑/↓ or Ctrl-R/Ctrl-N | Navigate results |
| Enter | Show details (standalone) or execute (Ctrl-R) |
| Tab or → | Edit before executing (Ctrl-R mode) |
| Alt-1 to Alt-9 | Quick-select visible entries |
| Backspace | Delete last character |
| Ctrl-W | Delete last word |
| Ctrl-U | Clear query |
| Ctrl-C, Ctrl-D, or Esc | Cancel |
| Ctrl-P/Ctrl-N | Navigate (emacs mode) |
| j/k | Navigate (vim normal mode) |

**Vim mode:** Configure `keymap = "vim"` in your config file to start in vim insert mode. Press Escape to enter normal mode.

### Searching History (pxh show)

For power searching and scripting, use the `show` command (alias: `s`). This is the primary interface for finding commands:

```bash
pxh show ffmpeg           # Find commands containing "ffmpeg"
pxh s docker run          # Multiple patterns match in order (docker.*\s.*run)
pxh s -i CMAKE            # Case-insensitive search
pxh s --here              # Only commands from current directory
pxh s --session $PXH_SESSION_ID  # Only commands from current shell session
pxh s -v cargo build      # Verbose: show duration, session, directory
pxh s -l 100 git          # Show up to 100 results (default: 50, 0 = unlimited)
pxh s --loosen foo bar    # Match patterns in any order
```

**Example output:**
```
$ pxh s ffmpeg
 2022-06-04 07:54:04  ffmpeg -encoders | grep '^ V'
 2022-06-07 23:17:33  ffmpeg -y -r 30 -f concat -safe 0 -i <(sed...) -c:v libx264rgb output.mp4
 2022-08-03 10:39:11  ffmpeg -i cropped.mp4 -vf "pad=width=430:..." cropped.gif
```

**Verbose output (`-v`):**
```
$ pxh s -v cargo build
 Start                Duration  Session       Context      Command
 2023-02-06 22:10:20  1s        116ef63fc226  .            cargo build --release
 2023-02-07 06:32:04  37s       ee6e1989f3da  .            cargo build --release
```

### Synchronizing History (pxh sync)

Sync history across machines via SSH or a shared directory. pxh contains no networking code itself - sync works by invoking SSH or reading/writing files from a shared filesystem.

#### SSH Synchronization

```bash
# Bidirectional sync (default)
pxh sync --remote myserver

# One-way sync
pxh sync --remote myserver --send-only     # Push local → remote
pxh sync --remote myserver --receive-only  # Pull remote → local

# Custom SSH options (like rsync's -e flag)
pxh sync --remote myserver -e "ssh -p 2222"
pxh sync --remote myserver -e "ssh -i ~/.ssh/special_key"

# Sync only recent history
pxh sync --remote myserver --since 30  # Last 30 days only

# Custom remote paths
pxh sync --remote myserver --remote-db /custom/path/pxh.db
pxh sync --remote myserver --remote-pxh /usr/local/bin/pxh
```

#### Shared Directory Synchronization

Use Dropbox, OneDrive, NFS, or any shared filesystem:

```bash
# On each machine, run:
pxh sync ~/Dropbox/pxh/

# Export only (don't import from others)
pxh sync ~/Dropbox/pxh/ --export-only
```

Each machine writes its own `.db` file and reads from all others.

### Security: Scanning and Scrubbing

#### Scanning for Secrets

Detect potential secrets (API keys, passwords, tokens) in your history:

```bash
pxh scan                        # Scan with default sensitivity (critical)
pxh scan -c high                # Include high-confidence matches
pxh scan -c all                 # Show all potential matches
pxh scan -v                     # Verbose: show which pattern matched
pxh scan --json                 # Output as JSON for scripting
pxh scan --histfile ~/.bash_history  # Scan a histfile directly
pxh scan --dir ~/Dropbox/pxh/   # Scan all databases in a sync directory
```

Confidence levels: `critical` (default), `high`, `low`, `all`

#### Removing Secrets

Remove sensitive commands from your history:

```bash
pxh scrub                       # Interactive: prompts for the secret to remove
pxh scrub "my-api-key"          # Remove commands containing this string
pxh scrub --scan                # Remove all secrets found by scan
pxh scrub --scan -c high        # Remove critical and high-confidence secrets
pxh scrub -n                    # Dry-run: show what would be removed
pxh scrub -y                    # Skip confirmation prompt

# Scrub from multiple locations
pxh scrub --histfile ~/.bash_history "secret"  # Also scrub from histfile
pxh scrub --dir ~/Dropbox/pxh/ "secret"        # Scrub from sync directory
pxh scrub --remote myserver "secret"           # Scrub from remote machine
```

### Other Commands

#### Import

Import history from existing shell history files:

```bash
pxh import --shellname zsh --histfile ~/.zsh_history
pxh import --shellname bash --histfile ~/.bash_history

# Import from another machine
pxh import --shellname zsh --histfile <(ssh server cat ~/.zsh_history) \
    --hostname server --username root
```

#### Export

Export your entire history as JSON:

```bash
pxh export > history.json
pxh export | jq '.[] | select(.exit_status != 0)'  # Filter failed commands
```

#### Maintenance

Optimize database performance and reclaim space:

```bash
pxh maintenance           # ANALYZE and VACUUM the database
pxh maintenance other.db  # Operate on a specific database file
```

## Configuration

pxh reads configuration from `~/.pxh/config.toml`. All settings are optional with sensible defaults.

**Example configuration:**

```toml
[recall]
# Keymap mode: "emacs" (default) or "vim"
keymap = "emacs"

# Show the preview pane with command details
show_preview = true

# Maximum results to load (default: 5000)
result_limit = 5000

[recall.preview]
# Which fields to show in the preview pane
show_directory = true
show_timestamp = true
show_exit_status = true
show_duration = true
show_hostname = false  # Useful if syncing across machines

[shell]
# Disable Ctrl-R binding (use pxh recall directly instead)
disable_ctrl_r = false
```

## Design Tradeoffs

pxh makes deliberate design choices that differ from other history tools:

### No Networking Code

pxh contains zero networking code. All synchronization happens through:
- **SSH**: pxh invokes your SSH client directly, leveraging your existing keys and config
- **Shared filesystems**: Dropbox, NFS, or any directory you can read/write

This means no accounts to create, no cloud services to trust, no ports to open, and no attack surface from network-facing code. Your history stays on machines you control.

### Local-First Storage

All data is stored in a local SQLite database (`~/.pxh/pxh.db`). There's no central server. You own your data completely.

### Performance Goals

pxh is designed to be fast enough that you never notice it:
- **Recording**: Commands are inserted via prepared statements; you won't feel any delay
- **Searching**: SQLite with proper indexes handles hundreds of thousands of commands easily
- **TUI**: Optimized rendering with buffered writes and minimal syscalls

### Native Binary

Unlike shell-script-based solutions, pxh is a compiled Rust binary. This provides:
- Consistent behavior across bash and zsh
- Proper handling of edge cases (quoting, binary data, concurrent access)
- No dependency on system SQLite version or shell quirks

### Minimal Dependencies at Runtime

pxh has no runtime dependencies beyond libc. The SQLite library is statically linked.

## Tips and Tricks

### Quick Search Alias

Create a symlink named `pxhs` pointing to `pxh`, and it will automatically run `pxh show`:

```bash
ln -s $(which pxh) ~/.local/bin/pxhs
pxhs ffmpeg  # Equivalent to: pxh show ffmpeg
```

### Useful Patterns

```bash
# Commands that failed
pxh s -v . | grep -v "0s"  # Or query the database directly

# What did I do in this project last week?
pxh s --here -l 0

# How did I use that obscure tool?
pxh s -v ansible-playbook

# Commands from a specific session
pxh s --session $PXH_SESSION_ID  # Current session
```

### Sync Strategies

- **Real-time sync:** Run `pxh sync --remote server` periodically via cron
- **Shared folder:** Just run `pxh sync ~/Dropbox/pxh/` occasionally on each machine
- **Backup:** The database is a single SQLite file - `cp ~/.pxh/pxh.db backup/`

### Privacy

- Commands starting with a space are ignored (like bash's `HISTCONTROL=ignorespace`)
- Use `pxh scan` regularly to detect accidentally committed secrets
- Use `--no-secret-filter` with sync if you want to disable automatic secret filtering during import

### Disabling Ctrl-R

If you prefer to keep your shell's default Ctrl-R behavior:

**Option 1: CLI flag**
```bash
# When sourcing manually:
source <(pxh shell-config zsh --no-ctrl-r)
```

**Option 2: Config file** (`~/.pxh/config.toml`)
```toml
[shell]
disable_ctrl_r = true
```

You can still use `pxh recall` directly or bind it to a different key.

## Credits

Inspired by:
- [bash-history-sqlite](https://github.com/thenewwazoo/bash-history-sqlite)
- [zsh-histdb](https://github.com/larkery/zsh-histdb)
- [mcfly](https://github.com/cantino/mcfly)
- [atuin](https://github.com/atuinsh/atuin)

pxh improves on shell-script solutions by:
- Supporting multiple shells with one tool
- Using a native binary instead of shell-invoked SQLite (faster, no quoting issues)
- Providing a TUI, synchronization, and security scanning

Embeds:
- [Bash-Preexec](https://github.com/rcaloras/bash-preexec) for bash hook support
- [secrets-patterns-db](https://github.com/mazen160/secrets-patterns-db) for secret detection patterns

## How it Works

pxh uses SQLite to store and search your history efficiently. It hooks into your shell via preexec/precmd functions to capture:

- The command itself
- Start and end timestamps
- Working directory
- Exit status
- Session ID (to group commands by shell session)
- Hostname and username

### Database Schema

The main `command_history` table stores commands as BLOBs to handle arbitrary binary data (some commands contain non-UTF8 characters). A unique index prevents duplicates while allowing the same command to appear with different metadata.

### Concurrency

The database uses WAL (Write-Ahead Logging) journal mode with a 5-second busy timeout, allowing multiple shells to record commands simultaneously without conflicts.

### Shell Integration

For **bash**, pxh embeds and uses [bash-preexec](https://github.com/rcaloras/bash-preexec), which provides zsh-style `preexec` and `precmd` hooks. The `preexec` hook records the command and start time; `precmd` records the exit status and end time.

For **zsh**, pxh uses native `zshaddhistory` and `precmd` hooks.

### Secret Detection

The `scan` command uses patterns from [secrets-patterns-db](https://github.com/mazen160/secrets-patterns-db) to detect API keys, passwords, and tokens. Patterns are categorized by confidence level to minimize false positives.

**Database location:** `~/.pxh/pxh.db` (override with `--db` or `PXH_DB_PATH` environment variable)

You can examine the database directly:
```bash
sqlite3 ~/.pxh/pxh.db "SELECT * FROM command_history LIMIT 10"
```
