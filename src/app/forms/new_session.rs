#[derive(Debug, Clone, PartialEq)]
pub struct NewSessionForm {
    /// The runner this session will use. Cycled via Left/Right when the
    /// runner picker is focused.
    pub runner: crate::types::Tool,
    /// Snapshot of `runner::implemented_tools()` taken at form construction
    /// so per-frame renders don't re-query the runner registry.
    pub runners: Vec<crate::types::Tool>,
    pub role: crate::types::SessionRole,
    pub parent_session_id: Option<String>,
    pub parent_conductors: Vec<(String, String)>,
    pub parent_conductor_index: usize,
    pub title: String,
    pub project_path: String,
    /// 0 = runner, 1 = role, 2 = parent conductor, 3 = title, 4 = project path,
    /// 5 = worktree branch, 6 = base ref, 7 = MCP
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
    pub mcp_catalog: Vec<crate::core::mcp::catalog::McpServerCatalogEntry>,
    pub mcp_expanded: bool,
    pub mcp_selected_row: usize,
    pub mcp_profile_save_name: Option<String>,
}

impl NewSessionForm {
    pub fn new() -> Self {
        let home = dirs::home_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/tmp".to_string());
        Self {
            runner: crate::types::Tool::Claude,
            runners: crate::core::runner::implemented_tools(),
            role: crate::types::SessionRole::Normal,
            parent_session_id: None,
            parent_conductors: Vec::new(),
            parent_conductor_index: 0,
            title: String::new(),
            project_path: home,
            focused_field: 3,
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
            mcp_catalog: Vec::new(),
            mcp_expanded: false,
            mcp_selected_row: 0,
            mcp_profile_save_name: None,
        }
    }

    pub fn from_app_config(config: &crate::core::config::AppConfig) -> Self {
        Self::from_config_and_catalog(config, crate::core::mcp::discover_mcp_server_catalog())
    }

    pub fn from_config_and_catalog(
        config: &crate::core::config::AppConfig,
        catalog: Vec<crate::core::mcp::catalog::McpServerCatalogEntry>,
    ) -> Self {
        let mut form = Self::new();
        form.mcp_profiles = config.mcp_profiles.clone();
        form.mcp_catalog = catalog;
        form.refresh_mcp_servers_for_runner();
        form
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
        self.refresh_mcp_servers_for_runner();
    }

    /// Move the runner selection to the previous entry in `runners`, wrapping.
    pub fn cycle_runner_prev(&mut self) {
        let idx = self.current_runner_index();
        let n = self.runners.len();
        self.runner = self.runners[(idx + n - 1) % n];
        self.refresh_mcp_servers_for_runner();
    }

    pub fn cycle_role_next(&mut self) {
        self.role = match self.role {
            crate::types::SessionRole::Normal => {
                self.clear_parent_selection();
                crate::types::SessionRole::Conductor
            }
            crate::types::SessionRole::Conductor => crate::types::SessionRole::Normal,
        };
    }

    pub fn parent_label(&self) -> String {
        let Some(parent_id) = self.parent_session_id.as_deref() else {
            return "(none)".to_string();
        };

        self.parent_conductors
            .iter()
            .find(|(id, _)| id == parent_id)
            .map(|(_, title)| title.clone())
            .unwrap_or_else(|| "(none)".to_string())
    }

    pub fn select_parent_at_index(&mut self, index: usize) {
        if let Some((id, _)) = self.parent_conductors.get(index) {
            self.parent_session_id = Some(id.clone());
            self.parent_conductor_index = index;
        }
    }

    pub fn cycle_parent_next(&mut self) {
        if self.parent_conductors.is_empty() {
            self.clear_parent_selection();
            return;
        }

        match self.current_parent_index() {
            Some(index) if index + 1 < self.parent_conductors.len() => {
                self.select_parent_at_index(index + 1);
            }
            Some(_) => self.clear_parent_selection(),
            None => self.select_parent_at_index(0),
        }
    }

