use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

/// Main configuration struct
#[derive(Debug, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub recall: RecallConfig,
}

/// Configuration for the recall TUI
#[derive(Debug, Deserialize)]
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
#[derive(Debug, Clone, Deserialize)]
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
    /// Load configuration from the default path (~/.pxh/config.toml)
    pub fn load() -> Self {
        Self::load_from_default_path().unwrap_or_default()
    }

    fn load_from_default_path() -> Option<Self> {
        let config_path = Self::default_config_path()?;
        Self::load_from_path(&config_path)
    }

    fn default_config_path() -> Option<PathBuf> {
        let home = home::home_dir()?;
        Some(home.join(".pxh").join("config.toml"))
    }

    fn load_from_path(path: &PathBuf) -> Option<Self> {
        let content = fs::read_to_string(path).ok()?;
        toml::from_str(&content).ok()
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
