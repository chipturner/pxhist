# pxh

Fast, local-first shell history search and sync for bash and zsh.

Your shell history is one of the most useful things on your machine -- but it's
fragile, unsearchable, and stuck on one box. pxh fixes that. It stores every
command in a local SQLite database with rich metadata (directory, host, exit
code, duration), gives you powerful regex search and an interactive TUI, and
syncs across machines over SSH or a shared filesystem.

**No cloud accounts. No servers. No networking code. No AI.** Just your history,
on your machines, searchable in milliseconds.

```
$ pxh s ffmpeg
 2022-06-04 07:54:04  ffmpeg -encoders | grep '^ V'
 2022-06-07 23:17:33  ffmpeg -y -r 30 -f concat -safe 0 -i <(sed...) -c:v libx264rgb output.mp4
 2022-08-03 10:39:11  ffmpeg -i cropped.mp4 -vf "pad=width=430:..." cropped.gif
```

## Install

**Homebrew** (macOS and Linux):

```bash
brew install chipturner/tap/pxh
```

**Prebuilt binaries** (Linux x86_64/ARM64, macOS x86_64/ARM64):

```bash
curl -sSfL https://raw.githubusercontent.com/chipturner/pxhist/main/install.sh | sh
```

**From source:**

```bash
cargo install pxh        # requires Rust 1.88+
```

### Shell setup

After installing, set up shell integration (including tab completions) and import your existing history:

```bash
pxh install bash  # or: pxh install zsh

# Import your existing history
pxh import --shellname bash --histfile ~/.bash_history
# or for zsh:
pxh import --shellname zsh --histfile ~/.zsh_history

# Activate in current session without restarting
source <(pxh shell-config bash)  # or: source <(pxh shell-config zsh)
```

From now on, pxh automatically records commands with directory, host, user, exit code, and duration.