    pub fn cycle_parent_prev(&mut self) {
        if self.parent_conductors.is_empty() {
            self.clear_parent_selection();
            return;
        }

        match self.current_parent_index() {
            Some(index) if index > 0 => self.select_parent_at_index(index - 1),
            Some(_) => self.clear_parent_selection(),
            None => self.select_parent_at_index(self.parent_conductors.len() - 1),
        }
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
        self.normalize_mcp_selection_to_default_when_all_known_servers_enabled();
    }

    pub fn activate_selected_mcp_row(&mut self) -> Result<(), String> {
        if let Some(profile_id) = self.selected_mcp_profile_id() {
            if self.mcp_selection.profile_id.as_deref() == Some(profile_id.as_str()) {
                self.mcp_selection = crate::core::mcp::McpSelection::default();
                Ok(())
            } else {
                self.apply_mcp_profile(&profile_id)
            }
        } else if let Some(server_id) = self.selected_mcp_server_id() {
            self.toggle_mcp_server(&server_id);
            Ok(())
        } else {
            Ok(())
        }
    }

    pub fn selected_mcp_profile_id(&self) -> Option<String> {
        self.mcp_profiles
            .get(self.mcp_selected_row)
            .map(|profile| profile.id.clone())
    }

    pub fn selected_mcp_server_id(&self) -> Option<String> {
        let server_idx = self.mcp_selected_row.checked_sub(self.mcp_profiles.len())?;
        self.mcp_servers
            .get(server_idx)
            .map(|server| server.id.clone())
    }

    pub fn delete_selected_mcp_profile(&mut self) -> Option<crate::core::mcp::McpProfile> {
        if self.mcp_selected_row >= self.mcp_profiles.len() {
            return None;
        }

        let removed = self.mcp_profiles.remove(self.mcp_selected_row);
        if self.mcp_selection.profile_id.as_deref() == Some(removed.id.as_str()) {
            self.mcp_selection = crate::core::mcp::McpSelection::default();
        }
        let row_count = self.mcp_row_count();
        self.mcp_selected_row = if row_count == 0 {
            0
        } else {
            self.mcp_selected_row.min(row_count - 1)
        };
        self.error = None;
        Some(removed)
    }

    pub fn mcp_row_count(&self) -> usize {
        self.mcp_profiles.len() + self.mcp_servers.len()
    }

    #[allow(dead_code)]
    pub fn apply_mcp_profile(&mut self, id: &str) -> Result<(), String> {
        let selection = crate::core::mcp::profiles::apply_profile(&self.mcp_profiles, id)
            .ok_or_else(|| format!("MCP profile '{}' not found", id))?;
        self.mcp_selection = selection;
        Ok(())
    }

    pub fn begin_save_mcp_profile(&mut self) {
        self.mcp_profile_save_name = Some(String::new());
        self.error = None;
    }

    pub fn cancel_save_mcp_profile(&mut self) {
        self.mcp_profile_save_name = None;
        self.error = None;
    }

    pub fn save_mcp_profile_from_prompt(&mut self) -> Result<crate::core::mcp::McpProfile, String> {
        let name = self
            .mcp_profile_save_name
            .as_deref()
            .unwrap_or_default()
            .trim();
        if name.is_empty() {
            return Err("Profile name is required".to_string());
        }
        let id = unique_profile_id(&self.mcp_profiles, &slugify_profile_name(name));
        let profile = crate::core::mcp::McpProfile {
            id: id.clone(),
            name: name.to_string(),
            selection: selection_without_profile_id(self.mcp_selection.clone()),
        };
        crate::core::mcp::profiles::upsert_profile(&mut self.mcp_profiles, profile.clone());
        self.mcp_selection.profile_id = Some(id);
        self.mcp_profile_save_name = None;
        self.error = None;
        Ok(profile)
    }

    pub fn update_active_mcp_profile(&mut self) -> Result<crate::core::mcp::McpProfile, String> {
        let id = self
            .mcp_selection
            .profile_id
            .clone()
            .ok_or_else(|| "No active MCP profile to update".to_string())?;
        let name = self
            .mcp_profiles
            .iter()
            .find(|profile| profile.id == id)
            .map(|profile| profile.name.clone())
            .ok_or_else(|| format!("MCP profile '{}' not found", id))?;
        let profile = crate::core::mcp::McpProfile {
            id,
            name,
            selection: selection_without_profile_id(self.mcp_selection.clone()),
        };
        crate::core::mcp::profiles::upsert_profile(&mut self.mcp_profiles, profile.clone());
        Ok(profile)
    }

