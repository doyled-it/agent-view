use std::collections::HashSet;

use crate::core::mcp::McpSelection;
use crate::types::Tool;

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerCatalogEntry {
    pub runner: Tool,
    pub id: String,
    pub display_name: String,
    pub server_filter_enforceable: bool,
    pub tool_filter_enforceable: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedMcpSelection {
    pub included_servers: Vec<String>,
    pub disabled_servers: Vec<String>,
    pub missing_servers: Vec<String>,
}

#[allow(dead_code)]
impl McpServerCatalogEntry {
    pub fn server_level(runner: Tool, id: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            runner,
            display_name: id.clone(),
            id,
            server_filter_enforceable: true,
            tool_filter_enforceable: false,
        }
    }
}

#[allow(dead_code)]
pub fn parse_codex_mcp_servers(toml_text: &str) -> Result<Vec<McpServerCatalogEntry>, String> {
    let servers = toml_text
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let name = direct_codex_mcp_server_name(line)?;

            Some(McpServerCatalogEntry::server_level(Tool::Codex, name))
        })
        .collect();

    Ok(servers)
}

fn direct_codex_mcp_server_name(line: &str) -> Option<&str> {
    let name = line
        .strip_prefix("[mcp_servers.")
        .and_then(|value| value.strip_suffix(']'))?;

    if name.is_empty()
        || name.contains('.')
        || name.contains('"')
        || name.contains('\'')
        || name.chars().any(char::is_whitespace)
    {
        None
    } else {
        Some(name)
    }
}

