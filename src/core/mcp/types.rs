use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpSelection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub servers: Vec<McpServerSelection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerSelection {
    pub id: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_tools: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpProfile {
    pub id: String,
    pub name: String,
    pub selection: McpSelection,
}

impl McpSelection {
    #[allow(dead_code)]
    pub fn is_all_servers(&self) -> bool {
        self.servers.is_empty()
    }

    #[allow(dead_code)]
    pub fn selected_server_count(&self) -> usize {
        self.servers.iter().filter(|s| s.enabled).count()
    }

    #[allow(dead_code)]
    pub fn summary(&self) -> String {
        if self.is_all_servers() {
            "All MCP servers".to_string()
        } else if let Some(profile) = &self.profile_id {
            format!(
                "Profile {}; {} servers selected",
                profile,
                self.selected_server_count()
            )
        } else {
            format!("{} servers selected", self.selected_server_count())
        }
    }
}

impl McpServerSelection {
    #[allow(dead_code)]
    pub fn all_tools(&self) -> bool {
        self.selected_tools.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_selection_means_all_servers() {
        let selection = McpSelection::default();
        assert!(selection.is_all_servers());
        assert_eq!(selection.summary(), "All MCP servers");
    }

    #[test]
    fn selected_tools_none_means_all_tools_for_server() {
        let server = McpServerSelection {
            id: "GitLabMITRE".to_string(),
            enabled: true,
            selected_tools: None,
        };
        assert!(server.all_tools());
    }

    #[test]
    fn narrowed_selection_reports_false_for_all_servers() {
        let selection = McpSelection {
            profile_id: Some("rust".to_string()),
            servers: vec![McpServerSelection {
                id: "GitLabMITRE".to_string(),
                enabled: false,
                selected_tools: None,
            }],
        };
        assert!(!selection.is_all_servers());
        assert_eq!(selection.summary(), "Profile rust; 0 servers selected");
    }
}