    #[cfg(test)]
    pub fn set_mcp_servers_for_test(&mut self, ids: Vec<String>) {
        self.mcp_catalog = ids
            .into_iter()
            .map(|id| crate::core::mcp::McpServerCatalogEntry::server_level(self.runner, id))
            .collect();
        self.refresh_mcp_servers_for_runner();
    }

    fn refresh_mcp_servers_for_runner(&mut self) {
        self.mcp_servers = self
            .mcp_catalog
            .iter()
            .filter(|server| server.runner == self.runner)
            .cloned()
            .collect();
        let row_count = self.mcp_row_count();
        if row_count == 0 {
            self.mcp_selected_row = 0;
        } else {
            self.mcp_selected_row = self.mcp_selected_row.min(row_count - 1);
        }
    }

    fn normalize_mcp_selection_to_default_when_all_known_servers_enabled(&mut self) {
        if self.mcp_servers.is_empty() || self.mcp_selection.is_all_servers() {
            return;
        }

        let all_known_enabled = self.mcp_servers.iter().all(|server| {
            self.mcp_selection
                .servers
                .iter()
                .find(|selected| selected.id == server.id)
                .map(|selected| selected.enabled && selected.selected_tools.is_none())
                .unwrap_or(true)
        });
        if all_known_enabled {
            self.mcp_selection = crate::core::mcp::McpSelection::default();
        }
    }

    fn current_runner_index(&self) -> usize {
        self.runners
            .iter()
            .position(|t| *t == self.runner)
            .expect("runner is always a member of runners")
    }

    fn current_parent_index(&self) -> Option<usize> {
        let parent_id = self.parent_session_id.as_deref()?;
        self.parent_conductors
            .iter()
            .position(|(id, _)| id == parent_id)
    }