> **Note:** By default, trivial commands (`ls`, `cd`, `pwd`, `exit`, etc.) are not recorded. See [Configuration](#configuration) to change this.

> **Note:** fish shell is not currently supported. PRs welcome!

## Usage

### Interactive Browser (pxh recall)

Press **Ctrl-R** in your shell to open the interactive history browser. Type to filter, arrows to navigate, Enter to execute, Tab to edit before executing. Alt-1 through Alt-9 quick-select visible entries.

```bash
pxh recall           # Open history browser
pxh recall --here    # Limit to current directory
pxh recall -q "git"  # Start with a pre-filled query
```

Supports both emacs (default) and vim keybindings -- set `keymap = "vim"` in `~/.config/pxh/config.toml`.

**Keybindings:**
- `Ctrl-Y` / `Alt-W` -- Copy selected command to clipboard (via OSC 52)
- `Ctrl-K` -- Delete selected entry from history
- `Ctrl-H` -- Toggle host filter
- `Ctrl-G` -- Toggle directory/global filter
- `Alt-1` through `Alt-9` -- Quick-select visible entries

### Searching History (pxh show)

The `show` command (alias: `s`) is the power-search interface:

```bash
pxh show ffmpeg           # Find commands containing "ffmpeg"
pxh s docker run          # Multiple patterns match in order (docker.*\s.*run)
pxh s -i CMAKE            # Case-insensitive search
pxh s --here              # Only commands from current directory
pxh s --session $PXH_SESSION_ID  # Only commands from current shell session
pxh s -v cargo build      # Verbose: show duration, session, directory
pxh s -l 100 git          # Show up to 100 results (default: 50, 0 = unlimited)
pxh s --loosen foo bar    # Match patterns in any order
pxh s -F                  # Show commands that failed (non-zero exit)
pxh s -F docker           # Failed docker commands
pxh s -H                  # Short for --here (current directory only)
pxh s -S current          # Short for --session current
pxh s --working-directory ~/project  # Filter to a specific directory
```

Failed commands are highlighted in red when the status column is visible (`-v` or `-F`).

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

> **Note:** Remote sync automatically detects whether the remote host uses XDG paths (`~/.local/share/pxh/`) or the legacy path (`~/.pxh/`). Use `--remote-db` to override if needed.

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
pxh import --shellname zsh                # defaults to ~/.zsh_history
pxh import --shellname bash               # defaults to ~/.bash_history
pxh import --shellname zsh -n             # dry-run: show count without importing

# Import from another machine
pxh import --shellname zsh --histfile <(ssh server cat ~/.zsh_history) \
    --hostname server --username root

# Import from Atuin
contrib/atuin-to-pxh | pxh import --shellname json --histfile /dev/stdin
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

#### Shell Completions

Tab completions are included automatically by `pxh shell-config`. To generate them separately:

```bash
pxh completions bash    # or: zsh
```

#### Diagnostics

Check for common issues or view history statistics:

```bash
pxh doctor                # Diagnose common issues
pxh doctor --fix          # Attempt automatic fixes
pxh doctor --report       # Generate a diagnostic report for bug reports
pxh stats                 # Show history statistics
```

## Configuration

pxh reads configuration from `~/.config/pxh/config.toml`. All settings are optional with sensible defaults.

> **Note:** If `~/.pxh` exists from a previous installation, pxh uses it as a fallback for both config and data. New installations use XDG paths by default (`~/.config/pxh/` for config, `~/.local/share/pxh/` for data). Override with `XDG_CONFIG_HOME` and `XDG_DATA_HOME`.

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

[host]
# Override the detected hostname
hostname = "my-laptop"
# Previous hostnames for this machine (history from these is treated as local)
aliases = ["old-laptop", "work-mac"]

[shell]
# Disable Ctrl-R binding (use pxh recall directly instead)
disable_ctrl_r = false

[history]
# Regex patterns for commands to skip recording.
# Default patterns ignore trivial commands (ls, cd, pwd, exit, etc.)
# Set to [] to disable.
ignore_patterns = [
    "^ls$",
    "^cd( .)?$",      # matches cd, cd -, cd ~, cd / (single-char args only)
    "^pwd$",
    "^exit$",
    "^clear$",
    "^fg$",
    "^bg$",
    "^jobs$",
    "^history$",
    "^true$",
    "^false$",
]
```

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
pxh s -F  # Show commands that exited with non-zero status

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
- **Backup:** The database is a single SQLite file - `cp ~/.local/share/pxh/pxh.db backup/`

### zsh-autosuggestions

If you use [zsh-autosuggestions](https://github.com/zsh-users/zsh-autosuggestions), pxh automatically registers itself as a suggestion strategy. Suggestions come from your full cross-machine history database, not just the local zsh history file.

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

**Option 2: Config file** (`~/.config/pxh/config.toml`)
```toml
[shell]
disable_ctrl_r = true
```

You can still use `pxh recall` directly or bind it to a different key.

## Credits

Inspired by [bash-history-sqlite](https://github.com/thenewwazoo/bash-history-sqlite), [zsh-histdb](https://github.com/larkery/zsh-histdb), [mcfly](https://github.com/cantino/mcfly), and [atuin](https://github.com/atuinsh/atuin). Embeds [bash-preexec](https://github.com/rcaloras/bash-preexec) and [secrets-patterns-db](https://github.com/mazen160/secrets-patterns-db).

## How it Works

pxh contains **zero networking code**. Sync works by invoking your SSH client or reading/writing files on a shared filesystem. No accounts, no cloud services, no ports, no attack surface. Your history stays on machines you control.

All data lives in a local SQLite database (`~/.local/share/pxh/pxh.db`). There's no central server. The binary is statically linked with no runtime dependencies beyond libc -- consistent behavior across bash and zsh, proper handling of edge cases (quoting, binary data, concurrent access), and fast enough that you never notice it.

pxh hooks into your shell via preexec/precmd functions to capture each command with its start/end time, working directory, exit status, session ID, hostname, and username. For bash, it embeds [bash-preexec](https://github.com/rcaloras/bash-preexec); for zsh, it uses native hooks.

Commands are stored as BLOBs (to handle non-UTF8 data) in SQLite with WAL mode and a 5-second busy timeout, so multiple shells can record simultaneously. A unique index prevents duplicates. Secret scanning uses patterns from [secrets-patterns-db](https://github.com/mazen160/secrets-patterns-db), categorized by confidence level.

**Database location:** `~/.local/share/pxh/pxh.db` (override with `--db` or `PXH_DB_PATH`)

```bash
sqlite3 ~/.local/share/pxh/pxh.db "SELECT * FROM command_history LIMIT 10"
```
