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
    #[serde(default = "default_detail_panel_mode")]
    pub detail_panel_mode: String,
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

fn default_detail_panel_mode() -> String {
    "metadata".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            default_tool: default_tool(),
            theme: default_theme(),
            default_group: default_group(),
            notifications: NotificationConfig::default(),
            detail_panel_mode: default_detail_panel_mode(),
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

/// Load config from a specific path. Returns defaults if file doesn't exist or fails to parse.
pub fn load_config_from_path(path: &std::path::Path) -> AppConfig {
    match fs::read_to_string(path) {
        Ok(content) => serde_json::from_str::<AppConfig>(&content).unwrap_or_default(),
        Err(_) => AppConfig::default(),
    }
}

/// Load config from disk, merging with defaults.
/// Returns defaults if file doesn't exist or fails to parse.
pub fn load_config() -> AppConfig {
    load_config_from_path(&config_path())
}

/// Save config to disk at the default config path.
pub fn save_config(config: &AppConfig) -> Result<(), std::io::Error> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(config).map_err(std::io::Error::other)?;
    fs::write(&path, json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_config_with_detail_panel_mode() {
        let json = r#"{ "detail_panel_mode": "preview" }"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.detail_panel_mode, "preview");
    }

    #[test]
    fn test_default_detail_panel_mode_is_metadata() {
        let config = AppConfig::default();
        assert_eq!(config.detail_panel_mode, "metadata");
    }

    #[test]
    fn test_parse_detail_panel_mode_all_variants() {
        for mode in &["none", "preview", "metadata", "both"] {
            let json = format!(r#"{{ "detail_panel_mode": "{}" }}"#, mode);
            let config: AppConfig = serde_json::from_str(&json).unwrap();
            assert_eq!(config.detail_panel_mode, *mode);
        }
    }

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
    fn test_load_config_from_path() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.json");
        fs::write(&path, r#"{ "theme": "light" }"#).unwrap();
        let config = load_config_from_path(&path);
        assert_eq!(config.theme, "light");

        fs::write(&path, r#"{ "theme": "dark" }"#).unwrap();
        let config2 = load_config_from_path(&path);
        assert_eq!(config2.theme, "dark");
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
    fn test_load_config_from_invalid_json_returns_default() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.json");
        fs::write(&path, "this is not json").unwrap();
        let config = load_config_from_path(&path);
        assert_eq!(config.default_tool, "claude");
        assert_eq!(config.theme, "dark");
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.json");

        let original = AppConfig {
            default_tool: "gemini".to_string(),
            theme: "gruvbox".to_string(),
            default_group: "work".to_string(),
            notifications: NotificationConfig { sound: true },
            detail_panel_mode: "preview".to_string(),
        };

        // Write manually using save_config logic (bypass the hardcoded path)
        let json = serde_json::to_string_pretty(&original).unwrap();
        fs::write(&path, json).unwrap();

        let loaded = load_config_from_path(&path);
        assert_eq!(loaded.default_tool, "gemini");
        assert_eq!(loaded.theme, "gruvbox");
        assert_eq!(loaded.default_group, "work");
        assert!(loaded.notifications.sound);
    }

    #[test]
    fn test_serialization_roundtrip_via_json_string() {
        let config = AppConfig {
            default_tool: "codex".to_string(),
            theme: "solarized".to_string(),
            default_group: "research".to_string(),
            notifications: NotificationConfig { sound: false },
            detail_panel_mode: "both".to_string(),
        };
        let json = serde_json::to_string(&config).unwrap();
        let restored: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.default_tool, config.default_tool);
        assert_eq!(restored.theme, config.theme);
        assert_eq!(restored.default_group, config.default_group);
        assert_eq!(restored.notifications.sound, config.notifications.sound);
    }
}
