use crate::core::mcp::{McpSelection, McpServerCatalogEntry, McpServerSelection};
use crate::core::runner::{RunnerLaunch, RunnerLaunchError};
use std::collections::{HashMap, HashSet};
use std::fs;

const UNSUPPORTED_MCP_TOOL_FILTERING_MESSAGE: &str =
    "Codex MCP tool filtering is not enforceable yet; select entire MCP servers only";

pub fn build_codex_mcp_launch(
    selection: Option<&McpSelection>,
) -> Result<RunnerLaunch, RunnerLaunchError> {
    let catalog = if selection_requires_catalog(selection) {
        Some(read_codex_mcp_catalog()?)
    } else {
        None
    };
    build_codex_mcp_launch_with_catalog(selection, catalog.as_deref())
}

pub(crate) fn build_codex_mcp_launch_with_catalog(
    selection: Option<&McpSelection>,
    catalog: Option<&[McpServerCatalogEntry]>,
) -> Result<RunnerLaunch, RunnerLaunchError> {
    let Some(selection) = selection else {
        return Ok(default_codex_launch());
    };

    if selection.is_all_servers() {
        return Ok(default_codex_launch());
    }

    if selection
        .servers
        .iter()
        .any(|server| server.selected_tools.is_some())
    {
        return Err(RunnerLaunchError::Unsupported(
            UNSUPPORTED_MCP_TOOL_FILTERING_MESSAGE.to_string(),
        ));
    }

    let selected = dedupe_server_selections(selection);
    let disabled_server_ids = if selected.iter().any(|server| server.enabled) {
        disabled_server_ids_from_catalog(&selected, catalog)?
    } else {
        selected
            .iter()
            .filter(|server| !server.enabled)
            .map(|server| server.id.clone())
            .collect()
    };

    if disabled_server_ids.is_empty() {
        return Ok(default_codex_launch());
    }

    let mut disabled_args = Vec::new();
    for server_id in disabled_server_ids {
        validate_server_id(&server_id)?;
        disabled_args.push(format!("-c mcp_servers.{}.enabled=false", server_id));
    }

    Ok(RunnerLaunch {
        command: Some(format!("codex {}", disabled_args.join(" "))),
        env: HashMap::new(),
        warning: None,
    })
}

fn selection_requires_catalog(selection: Option<&McpSelection>) -> bool {
    let Some(selection) = selection else {
        return false;
    };

    !selection.is_all_servers()
        && selection
            .servers
            .iter()
            .all(|server| server.selected_tools.is_none())
        && selection.servers.iter().any(|server| server.enabled)
}

fn dedupe_server_selections(selection: &McpSelection) -> Vec<McpServerSelection> {
    let mut seen = HashSet::new();
    selection
        .servers
        .iter()
        .filter(|server| seen.insert(server.id.as_str()))
        .cloned()
        .collect()
}

fn disabled_server_ids_from_catalog(
    selected: &[McpServerSelection],
    catalog: Option<&[McpServerCatalogEntry]>,
) -> Result<Vec<String>, RunnerLaunchError> {
    let catalog = catalog.ok_or_else(|| {
        RunnerLaunchError::Config(
            "enabled Codex MCP server selections require the configured Codex MCP server catalog"
                .to_string(),
        )
    })?;
    let catalog = dedupe_catalog_servers(catalog);
    let catalog_ids: HashSet<&str> = catalog.iter().map(|server| server.id.as_str()).collect();
    for server in selected {
        validate_server_id(&server.id)?;
    }

    let included_ids: HashSet<&str> = selected
        .iter()
        .filter(|server| server.enabled && catalog_ids.contains(server.id.as_str()))
        .map(|server| server.id.as_str())
        .collect();
    Ok(catalog
        .into_iter()
        .filter(|server| !included_ids.contains(server.id.as_str()))
        .map(|server| server.id)
        .collect())
}

fn dedupe_catalog_servers(catalog: &[McpServerCatalogEntry]) -> Vec<McpServerCatalogEntry> {
    let mut seen = HashSet::new();
    catalog
        .iter()
        .filter(|server| seen.insert(server.id.as_str()))
        .cloned()
        .collect()
}