#[allow(dead_code)]
pub fn resolve_selection_against_catalog(
    selection: &McpSelection,
    catalog: &[McpServerCatalogEntry],
) -> ResolvedMcpSelection {
    if selection.servers.is_empty() {
        return ResolvedMcpSelection {
            included_servers: catalog.iter().map(|server| server.id.clone()).collect(),
            disabled_servers: Vec::new(),
            missing_servers: Vec::new(),
        };
    }

    let mut included_servers = Vec::new();
    let mut disabled_servers = Vec::new();
    let mut missing_servers = Vec::new();
    let mut seen_selection_ids = HashSet::new();

    for selected in &selection.servers {
        if !seen_selection_ids.insert(selected.id.as_str()) {
            continue;
        }

        let Some(catalog_entry) = catalog.iter().find(|server| server.id == selected.id) else {
            missing_servers.push(selected.id.clone());
            continue;
        };

        if selected.enabled {
            included_servers.push(catalog_entry.id.clone());
        } else {
            disabled_servers.push(catalog_entry.id.clone());
        }
    }

    ResolvedMcpSelection {
        included_servers,
        disabled_servers,
        missing_servers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::mcp::{McpSelection, McpServerSelection};
    use crate::types::Tool;

    #[test]
    fn codex_config_parser_finds_mcp_servers() {
        let toml = r#"
            [mcp_servers.GitLabMITRE]
            url = "https://gitlab.mitre.org/api/v4/mcp"

            [mcp_servers.browser]
            command = "browser-mcp"
        "#;

        let servers = parse_codex_mcp_servers(toml).unwrap();

        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0].id, "GitLabMITRE");
        assert_eq!(servers[0].display_name, "GitLabMITRE");
        assert_eq!(servers[0].runner, Tool::Codex);
        assert!(servers[0].server_filter_enforceable);
        assert!(!servers[0].tool_filter_enforceable);
        assert_eq!(servers[1].id, "browser");
    }

    #[test]
    fn codex_config_parser_ignores_nested_and_malformed_server_headings() {
        let toml = r#"
            [mcp_servers.gitlab]
            url = "https://gitlab.example.com/api/v4/mcp"

            [mcp_servers.gitlab.env]
            GITLAB_TOKEN = "secret"

            [mcp_servers.gitlab.headers]
            Authorization = "Bearer token"

            [mcp_servers.]
            command = "empty-name"

            [mcp_servers."quoted"]
            command = "quoted-name"
        "#;

        let servers = parse_codex_mcp_servers(toml).unwrap();

        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].id, "gitlab");
    }

    #[test]
    fn selection_resolution_marks_missing_servers() {
        let catalog = vec![McpServerCatalogEntry::server_level(
            Tool::Codex,
            "GitLabMITRE",
        )];
        let selection = McpSelection {
            profile_id: None,
            servers: vec![
                McpServerSelection {
                    id: "GitLabMITRE".into(),
                    enabled: true,
                    selected_tools: None,
                },
                McpServerSelection {
                    id: "missing".into(),
                    enabled: true,
                    selected_tools: None,
                },
            ],
        };

        let resolved = resolve_selection_against_catalog(&selection, &catalog);

        assert_eq!(resolved.included_servers, vec!["GitLabMITRE"]);
        assert_eq!(resolved.disabled_servers, Vec::<String>::new());
        assert_eq!(resolved.missing_servers, vec!["missing"]);
    }

    #[test]
    fn selection_resolution_marks_disabled_known_servers() {
        let catalog = vec![
            McpServerCatalogEntry::server_level(Tool::Codex, "GitLabMITRE"),
            McpServerCatalogEntry::server_level(Tool::Codex, "browser"),
        ];
        let selection = McpSelection {
            profile_id: None,
            servers: vec![
                McpServerSelection {
                    id: "GitLabMITRE".into(),
                    enabled: true,
                    selected_tools: None,
                },
                McpServerSelection {
                    id: "browser".into(),
                    enabled: false,
                    selected_tools: None,
                },
            ],
        };

        let resolved = resolve_selection_against_catalog(&selection, &catalog);

        assert_eq!(resolved.included_servers, vec!["GitLabMITRE"]);
        assert_eq!(resolved.disabled_servers, vec!["browser"]);
        assert_eq!(resolved.missing_servers, Vec::<String>::new());
    }

    #[test]
    fn selection_resolution_ignores_duplicate_known_server_entries() {
        let catalog = vec![McpServerCatalogEntry::server_level(Tool::Codex, "browser")];
        let selection = McpSelection {
            profile_id: None,
            servers: vec![
                McpServerSelection {
                    id: "browser".into(),
                    enabled: true,
                    selected_tools: None,
                },
                McpServerSelection {
                    id: "browser".into(),
                    enabled: true,
                    selected_tools: None,
                },
            ],
        };

        let resolved = resolve_selection_against_catalog(&selection, &catalog);

        assert_eq!(resolved.included_servers, vec!["browser"]);
        assert_eq!(resolved.disabled_servers, Vec::<String>::new());
        assert_eq!(resolved.missing_servers, Vec::<String>::new());
    }

    #[test]
    fn selection_resolution_uses_first_entry_for_enabled_disabled_conflicts() {
        let catalog = vec![McpServerCatalogEntry::server_level(Tool::Codex, "browser")];
        let selection = McpSelection {
            profile_id: None,
            servers: vec![
                McpServerSelection {
                    id: "browser".into(),
                    enabled: true,
                    selected_tools: None,
                },
                McpServerSelection {
                    id: "browser".into(),
                    enabled: false,
                    selected_tools: None,
                },
                McpServerSelection {
                    id: "missing".into(),
                    enabled: true,
                    selected_tools: None,
                },
                McpServerSelection {
                    id: "missing".into(),
                    enabled: false,
                    selected_tools: None,
                },
            ],
        };

        let resolved = resolve_selection_against_catalog(&selection, &catalog);

        assert_eq!(resolved.included_servers, vec!["browser"]);
        assert_eq!(resolved.disabled_servers, Vec::<String>::new());
        assert_eq!(resolved.missing_servers, vec!["missing"]);
    }

    #[test]
    fn empty_selection_includes_all_catalog_servers() {
        let catalog = vec![
            McpServerCatalogEntry::server_level(Tool::Codex, "GitLabMITRE"),
            McpServerCatalogEntry::server_level(Tool::Codex, "browser"),
        ];

        let resolved = resolve_selection_against_catalog(&McpSelection::default(), &catalog);

        assert_eq!(resolved.included_servers, vec!["GitLabMITRE", "browser"]);
        assert_eq!(resolved.disabled_servers, Vec::<String>::new());
        assert_eq!(resolved.missing_servers, Vec::<String>::new());
    }
}
