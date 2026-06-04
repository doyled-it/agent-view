use crate::core::mcp::McpSelection;
use crate::core::runner::RunnerLaunch;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn build_claude_mcp_launch(
    session_id: &str,
    selection: Option<&McpSelection>,
    config_dir_override: Option<&Path>,
) -> Result<RunnerLaunch, String> {
    if selection.map(McpSelection::is_all_servers).unwrap_or(true) {
        return Ok(default_claude_launch());
    }

    let selection = selection.expect("narrowed selection checked above");
    let config_dir = config_dir_override
        .map(Path::to_path_buf)
        .unwrap_or_else(crate::core::paths::mcp_session_config_dir);

    if selection.servers.iter().any(|server| server.enabled) {
        let source_path = claude_settings_path()?;
        let source_settings = read_source_settings(&source_path)?;
        build_claude_mcp_launch_from_source_json(
            session_id,
            Some(selection),
            &config_dir,
            &source_settings,
        )
    } else {
        build_claude_mcp_launch_from_source_json(
            session_id,
            Some(selection),
            &config_dir,
            &Value::Object(Map::new()),
        )
    }
}

fn build_claude_mcp_launch_from_source_json(
    session_id: &str,
    selection: Option<&McpSelection>,
    config_dir: &Path,
    source_settings: &Value,
) -> Result<RunnerLaunch, String> {
    if selection.map(McpSelection::is_all_servers).unwrap_or(true) {
        return Ok(default_claude_launch());
    }

    let selection = selection.expect("narrowed selection checked above");
    validate_session_id(session_id)?;
    let mcp_servers = selected_mcp_servers(selection, source_settings)?;
    create_private_config_dir(config_dir)?;

    let config_path = session_config_path(config_dir, session_id);
    let config = serde_json::json!({ "mcpServers": mcp_servers });
    let bytes = serde_json::to_vec_pretty(&config)
        .map_err(|e| format!("failed to encode Claude MCP config: {}", e))?;
    std::fs::write(&config_path, bytes)
        .map_err(|e| format!("failed to write Claude MCP config: {}", e))?;

    Ok(RunnerLaunch {
        command: Some(format!(
            "claude --mcp-config {} --strict-mcp-config",
            shell_quote_path(&config_path)
        )),
        env: HashMap::new(),
        warning: None,
    })
}

fn claude_settings_path() -> Result<PathBuf, String> {
    let config_dir = crate::core::runner::claude::hooks::claude_config_dir()
        .ok_or_else(|| "no home directory for Claude settings.json".to_string())?;
    Ok(config_dir.join("settings.json"))
}

fn read_source_settings(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        format!(
            "failed to read Claude settings.json at {}: {}",
            path.display(),
            e
        )
    })?;
    serde_json::from_str(&text).map_err(|e| {
        format!(
            "failed to parse Claude settings.json at {}: {}",
            path.display(),
            e
        )
    })
}

fn selected_mcp_servers(
    selection: &McpSelection,
    source_settings: &Value,
) -> Result<Map<String, Value>, String> {
    let mut selected = Map::new();
    for server in selection.servers.iter().filter(|server| server.enabled) {
        let source_servers = source_settings
            .get("mcpServers")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                format!(
                    "enabled Claude MCP server '{}' not found in settings.json mcpServers",
                    server.id
                )
            })?;
        let source_definition = source_servers.get(&server.id).ok_or_else(|| {
            format!(
                "enabled Claude MCP server '{}' not found in settings.json mcpServers",
                server.id
            )
        })?;
        selected.insert(server.id.clone(), source_definition.clone());
    }
    Ok(selected)
}

fn validate_session_id(session_id: &str) -> Result<(), String> {
    if session_id.is_empty() {
        return Err("invalid session id for Claude MCP config: empty".to_string());
    }
    if session_id.contains('/') || session_id.contains('\\') || session_id.contains("..") {
        return Err(format!(
            "invalid session id for Claude MCP config: '{}'",
            session_id
        ));
    }
    if !session_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(format!(
            "invalid session id for Claude MCP config: '{}'",
            session_id
        ));
    }
    Ok(())
}