    fn clear_parent_selection(&mut self) {
        self.parent_session_id = None;
        self.parent_conductor_index = 0;
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

fn selection_without_profile_id(
    mut selection: crate::core::mcp::McpSelection,
) -> crate::core::mcp::McpSelection {
    selection.profile_id = None;
    selection
}

fn slugify_profile_name(name: &str) -> String {
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        "profile".to_string()
    } else {
        slug
    }
}

fn unique_profile_id(profiles: &[crate::core::mcp::McpProfile], base: &str) -> String {
    let mut candidate = base.to_string();
    let mut n = 2;
    while profiles.iter().any(|profile| profile.id == candidate) {
        candidate = format!("{base}-{n}");
        n += 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::mcp::{McpProfile, McpSelection, McpServerSelection};
    use crate::types::{SessionRole, Tool};

    #[test]
    fn test_form_default_runner_is_claude_with_title_focused() {
        let f = NewSessionForm::new();
        assert_eq!(f.runner, Tool::Claude);
        assert_eq!(f.role, SessionRole::Normal);
        assert_eq!(f.parent_session_id, None);
        // Title is focused by default so the existing keystroke flow
        // — open overlay, start typing — still works. Users reach the runner
        // picker via Shift-Tab or Up.
        assert_eq!(f.focused_field, 3);
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
    fn test_new_session_form_cycles_role() {
        let mut f = NewSessionForm::new();
        f.parent_conductors = vec![("parent-1".into(), "Conductor One".into())];
        f.select_parent_at_index(0);

        assert_eq!(f.role, SessionRole::Normal);
        assert_eq!(f.parent_session_id.as_deref(), Some("parent-1"));

        f.cycle_role_next();

        assert_eq!(f.role, SessionRole::Conductor);
        assert_eq!(f.parent_session_id, None);
        assert_eq!(f.parent_label(), "(none)");

        f.cycle_role_next();

        assert_eq!(f.role, SessionRole::Normal);
        assert_eq!(f.parent_session_id, None);
    }

    #[test]
    fn test_new_session_form_selects_parent_conductor() {
        let mut f = NewSessionForm::new();
        f.parent_conductors = vec![
            ("parent-1".into(), "Conductor One".into()),
            ("parent-2".into(), "Conductor Two".into()),
        ];

        assert_eq!(f.parent_label(), "(none)");

        f.select_parent_at_index(1);

        assert_eq!(f.parent_session_id.as_deref(), Some("parent-2"));
        assert_eq!(f.parent_conductor_index, 1);
        assert_eq!(f.parent_label(), "Conductor Two");

        f.select_parent_at_index(99);

        assert_eq!(f.parent_session_id.as_deref(), Some("parent-2"));
        assert_eq!(f.parent_conductor_index, 1);
        assert_eq!(f.parent_label(), "Conductor Two");
    }

    #[test]
    fn test_new_session_form_cycles_parent_conductor() {
        let mut f = NewSessionForm::new();
        f.parent_conductors = vec![
            ("parent-1".into(), "Conductor One".into()),
            ("parent-2".into(), "Conductor Two".into()),
        ];

        f.cycle_parent_next();
        assert_eq!(f.parent_session_id.as_deref(), Some("parent-1"));
        assert_eq!(f.parent_label(), "Conductor One");

        f.cycle_parent_next();
        assert_eq!(f.parent_session_id.as_deref(), Some("parent-2"));
        assert_eq!(f.parent_label(), "Conductor Two");

        f.cycle_parent_next();
        assert_eq!(f.parent_session_id, None);
        assert_eq!(f.parent_label(), "(none)");

        f.cycle_parent_prev();
        assert_eq!(f.parent_session_id.as_deref(), Some("parent-2"));
        assert_eq!(f.parent_label(), "Conductor Two");
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
    fn test_form_from_config_and_catalog_populates_profiles_and_current_runner_servers() {
        let config = crate::core::config::AppConfig {
            mcp_profiles: vec![McpProfile {
                id: "rust".into(),
                name: "Rust".into(),
                selection: McpSelection {
                    profile_id: None,
                    servers: vec![McpServerSelection {
                        id: "GitLabMITRE".into(),
                        enabled: true,
                        selected_tools: None,
                    }],
                },
            }],
            ..Default::default()
        };
        let catalog = vec![
            crate::core::mcp::McpServerCatalogEntry::server_level(Tool::Claude, "claude-gitlab"),
            crate::core::mcp::McpServerCatalogEntry::server_level(Tool::Codex, "codex-gitlab"),
        ];

        let f = NewSessionForm::from_config_and_catalog(&config, catalog);

        assert_eq!(f.mcp_profiles.len(), 1);
        assert_eq!(f.mcp_profiles[0].id, "rust");
        assert_eq!(f.mcp_servers.len(), 1);
        assert_eq!(f.mcp_servers[0].id, "claude-gitlab");
    }

    #[test]
    fn test_cycle_runner_refreshes_mcp_servers_for_selected_runner() {
        let catalog = vec![
            crate::core::mcp::McpServerCatalogEntry::server_level(Tool::Claude, "claude-gitlab"),
            crate::core::mcp::McpServerCatalogEntry::server_level(Tool::Codex, "codex-gitlab"),
        ];
        let mut f = NewSessionForm::from_config_and_catalog(
            &crate::core::config::AppConfig::default(),
            catalog,
        );

        while f.runner != Tool::Codex {
            f.cycle_runner_next();
        }

        assert_eq!(f.mcp_servers.len(), 1);
        assert_eq!(f.mcp_servers[0].id, "codex-gitlab");
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
    fn test_toggle_normalizes_all_enabled_selection_back_to_default() {
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

        assert!(f.mcp_selection.is_all_servers());
        assert_eq!(f.mcp_selection.profile_id, None);
    }

    #[test]
    fn test_apply_profile_then_toggle_disabled_known_server_normalizes_to_default() {
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

        assert!(f.mcp_selection.is_all_servers());
        assert_eq!(f.mcp_selection.profile_id, None);
    }
}
