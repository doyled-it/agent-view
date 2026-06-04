#[derive(Debug, Clone, PartialEq)]
pub struct NewSessionForm {
    /// The runner this session will use. Cycled via Left/Right when the
    /// runner picker is focused.
    pub runner: crate::types::Tool,
    /// Snapshot of `runner::implemented_tools()` taken at form construction
    /// so per-frame renders don't re-query the runner registry.
    pub runners: Vec<crate::types::Tool>,
    pub title: String,
    pub project_path: String,
    /// 0 = runner, 1 = title, 2 = project path, 3 = worktree branch, 4 = base ref, 5 = MCP
    pub focused_field: usize,
    pub completions: Vec<String>,
    pub completion_index: Option<usize>,
    /// Directory prefix the path completions were drawn from (preserves the
    /// user's original form, e.g. `~/projects/`). Used to rebuild the path on
    /// each cycle without stripping more segments than intended.
    pub completion_base: String,
    pub worktree_branch: String,
    /// When true, branch is created fresh; when false, attach to an existing
    /// branch. Toggle wired via Ctrl-T in the input handler.
    pub worktree_new_branch: bool,
    pub worktree_base: String,
    /// Inline validation error rendered under the worktree row.
    pub error: Option<String>,
    pub mcp_selection: crate::core::mcp::McpSelection,
    pub mcp_profiles: Vec<crate::core::mcp::McpProfile>,
    pub mcp_servers: Vec<crate::core::mcp::catalog::McpServerCatalogEntry>,
    pub mcp_expanded: bool,
    pub mcp_selected_row: usize,
}

impl NewSessionForm {
    pub fn new() -> Self {
        let home = dirs::home_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/tmp".to_string());
        Self {
            runner: crate::types::Tool::Claude,
            runners: crate::core::runner::implemented_tools(),
            title: String::new(),
            project_path: home,
            focused_field: 1,
            completions: Vec::new(),
            completion_index: None,
            completion_base: String::new(),
            worktree_branch: String::new(),
            worktree_new_branch: true,
            worktree_base: String::new(),
            error: None,
            mcp_selection: crate::core::mcp::McpSelection::default(),
            mcp_profiles: Vec::new(),
            mcp_servers: Vec::new(),
            mcp_expanded: false,
            mcp_selected_row: 0,
        }
    }

    /// Drop any in-flight completion state. Call when changing fields or when
    /// the path is edited so the next Tab refetches from a clean slate.
    pub fn clear_completions(&mut self) {
        self.completions.clear();
        self.completion_index = None;
        self.completion_base.clear();
    }

    /// Move the runner selection to the next entry in `runners`, wrapping.
    pub fn cycle_runner_next(&mut self) {
        let idx = self.current_runner_index();
        self.runner = self.runners[(idx + 1) % self.runners.len()];
    }

    /// Move the runner selection to the previous entry in `runners`, wrapping.
    pub fn cycle_runner_prev(&mut self) {
        let idx = self.current_runner_index();
        let n = self.runners.len();
        self.runner = self.runners[(idx + n - 1) % n];
    }

    #[allow(dead_code)]
    pub fn mcp_summary(&self) -> String {
        self.mcp_selection.summary()
    }

    pub fn toggle_mcp_server(&mut self, id: &str) {
        let is_known_server = self.mcp_servers.iter().any(|server| server.id == id);
        if self.mcp_selection.is_all_servers() {
            if !is_known_server {
                return;
            }

            self.mcp_selection.servers = mcp_server_selections_from_catalog(&self.mcp_servers);
        }

        dedupe_mcp_server_selections(&mut self.mcp_selection.servers);

        if let Some(server) = self
            .mcp_selection
            .servers
            .iter_mut()
            .find(|server| server.id == id)
        {
            server.enabled = !server.enabled;
        } else if is_known_server {
            self.mcp_selection
                .servers
                .push(crate::core::mcp::McpServerSelection {
                    id: id.to_string(),
                    enabled: false,
                    selected_tools: None,
                });
        }
    }

    #[allow(dead_code)]
    pub fn apply_mcp_profile(&mut self, id: &str) -> Result<(), String> {
        let selection = crate::core::mcp::profiles::apply_profile(&self.mcp_profiles, id)
            .ok_or_else(|| format!("MCP profile '{}' not found", id))?;
        self.mcp_selection = selection;
        Ok(())
    }

    #[cfg(test)]
    pub fn set_mcp_servers_for_test(&mut self, ids: Vec<String>) {
        self.mcp_servers = ids
            .into_iter()
            .map(|id| crate::core::mcp::McpServerCatalogEntry::server_level(self.runner, id))
            .collect();
        if self.mcp_servers.is_empty() {
            self.mcp_selected_row = 0;
        } else {
            self.mcp_selected_row = self.mcp_selected_row.min(self.mcp_servers.len() - 1);
        }
    }

    fn current_runner_index(&self) -> usize {
        self.runners
            .iter()
            .position(|t| *t == self.runner)
            .expect("runner is always a member of runners")
    }
}

fn mcp_server_selections_from_catalog(
    servers: &[crate::core::mcp::catalog::McpServerCatalogEntry],
) -> Vec<crate::core::mcp::McpServerSelection> {
    let mut seen = std::collections::HashSet::new();
    servers
        .iter()
        .filter(|server| seen.insert(server.id.as_str()))
        .map(|server| crate::core::mcp::McpServerSelection {
            id: server.id.clone(),
            enabled: true,
            selected_tools: None,
        })
        .collect()
}

