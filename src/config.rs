use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use toml_edit::DocumentMut;

/// Embedded default config template (repo root `config.toml`). Every key,
/// commented out, at its default value. `pxh config` writes it when no
/// config exists; `template_parses_to_defaults` keeps it honest.
pub const DEFAULT_CONFIG: &str = include_str!("../config.toml");

/// The strict answer to "what does pxh think of the config file?", for
/// diagnostics. `Config::load()` collapses all three into a usable `Config`
/// (falling back to defaults); doctor needs to tell them apart.
#[derive(Debug)]
pub enum ConfigStatus {
    NotFound,
    Valid(Box<Config>),
    Invalid(String),
}

/// Strictly parse the config file at `path` -- the same parser `Config::load`
/// uses, so doctor can never call "valid" a file that load would reject.
pub fn config_status(path: &Path) -> ConfigStatus {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return ConfigStatus::NotFound,
        Err(e) => return ConfigStatus::Invalid(format!("cannot read config: {e}")),
    };
    match toml::from_str(&content) {
        Ok(cfg) => ConfigStatus::Valid(Box::new(cfg)),
        Err(e) => ConfigStatus::Invalid(e.to_string()),
    }
}

/// Configuration for history recording
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct HistoryConfig {
    /// Regex patterns for commands to ignore (not record).
    /// Set to [] to disable.
    pub ignore_patterns: Vec<String>,
}

fn default_ignore_patterns() -> Vec<String> {
    vec![
        "^ls$".into(),
        "^cd( .)?$".into(),
        "^pwd$".into(),
        "^exit$".into(),
        "^clear$".into(),
        "^fg$".into(),
        "^bg$".into(),
        "^jobs$".into(),
        "^history$".into(),
        "^true$".into(),
        "^false$".into(),
    ]
}

impl Default for HistoryConfig {
    fn default() -> Self {
        HistoryConfig { ignore_patterns: default_ignore_patterns() }
    }
}

/// Main configuration struct
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub host: HostConfig,
    #[serde(default)]
    pub recall: RecallConfig,
    #[serde(default)]
    pub shell: ShellConfig,
    #[serde(default)]
    pub history: HistoryConfig,
}

/// Configuration for host identity
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct HostConfig {
    pub hostname: Option<String>,
    pub machine_id: Option<u64>,
    pub aliases: Vec<String>,
}

/// Configuration for shell integration
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ShellConfig {
    /// Disable Ctrl-R binding (keep shell's default behavior)
    pub disable_ctrl_r: bool,
}

/// Configuration for the recall TUI
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct RecallConfig {
    /// Keymap mode: "emacs" or "vim"
    pub keymap: String,
    /// Whether to show the preview pane
    pub show_preview: bool,
    /// Maximum number of results to load
    pub result_limit: usize,
    /// Preview pane configuration
    pub preview: PreviewConfig,
}

impl Default for RecallConfig {
    fn default() -> Self {
        RecallConfig {
            keymap: "emacs".to_string(),
            show_preview: true,
            result_limit: 5000,
            preview: PreviewConfig::default(),
        }
    }
}

/// Configuration for what to show in the preview pane
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct PreviewConfig {
    pub show_directory: bool,
    pub show_timestamp: bool,
    pub show_exit_status: bool,
    pub show_hostname: bool,
    pub show_duration: bool,
}

impl Default for PreviewConfig {
    fn default() -> Self {
        PreviewConfig {
            show_directory: true,
            show_timestamp: true,
            show_exit_status: true,
            show_hostname: false,
            show_duration: true,
        }
    }
}

/// Keymap mode for navigation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeymapMode {
    #[default]
    Emacs,
    VimInsert,
    VimNormal,
}

impl RecallConfig {
    /// Get the initial keymap mode from config
    pub fn initial_keymap_mode(&self) -> KeymapMode {
        match self.keymap.to_lowercase().as_str() {
            "vim" => KeymapMode::VimInsert,
            _ => KeymapMode::Emacs,
        }
    }
}

impl Config {
    /// Load configuration from the default path, warning about a file that
    /// cannot be parsed.
    pub fn load() -> Self {
        Self::load_from_default_path(true).unwrap_or_default()
    }

    /// Load configuration from the default path without printing anything.
    /// The shell hooks run on every prompt with stderr on the terminal; a
    /// config they cannot parse must not warn once per command.
    pub fn load_quiet() -> Self {
        Self::load_from_default_path(false).unwrap_or_default()
    }

    fn load_from_default_path(warn: bool) -> Option<Self> {
        let config_path = Self::default_config_path()?;
        Self::read_from_path(&config_path, warn)
    }

    /// Returns true if the config file exists but fails to parse.
    /// Used to prevent migrate_host_settings from overwriting a corrupt config.
    pub fn has_parse_error() -> bool {
        let Some(path) = Self::default_config_path() else {
            return false;
        };
        let Ok(content) = fs::read_to_string(&path) else {
            return false; // file doesn't exist -- not a parse error
        };
        toml::from_str::<Config>(&content).is_err()
    }

