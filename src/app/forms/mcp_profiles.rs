#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpProfilesMode {
    List,
    Edit(McpProfileEditMode),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpProfileEditMode {
    Create,
    Edit { original_id: String },
    Duplicate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpProfileServerRow {
    pub id: String,
    pub display_name: String,
    pub enabled: bool,
    pub missing: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct McpProfilesForm {
    pub profiles: Vec<crate::core::mcp::McpProfile>,
    pub catalog: Vec<crate::core::mcp::McpServerCatalogEntry>,
    pub mode: McpProfilesMode,
    pub selected_profile: usize,
    pub focused_field: usize,
    pub selected_server: usize,
    pub name_input: String,
    pub selection: crate::core::mcp::McpSelection,
    pub error: Option<String>,
}

impl McpProfilesForm {
    pub fn new(
        profiles: Vec<crate::core::mcp::McpProfile>,
        catalog: Vec<crate::core::mcp::McpServerCatalogEntry>,
    ) -> Self {
        Self {
            profiles,
            catalog,
            mode: McpProfilesMode::List,
            selected_profile: 0,
            focused_field: 0,
            selected_server: 0,
            name_input: String::new(),
            selection: crate::core::mcp::McpSelection::default(),
            error: None,
        }
    }

    pub fn start_create_from_selection(&mut self, selection: crate::core::mcp::McpSelection) {
        self.mode = McpProfilesMode::Edit(McpProfileEditMode::Create);
        self.focused_field = 0;
        self.selected_server = 0;
        self.name_input.clear();
        self.selection = selection_without_profile_id(selection);
        self.error = None;
    }

    pub fn start_edit_selected(&mut self) -> Result<(), String> {
        let profile = self
            .profiles
            .get(self.selected_profile)
            .cloned()
            .ok_or_else(|| "No MCP profile selected".to_string())?;
        self.mode = McpProfilesMode::Edit(McpProfileEditMode::Edit {
            original_id: profile.id,
        });
        self.focused_field = 0;
        self.selected_server = 0;
        self.name_input = profile.name;
        self.selection = selection_without_profile_id(profile.selection);
        self.error = None;
        Ok(())
    }

    pub fn start_duplicate_selected(&mut self) -> Result<(), String> {
        let profile = self
            .profiles
            .get(self.selected_profile)
            .cloned()
            .ok_or_else(|| "No MCP profile selected".to_string())?;
        self.mode = McpProfilesMode::Edit(McpProfileEditMode::Duplicate);
        self.focused_field = 0;
        self.selected_server = 0;
        self.name_input = unique_profile_name(&self.profiles, &format!("{} Copy", profile.name));
        self.selection = selection_without_profile_id(profile.selection);
        self.error = None;
        Ok(())
    }

    pub fn delete_selected(&mut self) -> Option<crate::core::mcp::McpProfile> {
        if self.selected_profile >= self.profiles.len() {
            return None;
        }
        let removed = self.profiles.remove(self.selected_profile);
        if self.selected_profile >= self.profiles.len() {
            self.selected_profile = self.profiles.len().saturating_sub(1);
        }
        Some(removed)
    }

    pub fn server_rows(&self) -> Vec<McpProfileServerRow> {
        let mut rows = Vec::new();
        let mut seen = std::collections::BTreeSet::new();

        for selected in &self.selection.servers {
            if seen.insert(selected.id.clone()) {
                rows.push(McpProfileServerRow {
                    id: selected.id.clone(),
                    display_name: selected.id.clone(),
                    enabled: selected.enabled,
                    missing: true,
                });
            }
        }

        for server in &self.catalog {
            if seen.insert(server.id.clone()) {
                rows.push(McpProfileServerRow {
                    id: server.id.clone(),
                    display_name: server.display_name.clone(),
                    enabled: self.server_enabled(&server.id),
                    missing: false,
                });
            } else if let Some(row) = rows.iter_mut().find(|row| row.id == server.id) {
                row.display_name = server.display_name.clone();
                row.missing = false;
            }
        }

        rows
    }

    pub fn selected_server_id(&self) -> Option<String> {
        self.server_rows()
            .get(self.selected_server)
            .map(|row| row.id.clone())
    }

    pub fn server_row_count(&self) -> usize {
        self.server_rows().len()
    }

    pub fn toggle_server(&mut self, id: &str) {
        if self.selection.is_all_servers() {
            self.selection.servers = self
                .server_rows()
                .into_iter()
                .map(|row| crate::core::mcp::McpServerSelection {
                    id: row.id,
                    enabled: true,
                    selected_tools: None,
                })
                .collect();
        }

        if let Some(server) = self
            .selection
            .servers
            .iter_mut()
            .find(|server| server.id == id)
        {
            server.enabled = !server.enabled;
        } else {
            self.selection
                .servers
                .push(crate::core::mcp::McpServerSelection {
                    id: id.to_string(),
                    enabled: false,
                    selected_tools: None,
                });
        }
    }

    pub fn save_edit(&mut self) -> Result<crate::core::mcp::McpProfile, String> {
        let name = self.name_input.trim();
        if name.is_empty() {
            self.error = Some("Profile name is required".to_string());
            return Err("Profile name is required".to_string());
        }

        let id = match &self.mode {
            McpProfilesMode::Edit(McpProfileEditMode::Edit { original_id }) => original_id.clone(),
            McpProfilesMode::Edit(McpProfileEditMode::Create | McpProfileEditMode::Duplicate) => {
                unique_profile_id(&self.profiles, &slugify_profile_name(name))
            }
            McpProfilesMode::List => return Err("No MCP profile edit in progress".to_string()),
        };
        let profile = crate::core::mcp::McpProfile {
            id,
            name: name.to_string(),
            selection: selection_without_profile_id(self.selection.clone()),
        };
        crate::core::mcp::profiles::upsert_profile(&mut self.profiles, profile.clone());
        self.selected_profile = self
            .profiles
            .iter()
            .position(|existing| existing.id == profile.id)
            .unwrap_or(0);
        self.mode = McpProfilesMode::List;
        self.error = None;
        Ok(profile)
    }

    fn server_enabled(&self, id: &str) -> bool {
        if self.selection.is_all_servers() {
            return true;
        }
        self.selection
            .servers
            .iter()
            .find(|server| server.id == id)
            .map(|server| server.enabled)
            .unwrap_or(true)
    }
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

fn unique_profile_name(profiles: &[crate::core::mcp::McpProfile], base: &str) -> String {
    let mut candidate = base.to_string();
    let mut n = 2;
    while profiles.iter().any(|profile| profile.name == candidate) {
        candidate = format!("{base} {n}");
        n += 1;
    }
    candidate
}
