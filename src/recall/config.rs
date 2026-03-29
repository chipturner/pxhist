use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use toml_edit::DocumentMut;

/// Configuration for history recording
#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
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
#[derive(Debug, Deserialize, Serialize, Default)]
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
#[derive(Debug, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct HostConfig {
    pub hostname: Option<String>,
    pub machine_id: Option<u64>,
    pub aliases: Vec<String>,
}

/// Configuration for shell integration
#[derive(Debug, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct ShellConfig {
    /// Disable Ctrl-R binding (keep shell's default behavior)
    pub disable_ctrl_r: bool,
}

/// Configuration for the recall TUI
#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
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
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
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
    /// Load configuration from the default path.
    pub fn load() -> Self {
        Self::load_from_default_path().unwrap_or_default()
    }

    fn load_from_default_path() -> Option<Self> {
        let config_path = Self::default_config_path()?;
        Self::load_from_path(&config_path)
    }

    pub fn default_config_path() -> Option<PathBuf> {
        Some(crate::pxh_config_dir()?.join("config.toml"))
    }

    pub fn load_from_path(path: &PathBuf) -> Option<Self> {
        let content = fs::read_to_string(path).ok()?;
        match toml::from_str(&content) {
            Ok(config) => Some(config),
            Err(e) => {
                eprintln!("pxh: warning: failed to parse {}: {e}", path.display());
                eprintln!("pxh: using default configuration");
                None
            }
        }
    }

    /// Update the config file at the default path, preserving existing content.
    /// Each update is a (dotted_key, value) pair, e.g. ("host.hostname", value).
    pub fn update_default_config(
        updates: &[(&str, toml_edit::Item)],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let path = Self::default_config_path().ok_or("Could not determine config path")?;
        Self::update_config_at_path(&path, updates)
    }

    pub fn update_config_at_path(
        path: &PathBuf,
        updates: &[(&str, toml_edit::Item)],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path).unwrap_or_default();
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
                _ => return Err(format!("Unsupported key depth: {dotted_key}").into()),
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
        std::fs::write(
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

        let content = std::fs::read_to_string(&path).unwrap();
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
        std::fs::write(&path, "this is not valid [[ toml").unwrap();
        assert!(Config::load_from_path(&path).is_none());
    }

    #[test]
    fn test_wrong_type_returns_none() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("bad_type.toml");
        std::fs::write(&path, "[recall]\nresult_limit = \"not a number\"\n").unwrap();
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
}