    pub fn default_config_path() -> Option<PathBuf> {
        Some(crate::pxh_config_dir()?.join("config.toml"))
    }

    pub fn load_from_path(path: &Path) -> Option<Self> {
        Self::read_from_path(path, true)
    }

    fn read_from_path(path: &Path, warn: bool) -> Option<Self> {
        let content = fs::read_to_string(path).ok()?;
        match toml::from_str(&content) {
            Ok(config) => Some(config),
            Err(e) => {
                if warn {
                    // `e.message()` and not `{e}`: the full Display is a
                    // multi-line snippet, which belongs in `pxh doctor`.
                    crate::ui::warn(&format!(
                        "failed to parse {}: {}; using defaults (see 'pxh doctor')",
                        path.display(),
                        e.message()
                    ));
                }
                None
            }
        }
    }

    /// Update the config file at the default path, preserving existing content.
    /// Each update is a (dotted_key, value) pair, e.g. ("host.hostname", value).
    pub fn update_default_config(updates: &[(&str, toml_edit::Item)]) -> Result<()> {
        let path = Self::default_config_path().context("Could not determine config path")?;
        Self::update_config_at_path(&path, updates)
    }

    pub fn update_config_at_path(
        path: &PathBuf,
        updates: &[(&str, toml_edit::Item)],
    ) -> Result<()> {
        // A config file born here starts from the commented template, so the
        // file the user later opens explains every key it does not set.
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => DEFAULT_CONFIG.to_string(),
            Err(e) => return Err(e.into()),
        };
        let mut doc: DocumentMut = content.parse()?;

        for (dotted_key, item) in updates {
            let parts: Vec<&str> = dotted_key.split('.').collect();
            match parts.as_slice() {
                [section, key] => {
                    if !doc.contains_table(section) {
                        doc[section] = toml_edit::Item::Table(toml_edit::Table::new());
                    }
                    doc[section][key] = item.clone();
                }
                [key] => {
                    doc[key] = item.clone();
                }
                _ => bail!("Unsupported key depth: {dotted_key}"),
            }
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, doc.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.recall.keymap, "emacs");
        assert!(config.recall.show_preview);
        assert_eq!(config.recall.result_limit, 5000);
        assert!(config.recall.preview.show_directory);
        assert!(config.recall.preview.show_timestamp);
        assert!(config.recall.preview.show_exit_status);
        assert!(!config.recall.preview.show_hostname);
        assert!(config.recall.preview.show_duration);
        assert!(!config.history.ignore_patterns.is_empty());
        assert!(config.history.ignore_patterns.contains(&"^ls$".to_string()));
    }

    #[test]
    fn test_parse_config() {
        let toml = r#"
[recall]
keymap = "vim"
show_preview = false
result_limit = 1000

[recall.preview]
show_directory = false
show_hostname = true
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.recall.keymap, "vim");
        assert!(!config.recall.show_preview);
        assert_eq!(config.recall.result_limit, 1000);
        assert!(!config.recall.preview.show_directory);
        assert!(config.recall.preview.show_hostname);
        // Defaults should be preserved for unspecified fields
        assert!(config.recall.preview.show_timestamp);
        assert!(config.recall.preview.show_exit_status);
    }

    #[test]
    fn test_default_host_config() {
        let config = Config::default();
        assert!(config.host.hostname.is_none());
        assert!(config.host.machine_id.is_none());
        assert!(config.host.aliases.is_empty());
    }

    #[test]
    fn test_parse_host_config() {
        let toml = r#"
[host]
hostname = "my-old-mac"
machine_id = 12345678901234567
aliases = ["old-mac", "work-laptop"]
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.host.hostname.as_deref(), Some("my-old-mac"));
        assert_eq!(config.host.machine_id, Some(12345678901234567));
        assert_eq!(config.host.aliases, vec!["old-mac", "work-laptop"]);
    }

    #[test]
    fn test_parse_partial_host_config() {
        let toml = r#"
[host]
aliases = ["other-host"]
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.host.hostname.is_none());
        assert!(config.host.machine_id.is_none());
        assert_eq!(config.host.aliases, vec!["other-host"]);
    }

    #[test]
    fn test_update_config_preserves_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"# my comment
