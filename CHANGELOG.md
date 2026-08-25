# Changelog

All notable user-visible changes. Schema-version and sync-protocol changes
are called out explicitly because they affect machines that sync with each
other.

## Unreleased

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
