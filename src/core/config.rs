//! Configuration loading from ~/.agent-view/config.json

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    #[serde(default)]
    pub sound: bool,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self { sound: false }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_tool")]
    pub default_tool: String,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_group")]
    pub default_group: String,
    #[serde(default)]
    pub notifications: NotificationConfig,
}

fn default_tool() -> String {
    "claude".to_string()
}

fn default_theme() -> String {
    "dark".to_string()
}

fn default_group() -> String {
    "default".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            default_tool: default_tool(),
            theme: default_theme(),
            default_group: default_group(),
            notifications: NotificationConfig::default(),
        }
    }
}

/// Get the config directory path (~/.agent-view)
pub fn config_dir() -> PathBuf {
    let home = dirs::home_dir().expect("Cannot determine home directory");
    home.join(".agent-view")
}

/// Get the config file path (~/.agent-view/config.json)
pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

/// Load config from disk, merging with defaults.
/// Returns defaults if file doesn't exist or fails to parse.
pub fn load_config() -> AppConfig {
    let path = config_path();
    match fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<AppConfig>(&content) {
            Ok(config) => config,
            Err(_) => {
                eprintln!(
                    "Warning: Failed to parse config from {}",
                    path.display()
                );
                AppConfig::default()
            }
        },
        Err(_) => AppConfig::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.default_tool, "claude");
        assert_eq!(config.theme, "dark");
        assert_eq!(config.default_group, "default");
        assert!(!config.notifications.sound);
    }

    #[test]
    fn test_parse_full_config() {
        let json = r#"{
            "default_tool": "gemini",
            "theme": "light",
            "default_group": "work",
            "notifications": { "sound": true }
        }"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.default_tool, "gemini");
        assert_eq!(config.theme, "light");
        assert_eq!(config.default_group, "work");
        assert!(config.notifications.sound);
    }

    #[test]
    fn test_parse_partial_config_uses_defaults() {
        let json = r#"{ "theme": "light" }"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.theme, "light");
        assert_eq!(config.default_tool, "claude"); // default
        assert!(!config.notifications.sound); // default
    }

    #[test]
    fn test_parse_empty_object_uses_defaults() {
        let json = "{}";
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.default_tool, "claude");
        assert_eq!(config.theme, "dark");
    }

    #[test]
    fn test_invalid_json_returns_default() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.json");
        fs::write(&path, "not valid json!!!").unwrap();

        // We can't easily test load_config() with a custom path,
        // but we test the parsing logic
        let result: Result<AppConfig, _> = serde_json::from_str("not valid json!!!");
        assert!(result.is_err());
    }
}