fn read_codex_mcp_catalog() -> Result<Vec<McpServerCatalogEntry>, RunnerLaunchError> {
    let config_dir = crate::core::runner::codex::hooks::codex_config_dir().ok_or_else(|| {
        RunnerLaunchError::Config("no home directory for Codex config".to_string())
    })?;
    let config_path = config_dir.join("config.toml");
    let text = fs::read_to_string(&config_path).map_err(|e| {
        RunnerLaunchError::Config(format!(
            "failed to read Codex config.toml at {}: {}",
            config_path.display(),
            e
        ))
    })?;
    crate::core::mcp::parse_codex_mcp_servers(&text).map_err(RunnerLaunchError::Config)
}

fn default_codex_launch() -> RunnerLaunch {
    RunnerLaunch {
        command: Some("codex".to_string()),
        env: HashMap::new(),
        warning: None,
    }
}

fn validate_server_id(id: &str) -> Result<(), RunnerLaunchError> {
    let is_safe = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if is_safe {
        Ok(())
    } else {
        Err(RunnerLaunchError::Config(format!(
            "unsafe MCP server id '{}' for Codex config override; expected [A-Za-z0-9_-]+",
            id
        )))
    }
}

#[cfg(test)]
mod tests {
    use crate::core::mcp::catalog::McpServerCatalogEntry;
    use crate::core::mcp::{McpSelection, McpServerSelection};
    use crate::core::runner::RunnerLaunchError;
    use crate::types::Tool;

    #[test]
    fn default_selection_launches_plain_codex() {
        let launch = super::build_codex_mcp_launch(None).unwrap();

        assert_eq!(launch.command.as_deref(), Some("codex"));
        assert!(launch.env.is_empty());
        assert_eq!(launch.warning, None);

        let selection = McpSelection::default();
        let catalog = vec![McpServerCatalogEntry::server_level(
            Tool::Codex,
            "GitLabMITRE",
        )];
        let launch =
            super::build_codex_mcp_launch_with_catalog(Some(&selection), Some(&catalog)).unwrap();

        assert_eq!(launch.command.as_deref(), Some("codex"));
        assert!(launch.env.is_empty());
        assert_eq!(launch.warning, None);
    }

    #[test]
    fn enabled_allowlist_disables_omitted_catalog_servers() {
        let selection = McpSelection {
            profile_id: Some("gitlab-only".to_string()),
            servers: vec![McpServerSelection {
                id: "GitLabMITRE".to_string(),
                enabled: true,
                selected_tools: None,
            }],
        };
        let catalog = vec![
            McpServerCatalogEntry::server_level(Tool::Codex, "GitLabMITRE"),
            McpServerCatalogEntry::server_level(Tool::Codex, "browser"),
        ];

        let launch =
            super::build_codex_mcp_launch_with_catalog(Some(&selection), Some(&catalog)).unwrap();

        assert_eq!(
            launch.command.as_deref(),
            Some("codex -c mcp_servers.browser.enabled=false")
        );
    }

    #[test]
    fn enabled_allowlist_requires_catalog_to_avoid_widening_access() {
        let selection = McpSelection {
            profile_id: Some("gitlab-only".to_string()),
            servers: vec![McpServerSelection {
                id: "GitLabMITRE".to_string(),
                enabled: true,
                selected_tools: None,
            }],
        };

        let err = super::build_codex_mcp_launch_with_catalog(Some(&selection), None).unwrap_err();

        match err {
            RunnerLaunchError::Config(message) => {
                assert!(
                    message.contains("require the configured Codex MCP server catalog"),
                    "{message}"
                );
            }
            other => panic!("expected config error, got {other:?}"),
        }
    }

    #[test]
    fn enabled_allowlist_ignores_missing_selected_servers() {
        let selection = McpSelection {
            profile_id: Some("gitlab-only".to_string()),
            servers: vec![
                McpServerSelection {
                    id: "GitLabMITRE".to_string(),
                    enabled: true,
                    selected_tools: None,
                },
                McpServerSelection {
                    id: "retired".to_string(),
                    enabled: true,
                    selected_tools: None,
                },
            ],
        };
        let catalog = vec![
            McpServerCatalogEntry::server_level(Tool::Codex, "GitLabMITRE"),
            McpServerCatalogEntry::server_level(Tool::Codex, "browser"),
        ];

        let launch =
            super::build_codex_mcp_launch_with_catalog(Some(&selection), Some(&catalog)).unwrap();

        assert_eq!(
            launch.command.as_deref(),
            Some("codex -c mcp_servers.browser.enabled=false")
        );
    }

