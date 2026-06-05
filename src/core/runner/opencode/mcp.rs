use crate::core::mcp::McpSelection;
use crate::core::runner::{RunnerLaunch, RunnerLaunchError};
use std::collections::HashMap;

const UNSUPPORTED_MCP_FILTERING_MESSAGE: &str =
    "OpenCode MCP filtering is not enforceable yet; keep all MCP servers selected or use Claude/Codex";

pub fn build_opencode_mcp_launch(
    selection: Option<&McpSelection>,
) -> Result<RunnerLaunch, RunnerLaunchError> {
    let Some(selection) = selection else {
        return Ok(default_opencode_launch());
    };

    if selection.is_all_servers() {
        return Ok(default_opencode_launch());
    }

    Err(RunnerLaunchError::Unsupported(
        UNSUPPORTED_MCP_FILTERING_MESSAGE.to_string(),
    ))
}

fn default_opencode_launch() -> RunnerLaunch {
    RunnerLaunch {
        command: Some("opencode".to_string()),
        env: HashMap::new(),
        warning: None,
    }
}

#[cfg(test)]
mod tests {
    use crate::core::mcp::{McpSelection, McpServerSelection};
    use crate::core::runner::RunnerLaunchError;

    #[test]
    fn default_selection_launches_plain_opencode() {
        let launch = super::build_opencode_mcp_launch(None).unwrap();

        assert_eq!(launch.command.as_deref(), Some("opencode"));
        assert!(launch.env.is_empty());
        assert_eq!(launch.warning, None);

        let selection = McpSelection::default();
        let launch = super::build_opencode_mcp_launch(Some(&selection)).unwrap();

        assert_eq!(launch.command.as_deref(), Some("opencode"));
        assert!(launch.env.is_empty());
        assert_eq!(launch.warning, None);
    }

    #[test]
    fn disabled_server_returns_unsupported_error() {
        let selection = McpSelection {
            profile_id: Some("no-gitlab".to_string()),
            servers: vec![McpServerSelection {
                id: "GitLabMITRE".to_string(),
                enabled: false,
                selected_tools: None,
            }],
        };

        let err = super::build_opencode_mcp_launch(Some(&selection)).unwrap_err();

        match err {
            RunnerLaunchError::Unsupported(message) => {
                assert_eq!(message, super::UNSUPPORTED_MCP_FILTERING_MESSAGE)
            }
            other => panic!("expected unsupported error, got {other:?}"),
        }
    }

    #[test]
    fn selected_tools_returns_unsupported_error_even_when_server_enabled() {
        let selection = McpSelection {
            profile_id: Some("tools".to_string()),
            servers: vec![McpServerSelection {
                id: "GitLabMITRE".to_string(),
                enabled: true,
                selected_tools: Some(vec!["search".to_string()]),
            }],
        };

        let err = super::build_opencode_mcp_launch(Some(&selection)).unwrap_err();

        match err {
            RunnerLaunchError::Unsupported(message) => {
                assert_eq!(message, super::UNSUPPORTED_MCP_FILTERING_MESSAGE)
            }
            other => panic!("expected unsupported error, got {other:?}"),
        }
    }

    #[test]
    fn enabled_server_list_returns_unsupported_error_instead_of_plain_opencode() {
        let selection = McpSelection {
            profile_id: Some("gitlab-only".to_string()),
            servers: vec![McpServerSelection {
                id: "GitLabMITRE".to_string(),
                enabled: true,
                selected_tools: None,
            }],
        };

        let err = super::build_opencode_mcp_launch(Some(&selection)).unwrap_err();

        match err {
            RunnerLaunchError::Unsupported(message) => {
                assert_eq!(message, super::UNSUPPORTED_MCP_FILTERING_MESSAGE)
            }
            other => panic!("expected unsupported error, got {other:?}"),
        }
    }
}
