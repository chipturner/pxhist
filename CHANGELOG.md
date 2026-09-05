# Changelog

All notable user-visible changes. Schema-version and sync-protocol changes
are called out explicitly because they affect machines that sync with each
other.

## Unreleased

- The `pxh recall` preview pane shows timestamps in the local time zone,
  matching `pxh show`; it was UTC.
- `pxh show` renders its table itself instead of through `prettytable-rs`
  (unmaintained since 2022). Layout is unchanged; colors now go through the
  same `anstream` path as diagnostics, so `NO_COLOR` and `CLICOLOR` apply to
  the table too, and an unknown exit status shows dim rather than black.
- `pxh recall` measures commands, the host suffix, the mode indicator, and
  the cursor position in terminal columns, so CJK and other wide characters
  no longer overflow the row or misplace the cursor. A command that exactly
  fits the row is shown whole instead of being cut for a "..." it did not
  need.
- The host-settings migration (run by `pxh install` and `pxh config`) reports
  a config parse error or a failed write as a `warning:` line; previously
  those were `log` messages hidden unless `RUST_LOG` was set. `RUST_LOG` no
  longer does anything.
- `pxh bootstrap <host>`: install pxh on a remote host over SSH (this
  machine's release by default, `--release latest` for the newest), confirm
  the installed version, warn if plain `sync --remote` would not find it or
  would pick an older pxh installed elsewhere, and run a first sync through
  the new binary (`--no-sync` to skip). A remote sync that fails because pxh
  is missing there now points at it.
- `install.sh`: a relative `PXH_INSTALL_DIR` is relative to where the script
  was started, not to its scratch directory (where the binary was deleted
  on exit).
- Errors print as `error: <message>` (was Rust Debug output); warnings and
  hints are lowercase `warning:` / `hint:`; `NO_COLOR` honored.
- Config is strict: unknown keys are reported by `pxh doctor` and the file is
  ignored (defaults used) until fixed. `pxh config` writes a fully commented
  template when the config file is missing; the host-settings migration does
  the same.
- `pxh doctor` fails for configs pxh would reject (unknown keys) and reports
  the parser error.
- `pxh doctor --json`.
- `--version` shows the git build id.
- `pxh mangen <dir>` (hidden; for packagers).
- Internal subcommands (`insert`, `seal`, `shell-config`, `autosuggest`) are
  hidden from `--help`.

## 0.10.4

- Current release at the time this changelog was introduced. Schema v3;
  sync protocol `-v2` modes. Earlier history: `git log`.