fn dedupe_mcp_server_selections(servers: &mut Vec<crate::core::mcp::McpServerSelection>) {
    let mut seen = std::collections::HashSet::new();
    servers.retain(|server| seen.insert(server.id.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::mcp::{McpProfile, McpSelection, McpServerSelection};
    use crate::types::Tool;

    #[test]
    fn test_form_default_runner_is_claude_with_title_focused() {
        let f = NewSessionForm::new();
        assert_eq!(f.runner, Tool::Claude);
        // Title (field 1) is focused by default so the existing keystroke flow
        // — open overlay, start typing — still works. Users reach the runner
        // picker via Shift-Tab or Up.
        assert_eq!(f.focused_field, 1);
    }

    #[test]
    fn test_runners_field_populated_at_construction() {
        let f = NewSessionForm::new();
        assert!(f.runners.contains(&Tool::Claude));
        assert!(f.runners.contains(&Tool::Shell));
        assert!(!f.runners.is_empty());
    }

    #[test]
    fn test_cycle_runner_next_wraps() {
        let mut f = NewSessionForm::new();
        let n = f.runners.len();
        for _ in 0..n {
            f.cycle_runner_next();
        }
        assert_eq!(f.runner, Tool::Claude);
    }

    #[test]
    fn test_cycle_runner_prev_wraps() {
        let mut f = NewSessionForm::new();
        f.cycle_runner_prev();
        assert_eq!(f.runner, *f.runners.last().unwrap());
    }

    #[test]
    fn test_cycle_runner_next_advances_one() {
        let mut f = NewSessionForm::new();
        assert_eq!(f.runner, Tool::Claude);
        f.cycle_runner_next();
        assert_eq!(f.runner, f.runners[1]);
    }

    #[test]
    fn test_form_defaults_to_all_mcp_servers() {
        let f = NewSessionForm::new();

        assert!(f.mcp_selection.is_all_servers());
        assert_eq!(f.mcp_summary(), "All MCP servers");
        assert!(f.mcp_profiles.is_empty());
        assert!(f.mcp_servers.is_empty());
        assert!(!f.mcp_expanded);
        assert_eq!(f.mcp_selected_row, 0);
    }

    #[test]
    fn test_toggle_known_server_from_all_materializes_disabled_selection() {
        let mut f = NewSessionForm::new();
        f.set_mcp_servers_for_test(vec!["GitLabMITRE".into(), "browser".into()]);

        f.toggle_mcp_server("browser");

        assert_eq!(f.mcp_selection.profile_id, None);
        assert_eq!(
            f.mcp_selection.servers,
            vec![
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
            ]
        );
    }

    #[test]
    fn test_toggle_from_all_dedupes_known_server_rows() {
        let mut f = NewSessionForm::new();
        f.set_mcp_servers_for_test(vec![
            "GitLabMITRE".into(),
            "browser".into(),
            "GitLabMITRE".into(),
            "browser".into(),
        ]);

        f.toggle_mcp_server("browser");

        assert_eq!(
            f.mcp_selection.servers,
            vec![
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
            ]
        );
    }

    #[test]
    fn test_toggle_normalizes_duplicate_selection_rows_with_first_entry_wins() {
        let mut f = NewSessionForm::new();
        f.set_mcp_servers_for_test(vec!["browser".into(), "GitLabMITRE".into()]);
        f.mcp_selection = McpSelection {
            profile_id: Some("mixed".into()),
            servers: vec![
                McpServerSelection {
                    id: "browser".into(),
                    enabled: false,
                    selected_tools: None,
                },
                McpServerSelection {
                    id: "browser".into(),
                    enabled: true,
                    selected_tools: None,
                },
                McpServerSelection {
                    id: "GitLabMITRE".into(),
                    enabled: true,
                    selected_tools: None,
                },
                McpServerSelection {
                    id: "GitLabMITRE".into(),
                    enabled: false,
                    selected_tools: None,
                },
            ],
        };

        f.toggle_mcp_server("browser");

        assert_eq!(
            f.mcp_selection.servers,
            vec![
                McpServerSelection {
                    id: "browser".into(),
                    enabled: true,
                    selected_tools: None,
                },
                McpServerSelection {
                    id: "GitLabMITRE".into(),
                    enabled: true,
                    selected_tools: None,
                },
            ]
        );
    }

    #[test]
    fn test_apply_profile_then_toggle_disabled_known_server_reenables_it() {
        let mut f = NewSessionForm::new();
        f.set_mcp_servers_for_test(vec!["GitLabMITRE".into(), "browser".into()]);
        f.mcp_profiles = vec![McpProfile {
            id: "minimal".into(),
            name: "Minimal".into(),
            selection: McpSelection {
                profile_id: None,
                servers: vec![McpServerSelection {
                    id: "browser".into(),
                    enabled: false,
                    selected_tools: None,
                }],
            },
        }];

        f.apply_mcp_profile("minimal").unwrap();
        f.toggle_mcp_server("browser");

        let browser = f
            .mcp_selection
            .servers
            .iter()
            .find(|server| server.id == "browser")
            .unwrap();
        assert_eq!(f.mcp_selection.profile_id.as_deref(), Some("minimal"));
        assert!(browser.enabled);
    }
}
