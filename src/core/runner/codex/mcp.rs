use crate::core::mcp::McpSelection;
use crate::core::runner::{RunnerLaunch, RunnerLaunchError};
use std::collections::{HashMap, HashSet};

const UNSUPPORTED_MCP_TOOL_FILTERING_MESSAGE: &str =
    "Codex MCP tool filtering is not enforceable yet; select entire MCP servers only";

pub fn build_codex_mcp_launch(
    selection: Option<&McpSelection>,
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

    let mut seen = HashSet::new();
    let mut disabled_args = Vec::new();
    for server in &selection.servers {
        if !seen.insert(&server.id) {
            continue;
        }

        if server.enabled {
            continue;
        }

        validate_server_id(&server.id)?;
        disabled_args.push(format!("-c mcp_servers.{}.enabled=false", server.id));
    }

    if disabled_args.is_empty() {
        return Ok(default_codex_launch());
    }

    Ok(RunnerLaunch {
        command: Some(format!("codex {}", disabled_args.join(" "))),
        env: HashMap::new(),
        warning: None,
    })
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
    use crate::core::mcp::{McpSelection, McpServerSelection};
    use crate::core::runner::RunnerLaunchError;

    #[test]
    fn default_selection_launches_plain_codex() {
        let launch = super::build_codex_mcp_launch(None).unwrap();

        assert_eq!(launch.command.as_deref(), Some("codex"));
        assert!(launch.env.is_empty());
        assert_eq!(launch.warning, None);

        let selection = McpSelection::default();
        let launch = super::build_codex_mcp_launch(Some(&selection)).unwrap();

        assert_eq!(launch.command.as_deref(), Some("codex"));
        assert!(launch.env.is_empty());
        assert_eq!(launch.warning, None);
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

        let launch = super::build_codex_mcp_launch(Some(&selection)).unwrap();

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