fn create_private_config_dir(config_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(config_dir)
        .map_err(|e| format!("failed to create Claude MCP config dir: {}", e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(config_dir)
            .map_err(|e| format!("failed to inspect Claude MCP config dir: {}", e))?
            .permissions();
        perms.set_mode(0o700);
        std::fs::set_permissions(config_dir, perms)
            .map_err(|e| format!("failed to set Claude MCP config dir permissions: {}", e))?;
    }
    Ok(())
}

fn default_claude_launch() -> RunnerLaunch {
    RunnerLaunch {
        command: Some("claude".to_string()),
        env: HashMap::new(),
        warning: None,
    }
}

fn session_config_path(config_dir: &Path, session_id: &str) -> PathBuf {
    config_dir.join(format!("{}-claude-mcp.json", session_id))
}

fn shell_quote_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-'))
    {
        value.into_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}

#[cfg(test)]
mod tests {
    use crate::core::mcp::{McpSelection, McpServerSelection};
    use serde_json::json;
    use serde_json::Value;
    use std::fs;

    fn narrowed_selection() -> McpSelection {
        McpSelection {
            profile_id: Some("minimal".to_string()),
            servers: vec![
                McpServerSelection {
                    id: "GitLabMITRE".to_string(),
                    enabled: true,
                    selected_tools: None,
                },
                McpServerSelection {
                    id: "browser".to_string(),
                    enabled: false,
                    selected_tools: None,
                },
            ],
        }
    }

    #[test]
    fn none_selection_launches_default_claude() {
        let dir = tempfile::tempdir().unwrap();

        let launch = super::build_claude_mcp_launch("session-123", None, Some(dir.path())).unwrap();

        assert_eq!(launch.command.as_deref(), Some("claude"));
        assert!(launch.env.is_empty());
        assert_eq!(launch.warning, None);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn all_servers_selection_launches_default_claude() {
        let dir = tempfile::tempdir().unwrap();
        let selection = McpSelection::default();

        let launch =
            super::build_claude_mcp_launch("session-123", Some(&selection), Some(dir.path()))
                .unwrap();

        assert_eq!(launch.command.as_deref(), Some("claude"));
        assert!(launch.env.is_empty());
        assert_eq!(launch.warning, None);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn narrowed_selection_writes_session_config_and_enables_strict_launch() {
        let dir = tempfile::tempdir().unwrap();
        let source = json!({
            "mcpServers": {
                "GitLabMITRE": {
                    "type": "http",
                    "url": "https://gitlab.example.test/api/v4/mcp"
                }
            }
        });
        let selection = narrowed_selection();

        let launch = super::build_claude_mcp_launch_from_source_json(
            "session-123",
            Some(&selection),
            dir.path(),
            &source,
        )
        .unwrap();

        let config_path = dir.path().join("session-123-claude-mcp.json");
        assert!(config_path.exists());
        let json: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
        assert!(json.get("mcpServers").and_then(Value::as_object).is_some());

        let command = launch.command.as_deref().unwrap();
        assert!(command.starts_with("claude --mcp-config "));
        assert!(command.contains(config_path.to_str().unwrap()));
        assert!(command.ends_with(" --strict-mcp-config"));
        assert!(launch.env.is_empty());
        assert_eq!(launch.warning, None);
    }

    #[test]
    fn narrowed_selection_copies_enabled_server_definition_from_source_settings() {
        let dir = tempfile::tempdir().unwrap();
        let source = json!({
            "mcpServers": {
                "GitLabMITRE": {
                    "type": "http",
                    "url": "https://gitlab.example.test/api/v4/mcp",
                    "headers": {
                        "X-Test": "1"
                    }
                },
                "browser": {
                    "command": "npx",
                    "args": ["@playwright/mcp"]
                }
            }
        });
        let selection = narrowed_selection();

        super::build_claude_mcp_launch_from_source_json(
            "session-123",
            Some(&selection),
            dir.path(),
            &source,
        )
        .unwrap();

        let config_path = dir.path().join("session-123-claude-mcp.json");
        let json: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
        let servers = json.get("mcpServers").and_then(Value::as_object).unwrap();

        assert_eq!(servers.len(), 1);
        assert_eq!(
            servers.get("GitLabMITRE").unwrap(),
            source
                .get("mcpServers")
                .unwrap()
                .get("GitLabMITRE")
                .unwrap()
        );
        assert!(!servers.contains_key("browser"));
    }

    #[test]
    fn missing_enabled_server_returns_config_error() {
        let dir = tempfile::tempdir().unwrap();
        let source = json!({ "mcpServers": {} });
        let selection = narrowed_selection();

        let err = super::build_claude_mcp_launch_from_source_json(
            "session-123",
            Some(&selection),
            dir.path(),
            &source,
        )
        .unwrap_err();

        assert!(err.contains("GitLabMITRE"), "{err}");
        assert!(err.contains("settings.json"), "{err}");
        assert!(!dir.path().join("session-123-claude-mcp.json").exists());
    }

    #[test]
    fn disabled_all_selection_writes_empty_strict_config() {
        let dir = tempfile::tempdir().unwrap();
        let source = json!({ "mcpServers": {} });
        let selection = McpSelection {
            profile_id: Some("disabled".to_string()),
            servers: vec![
                McpServerSelection {
                    id: "GitLabMITRE".to_string(),
                    enabled: false,
                    selected_tools: None,
                },
                McpServerSelection {
                    id: "browser".to_string(),
                    enabled: false,
                    selected_tools: None,
                },
            ],
        };

        super::build_claude_mcp_launch_from_source_json(
            "session-123",
            Some(&selection),
            dir.path(),
            &source,
        )
        .unwrap();

        let config_path = dir.path().join("session-123-claude-mcp.json");
        let json: Value = serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
        let servers = json.get("mcpServers").and_then(Value::as_object).unwrap();
        assert!(servers.is_empty());
    }

    #[test]
    fn unsafe_session_id_is_rejected_before_writing_config() {
        let temp = tempfile::tempdir().unwrap();
        let config_dir = temp.path().join("configs");
        let source = json!({ "mcpServers": {} });
        let selection = McpSelection {
            profile_id: Some("disabled".to_string()),
            servers: vec![McpServerSelection {
                id: "GitLabMITRE".to_string(),
                enabled: false,
                selected_tools: None,
            }],
        };

        let err = super::build_claude_mcp_launch_from_source_json(
            "../escape",
            Some(&selection),
            &config_dir,
            &source,
        )
        .unwrap_err();

        assert!(err.contains("invalid session id"), "{err}");
        assert!(!temp.path().join("escape-claude-mcp.json").exists());
        assert!(!config_dir.exists());
    }

    #[cfg(unix)]
    #[test]
    fn generated_config_dir_uses_private_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let config_dir = temp.path().join("configs");
        let source = json!({ "mcpServers": {} });
        let selection = McpSelection {
            profile_id: Some("disabled".to_string()),
            servers: vec![McpServerSelection {
                id: "GitLabMITRE".to_string(),
                enabled: false,
                selected_tools: None,
            }],
        };

        super::build_claude_mcp_launch_from_source_json(
            "session-123",
            Some(&selection),
            &config_dir,
            &source,
        )
        .unwrap();

        let mode = fs::metadata(&config_dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }
}