[recall]
keymap = "vim"
"#,
        )
        .unwrap();

        Config::update_config_at_path(
            &path,
            &[
                ("host.hostname", toml_edit::value("my-host")),
                ("host.machine_id", toml_edit::value(42_i64)),
            ],
        )
        .unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("# my comment"), "comment preserved");
        assert!(content.contains("keymap = \"vim\""), "existing config preserved");
        assert!(content.contains("hostname = \"my-host\""));
        assert!(content.contains("machine_id = 42"));

        let config = Config::load_from_path(&path).unwrap();
        assert_eq!(config.host.hostname.as_deref(), Some("my-host"));
        assert_eq!(config.recall.keymap, "vim");
    }

    #[test]
    fn test_update_config_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("subdir").join("config.toml");

        Config::update_config_at_path(
            &path,
            &[("host.aliases", toml_edit::value(toml_edit::Array::from_iter(["a", "b"])))],
        )
        .unwrap();

        let config = Config::load_from_path(&path).unwrap();
        assert_eq!(config.host.aliases, vec!["a", "b"]);
    }

    #[test]
    fn test_existing_config_without_host_section() {
        let toml = r#"
[recall]
keymap = "vim"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.host.hostname.is_none());
        assert!(config.host.aliases.is_empty());
        assert_eq!(config.recall.keymap, "vim");
    }

    #[test]
    fn test_parse_history_config() {
        let toml = r#"
[history]
ignore_patterns = ["^secret$", "^rm -rf"]
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.history.ignore_patterns, vec!["^secret$", "^rm -rf"]);
    }

    #[test]
    fn test_parse_empty_history_ignore_patterns() {
        let toml = r#"
[history]
ignore_patterns = []
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.history.ignore_patterns.is_empty());
    }

    #[test]
    fn test_missing_history_section_uses_defaults() {
        let toml = r#"
[recall]
keymap = "vim"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(!config.history.ignore_patterns.is_empty());
        assert!(config.history.ignore_patterns.contains(&"^ls$".to_string()));
    }

    #[test]
    fn test_default_ignore_patterns_are_valid_regexes() {
        let config = HistoryConfig::default();
        let set = regex::RegexSet::new(&config.ignore_patterns);
        assert!(set.is_ok(), "default patterns should all be valid regexes");
    }

    #[test]
    fn test_invalid_toml_returns_none() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("bad.toml");
        fs::write(&path, "this is not valid [[ toml").unwrap();
        assert!(Config::load_from_path(&path).is_none());
    }

    #[test]
    fn test_wrong_type_returns_none() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("bad_type.toml");
        fs::write(&path, "[recall]\nresult_limit = \"not a number\"\n").unwrap();
        assert!(Config::load_from_path(&path).is_none());
    }

    #[test]
    fn test_initial_keymap_mode() {
        let mut config = RecallConfig::default();
        assert_eq!(config.initial_keymap_mode(), KeymapMode::Emacs);

        config.keymap = "vim".to_string();
        assert_eq!(config.initial_keymap_mode(), KeymapMode::VimInsert);

        config.keymap = "VIM".to_string();
        assert_eq!(config.initial_keymap_mode(), KeymapMode::VimInsert);

        config.keymap = "unknown".to_string();
        assert_eq!(config.initial_keymap_mode(), KeymapMode::Emacs);
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let err = toml::from_str::<Config>("[recall]\nkeymap = \"vim\"\nshow_previeww = true\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown field `show_previeww`"), "{err}");
    }

    #[test]
    fn unknown_sections_are_rejected() {
        let err = toml::from_str::<Config>("[recal]\nkeymap = \"vim\"\n").unwrap_err().to_string();
        assert!(err.contains("unknown field `recal`"), "{err}");
    }

    /// The template ships every key, commented, at its default value. If a
    /// default changes and the template does not, this fails.
    #[test]
    fn template_parses_to_defaults() {
        let parsed: Config = toml::from_str(DEFAULT_CONFIG).expect("template is valid");
        assert_eq!(parsed, Config::default());
    }

    #[test]
    fn template_uncommented_parses_to_defaults() {
        // Strip exactly one leading `# ` from commented key lines so the
        // documented values are exercised, not just the comments.
        let uncommented: String = DEFAULT_CONFIG
            .lines()
            .map(|l| l.strip_prefix("# ").filter(|r| r.contains(" = ")).unwrap_or(l))
            .collect::<Vec<_>>()
            .join("\n");
        let parsed: Config = toml::from_str(&uncommented).expect("uncommented template is valid");
        assert_eq!(parsed, Config::default());
    }

    #[test]
    fn config_status_distinguishes_missing_invalid_and_valid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        assert!(matches!(config_status(&path), ConfigStatus::NotFound));

        fs::write(&path, "[recall]\nkeymap = ").unwrap();
        assert!(matches!(config_status(&path), ConfigStatus::Invalid(_)));

        fs::write(&path, "[recall]\nnope = 1\n").unwrap();
        match config_status(&path) {
            ConfigStatus::Invalid(msg) => assert!(msg.contains("unknown field `nope`"), "{msg}"),
            other => panic!("expected Invalid, got {other:?}"),
        }

        fs::write(&path, "[recall]\nkeymap = \"vim\"\n").unwrap();
        match config_status(&path) {
            ConfigStatus::Valid(cfg) => assert_eq!(cfg.recall.keymap, "vim"),
            other => panic!("expected Valid, got {other:?}"),
        }
    }
}