    #[test]
    fn disabled_server_adds_codex_config_override() {
        let selection = McpSelection {
            profile_id: Some("no-gitlab".to_string()),
            servers: vec![McpServerSelection {
                id: "GitLabMITRE".to_string(),
                enabled: false,
                selected_tools: None,
            }],
        };

        let launch = super::build_codex_mcp_launch(Some(&selection)).unwrap();

        assert_eq!(
            launch.command.as_deref(),
            Some("codex -c mcp_servers.GitLabMITRE.enabled=false")
        );
        assert!(launch.env.is_empty());
        assert_eq!(launch.warning, None);
    }

    #[test]
    fn disabled_servers_preserve_selection_order() {
        let selection = McpSelection {
            profile_id: Some("ordered".to_string()),
            servers: vec![
                McpServerSelection {
                    id: "browser".to_string(),
                    enabled: false,
                    selected_tools: None,
                },
                McpServerSelection {
                    id: "GitLabMITRE".to_string(),
                    enabled: false,
                    selected_tools: None,
                },
            ],
        };

        let launch = super::build_codex_mcp_launch(Some(&selection)).unwrap();

        assert_eq!(
            launch.command.as_deref(),
            Some(
                "codex -c mcp_servers.browser.enabled=false -c mcp_servers.GitLabMITRE.enabled=false"
            )
        );
    }

    #[test]
    fn duplicate_disabled_server_emits_one_override() {
        let selection = McpSelection {
            profile_id: Some("duplicate".to_string()),
            servers: vec![
                McpServerSelection {
                    id: "GitLabMITRE".to_string(),
                    enabled: false,
                    selected_tools: None,
                },
                McpServerSelection {
                    id: "GitLabMITRE".to_string(),
                    enabled: false,
                    selected_tools: None,
                },
            ],
        };

        let launch = super::build_codex_mcp_launch(Some(&selection)).unwrap();

        assert_eq!(
            launch.command.as_deref(),
            Some("codex -c mcp_servers.GitLabMITRE.enabled=false")
        );
    }

    #[test]
    fn enabled_then_disabled_duplicate_does_not_disable_server() {
        let selection = McpSelection {
            profile_id: Some("conflict".to_string()),
            servers: vec![
                McpServerSelection {
                    id: "GitLabMITRE".to_string(),
                    enabled: true,
                    selected_tools: None,
                },
                McpServerSelection {
                    id: "GitLabMITRE".to_string(),
                    enabled: false,
                    selected_tools: None,
                },
            ],
        };

        let catalog = vec![McpServerCatalogEntry::server_level(
            Tool::Codex,
            "GitLabMITRE",
        )];
        let launch =
            super::build_codex_mcp_launch_with_catalog(Some(&selection), Some(&catalog)).unwrap();

        assert_eq!(launch.command.as_deref(), Some("codex"));
        assert!(launch.env.is_empty());
        assert_eq!(launch.warning, None);
    }

    #[test]
    fn selected_tools_returns_unsupported_error() {
        let selection = McpSelection {
            profile_id: Some("tools".to_string()),
            servers: vec![McpServerSelection {
                id: "GitLabMITRE".to_string(),
                enabled: true,
                selected_tools: Some(vec!["search".to_string()]),
            }],
        };

        let err = super::build_codex_mcp_launch(Some(&selection)).unwrap_err();

        match err {
            RunnerLaunchError::Unsupported(message) => {
                assert_eq!(message, super::UNSUPPORTED_MCP_TOOL_FILTERING_MESSAGE)
            }
            other => panic!("expected unsupported error, got {other:?}"),
        }
    }

    #[test]
    fn unsafe_server_id_returns_config_error() {
        let selection = McpSelection {
            profile_id: Some("unsafe".to_string()),
            servers: vec![McpServerSelection {
                id: "GitLab;MITRE".to_string(),
                enabled: false,
                selected_tools: None,
            }],
        };

        let err = super::build_codex_mcp_launch(Some(&selection)).unwrap_err();

        match err {
            RunnerLaunchError::Config(message) => {
                assert!(message.contains("unsafe MCP server id"), "{message}");
                assert!(message.contains("GitLab;MITRE"), "{message}");
            }
            other => panic!("expected config error, got {other:?}"),
        }
    }
}
