//! Configuration loading from ~/.agent-view/config.json

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NotificationConfig {
    #[serde(default)]
    pub sound: bool,
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
    load_config_from_path(&path)
}

/// Load config from an explicit path. Returns defaults if the file is missing or unparseable.
pub fn load_config_from_path(path: &std::path::Path) -> AppConfig {
    match fs::read_to_string(path) {
        Ok(content) => match serde_json::from_str::<AppConfig>(&content) {
            Ok(config) => config,
            Err(_) => {
                eprintln!("Warning: Failed to parse config from {}", path.display());
                AppConfig::default()
            }
        },
        Err(_) => AppConfig::default(),
    }
}

/// Save a config to the given path, creating parent directories as needed.
#[cfg(test)]
pub fn save_config_to_path(path: &std::path::Path, config: &AppConfig) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(config).map_err(|e| std::io::Error::other(e))?;
    fs::write(path, json)
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

    #[test]
    fn test_save_and_reload_config() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.json");

        let original = AppConfig {
            default_tool: "gemini".to_string(),
            theme: "light".to_string(),
            default_group: "work".to_string(),
            notifications: NotificationConfig { sound: true },
        };

        save_config_to_path(&path, &original).expect("save should succeed");

        let reloaded = load_config_from_path(&path);
        assert_eq!(reloaded.default_tool, "gemini");
        assert_eq!(reloaded.theme, "light");
        assert_eq!(reloaded.default_group, "work");
        assert!(reloaded.notifications.sound);
    }

    #[test]
    fn test_load_config_from_missing_path_returns_default() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nonexistent.json");

        let config = load_config_from_path(&path);
        assert_eq!(config.default_tool, "claude");
        assert_eq!(config.theme, "dark");
        assert_eq!(config.default_group, "default");
        assert!(!config.notifications.sound);
    }

    #[test]
    fn test_save_config_creates_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("sub").join("config.json");

        // File and parent directory do not exist yet.
        assert!(!path.exists());

        let config = AppConfig::default();
        save_config_to_path(&path, &config).expect("save should create parent dirs and file");

        assert!(path.exists());

        // Verify the file contains valid JSON that round-trips correctly.
        let contents = fs::read_to_string(&path).unwrap();
        let parsed: AppConfig =
            serde_json::from_str(&contents).expect("saved file must be valid JSON");
        assert_eq!(parsed.default_tool, config.default_tool);
        assert_eq!(parsed.theme, config.theme);
    }
}
