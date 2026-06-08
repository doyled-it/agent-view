//! Application state and event dispatch

use crate::core::groups::ListRow;
use crate::types::{Group, Session};
use crate::ui::theme::Theme;

mod command_palette;
mod detail_panel;
mod forms;
mod overlay;
mod schedule_freq;
mod state;

pub use command_palette::{CommandAction, CommandPalette};
pub use detail_panel::DetailPanelMode;
pub use forms::{
    ConfirmAction, ConfirmDialog, GroupForm, McpProfilesForm, McpProfilesMode, McpSyncForm,
    MoveForm, NewRoutineForm, NewSessionForm, NoteForm, RenameForm, RenameTarget, ThemeSelectForm,
};
pub use overlay::Overlay;
pub use schedule_freq::ScheduleFrequency;
pub use state::ActivityState;
pub use state::BulkSelection;
pub use state::PreviewState;
pub use state::RoutineState;
pub use state::StatusPageState;
pub use state::ToastState;
pub use state::UsageState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveTab {
    Sessions,
    Routines,
    Costs,
}

#[derive(Debug, Clone)]
pub enum RoutineListRow {
    Group {
        group: crate::types::Group,
        routine_count: usize,
    },
    Routine(Box<crate::types::Routine>),
    Run {
        run: Box<crate::types::RoutineRun>,
        routine_name: String,
    },
}

pub struct App {
    pub sessions: Vec<Session>,
    pub groups: Vec<Group>,
    pub list_rows: Vec<ListRow>,
    pub selected_index: usize,
    pub overlay: Overlay,
    pub should_quit: bool,
    pub last_storage_mtime: i64,
    pub attach_session: Option<String>,
    pub theme: Theme,
    pub theme_name: String,
    pub search_query: Option<String>,
    pub toast: ToastState,
    pub sort_mode: crate::types::SortMode,
    pub activity: ActivityState,
    pub bulk: BulkSelection,
    pub config_changed: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub detail_mode: DetailPanelMode,
    pub preview: PreviewState,
    pub active_tab: ActiveTab,
    pub cost_period: crate::core::cost::CostPeriod,
    pub routine_state: RoutineState,
    pub usage_state: UsageState,
    pub status_state: StatusPageState,
    /// Live hook-status state from the watcher thread. `None` in tests
    /// and during the brief window before `main` wires it in.
    pub event_state: Option<crate::core::runner::event_watcher::EventStateHandle>,
    /// Shared storage handle for read-only access from render code
    /// (cost totals, etc.). Owned `Storage` continues to live in `main`
    /// for setup-time loads.
    pub storage: Option<crate::core::runner::event_watcher::SharedStorage>,
    pub config: crate::core::config::AppConfig,
}

impl App {
    pub fn new(light: bool) -> Self {
        Self {
            sessions: Vec::new(),
            groups: Vec::new(),
            list_rows: Vec::new(),
            selected_index: 0,
            overlay: Overlay::None,
            should_quit: false,
            last_storage_mtime: 0,
            attach_session: None,
            theme: if light { Theme::light() } else { Theme::dark() },
            theme_name: if light {
                "light".to_string()
            } else {
                "dark".to_string()
            },
            search_query: None,
            toast: ToastState::new(),
            sort_mode: crate::types::SortMode::StatusPriority,
            activity: ActivityState::new(),
            bulk: BulkSelection::new(),
            config_changed: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            detail_mode: DetailPanelMode::Metadata,
            preview: PreviewState::new(),
            active_tab: ActiveTab::Sessions,
            cost_period: crate::core::cost::CostPeriod::Week,
            routine_state: RoutineState::new(),
            usage_state: UsageState::new(),
            status_state: StatusPageState::new(),
            event_state: None,
            storage: None,
            config: crate::core::config::AppConfig::default(),
        }
    }

    pub fn push_activity(&mut self, event: crate::types::ActivityEvent) {
        self.activity.feed.push_front(event);
        if self.activity.feed.len() > 100 {
            self.activity.feed.pop_back();
        }
    }

    pub fn toggle_bulk_select(&mut self, session_id: &str) {
        if self.bulk.selected.contains(session_id) {
            self.bulk.selected.remove(session_id);
        } else {
            self.bulk.selected.insert(session_id.to_string());
        }
    }

    pub fn clear_bulk_selection(&mut self) {
        self.bulk.selected.clear();
    }

    pub fn select_all_visible(&mut self) {
        for row in &self.list_rows {
            if let crate::core::groups::ListRow::Session(s) = row {
                self.bulk.selected.insert(s.id.clone());
            }
        }
    }

    /// Rebuild the flattened list from current sessions and groups
    pub fn rebuild_list_rows(&mut self) {
        let groups = crate::core::groups::ensure_default_group(&self.groups);
        self.list_rows =
            crate::core::groups::flatten_group_tree(&self.sessions, &groups, self.sort_mode);
        self.clamp_selection();
    }

    pub fn selected_session(&self) -> Option<&Session> {
        match self.list_rows.get(self.selected_index) {
            Some(ListRow::Session(s)) => Some(s),
            _ => None,
        }
    }

    pub fn selected_group(&self) -> Option<&Group> {
        match self.list_rows.get(self.selected_index) {
            Some(ListRow::Group { group, .. }) => Some(group),
            _ => None,
        }
    }

    pub fn move_selection_up(&mut self) {
        if self.list_rows.is_empty() {
            return;
        }
        if self.selected_index > 0 {
            self.selected_index -= 1;
        } else {
            self.selected_index = self.list_rows.len() - 1;
        }
    }

    pub fn move_selection_down(&mut self) {
        if self.list_rows.is_empty() {
            return;
        }
        if self.selected_index < self.list_rows.len() - 1 {
            self.selected_index += 1;
        } else {
            self.selected_index = 0;
        }
    }

    /// Get the indices of list_rows entries (sessions) matching the current search query.
    /// Returns an empty Vec when no search is active or the query is empty.
    pub fn search_matches(&self) -> Vec<usize> {
        let query = match &self.search_query {
            Some(q) if !q.is_empty() => q.to_lowercase(),
            _ => return Vec::new(),
        };

        self.list_rows
            .iter()
            .enumerate()
            .filter_map(|(i, row)| match row {
                ListRow::Session(s) if s.title.to_lowercase().contains(&query) => Some(i),
                _ => None,
            })
            .collect()
    }

    /// Get the indices of routine_list_rows entries matching the current search query.
    /// Returns an empty Vec when no search is active or the query is empty.
    pub fn routine_search_matches(&self) -> Vec<usize> {
        let query = match &self.search_query {
            Some(q) if !q.is_empty() => q.to_lowercase(),
            _ => return Vec::new(),
        };

        self.routine_state
            .list_rows
            .iter()
            .enumerate()
            .filter_map(|(i, row)| match row {
                RoutineListRow::Routine(r) if r.name.to_lowercase().contains(&query) => Some(i),
                RoutineListRow::Run { routine_name, .. }
                    if routine_name.to_lowercase().contains(&query) =>
                {
                    Some(i)
                }
                _ => None,
            })
            .collect()
    }

    pub fn clamp_selection(&mut self) {
        if self.list_rows.is_empty() {
            self.selected_index = 0;
        } else if self.selected_index >= self.list_rows.len() {
            self.selected_index = self.list_rows.len() - 1;
        }
    }

    pub fn toggle_tab(&mut self) {
        self.active_tab = match self.active_tab {
            ActiveTab::Sessions => {
                if !self.routine_state.tab_warning_shown {
                    self.overlay = Overlay::RoutineWarning;
                }
                ActiveTab::Routines
            }
            ActiveTab::Routines => ActiveTab::Costs,
            ActiveTab::Costs => ActiveTab::Sessions,
        };
    }

    /// Rebuild the flattened routine list from current routines and their runs
    pub fn rebuild_routine_list_rows(&mut self) {
        let mut rows: Vec<RoutineListRow> = Vec::new();

        // Group routines by group_path
        let mut groups_map: std::collections::HashMap<String, Vec<&crate::types::Routine>> =
            std::collections::HashMap::new();
        for routine in &self.routine_state.routines {
            groups_map
                .entry(routine.group_path.clone())
                .or_default()
                .push(routine);
        }

        // Sort groups by name
        let mut group_paths: Vec<String> = groups_map.keys().cloned().collect();
        group_paths.sort();

        for group_path in &group_paths {
            let group_routines = &groups_map[group_path];
            let group_name = group_path
                .rsplit('/')
                .next()
                .unwrap_or(group_path)
                .to_string();

            // Check if this group is expanded (look in self.groups first, default to expanded)
            let expanded = self
                .groups
                .iter()
                .find(|g| g.path == *group_path)
                .map(|g| g.expanded)
                .unwrap_or(true);

            let group = crate::types::Group {
                path: group_path.clone(),
                name: group_name,
                expanded,
                order: 0,
                default_path: String::new(),
            };

            rows.push(RoutineListRow::Group {
                group,
                routine_count: group_routines.len(),
            });

            if expanded {
                for routine in group_routines {
                    rows.push(RoutineListRow::Routine(Box::new((*routine).clone())));

                    // If routine is expanded, add its runs
                    if routine.expanded {
                        if let Some(runs) = self.routine_state.runs_cache.get(&routine.id) {
                            for run in runs {
                                rows.push(RoutineListRow::Run {
                                    run: Box::new(run.clone()),
                                    routine_name: routine.name.clone(),
                                });
                            }
                        }
                    }
                }
            }
        }

        self.routine_state.list_rows = rows;
        self.clamp_routine_selection();
    }

    pub fn clamp_routine_selection(&mut self) {
        if self.routine_state.list_rows.is_empty() {
            self.routine_state.selected_index = 0;
        } else if self.routine_state.selected_index >= self.routine_state.list_rows.len() {
            self.routine_state.selected_index = self.routine_state.list_rows.len() - 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::groups::ListRow;
    use crate::types::{Group, Session, SessionStatus, SortMode, Tool};

    fn make_session(id: &str, title: &str, group: &str, tmux: &str) -> Session {
        Session {
            id: id.to_string(),
            title: title.to_string(),
            project_path: "/tmp".to_string(),
            group_path: group.to_string(),
            order: 0,
            command: String::new(),
            wrapper: String::new(),
            tool: Tool::Claude,
            status: SessionStatus::Idle,
            tmux_session: tmux.to_string(),
            created_at: 0,
            last_accessed: 0,
            parent_session_id: String::new(),
            worktree_path: String::new(),
            worktree_repo: String::new(),
            worktree_branch: String::new(),
            tool_data: "{}".to_string(),
            mcp_selection: crate::core::mcp::McpSelection::default(),
            acknowledged: false,
            notify: false,
            follow_up: false,
            user_waiting: false,
            status_changed_at: 0,
            restart_count: 0,
            last_started_at: 0,
            notes: vec![],
            status_history: vec![],
            pinned: false,
            tokens_used: 0,
        }
    }

    fn make_group(path: &str, name: &str) -> Group {
        Group {
            path: path.to_string(),
            name: name.to_string(),
            expanded: true,
            order: 0,
            default_path: String::new(),
        }
    }

    fn app_with_sessions(sessions: Vec<Session>) -> App {
        let mut app = App::new(false);
        app.groups = vec![make_group("my-sessions", "Ungrouped")];
        app.sessions = sessions;
        app.rebuild_list_rows();
        app
    }

    #[test]
    fn test_detail_panel_mode_cycles() {
        assert_eq!(DetailPanelMode::None.next(), DetailPanelMode::Preview);
        assert_eq!(DetailPanelMode::Preview.next(), DetailPanelMode::Metadata);
        assert_eq!(DetailPanelMode::Metadata.next(), DetailPanelMode::Both);
        assert_eq!(DetailPanelMode::Both.next(), DetailPanelMode::None);
    }

    #[test]
    fn test_detail_panel_mode_labels() {
        assert_eq!(DetailPanelMode::None.label(), "Off");
        assert_eq!(DetailPanelMode::Preview.label(), "Preview");
        assert_eq!(DetailPanelMode::Metadata.label(), "Details");
        assert_eq!(DetailPanelMode::Both.label(), "Both");
    }

    #[test]
    fn test_detail_panel_mode_from_str() {
        assert_eq!(DetailPanelMode::from_str("none"), DetailPanelMode::None);
        assert_eq!(
            DetailPanelMode::from_str("preview"),
            DetailPanelMode::Preview
        );
        assert_eq!(
            DetailPanelMode::from_str("metadata"),
            DetailPanelMode::Metadata
        );
        assert_eq!(DetailPanelMode::from_str("both"), DetailPanelMode::Both);
        assert_eq!(
            DetailPanelMode::from_str("unknown"),
            DetailPanelMode::Metadata
        );
    }

    #[test]
    fn test_detail_panel_mode_as_config_str() {
        assert_eq!(DetailPanelMode::None.as_config_str(), "none");
        assert_eq!(DetailPanelMode::Preview.as_config_str(), "preview");
        assert_eq!(DetailPanelMode::Metadata.as_config_str(), "metadata");
        assert_eq!(DetailPanelMode::Both.as_config_str(), "both");
    }

    #[test]
    fn test_toggle_bulk_selection() {
        let mut app = App::new(false);
        app.toggle_bulk_select("s1");
        assert!(app.bulk.selected.contains("s1"));
        app.toggle_bulk_select("s1");
        assert!(!app.bulk.selected.contains("s1"));
    }

    #[test]
    fn test_clear_bulk_selection() {
        let mut app = App::new(false);
        app.toggle_bulk_select("s1");
        app.toggle_bulk_select("s2");
        app.clear_bulk_selection();
        assert!(app.bulk.selected.is_empty());
    }

    #[test]
    fn test_move_selection_down_wraps() {
        let sessions = vec![
            make_session("s1", "Alpha", "my-sessions", ""),
            make_session("s2", "Beta", "my-sessions", ""),
        ];
        let mut app = app_with_sessions(sessions);
        // Place cursor at last item
        app.selected_index = app.list_rows.len() - 1;
        app.move_selection_down();
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_move_selection_up_wraps() {
        let sessions = vec![
            make_session("s1", "Alpha", "my-sessions", ""),
            make_session("s2", "Beta", "my-sessions", ""),
        ];
        let mut app = app_with_sessions(sessions);
        app.selected_index = 0;
        app.move_selection_up();
        assert_eq!(app.selected_index, app.list_rows.len() - 1);
    }

    #[test]
    fn test_move_selection_down_increments() {
        let sessions = vec![
            make_session("s1", "Alpha", "my-sessions", ""),
            make_session("s2", "Beta", "my-sessions", ""),
        ];
        let mut app = app_with_sessions(sessions);
        app.selected_index = 0;
        app.move_selection_down();
        assert_eq!(app.selected_index, 1);
    }

    #[test]
    fn test_move_selection_up_decrements() {
        let sessions = vec![
            make_session("s1", "Alpha", "my-sessions", ""),
            make_session("s2", "Beta", "my-sessions", ""),
        ];
        let mut app = app_with_sessions(sessions);
        app.selected_index = 1;
        app.move_selection_up();
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_move_selection_empty_list_noop() {
        let mut app = App::new(false);
        // No sessions, no groups — list_rows is empty
        app.rebuild_list_rows();
        app.selected_index = 0;
        app.move_selection_down();
        assert_eq!(app.selected_index, 0);
        app.move_selection_up();
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_clamp_selection_empty_list() {
        let mut app = App::new(false);
        app.selected_index = 99;
        app.clamp_selection();
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_clamp_selection_out_of_bounds() {
        let sessions = vec![make_session("s1", "Alpha", "my-sessions", "")];
        let mut app = app_with_sessions(sessions);
        app.selected_index = 999;
        app.clamp_selection();
        assert!(app.selected_index < app.list_rows.len());
    }

    #[test]
    fn test_clamp_selection_in_bounds_unchanged() {
        let sessions = vec![
            make_session("s1", "Alpha", "my-sessions", ""),
            make_session("s2", "Beta", "my-sessions", ""),
        ];
        let mut app = app_with_sessions(sessions);
        app.selected_index = 1;
        app.clamp_selection();
        assert_eq!(app.selected_index, 1);
    }

    #[test]
    fn test_selected_session_returns_session() {
        let sessions = vec![
            make_session("s1", "Alpha", "my-sessions", ""),
            make_session("s2", "Beta", "my-sessions", ""),
        ];
        let mut app = app_with_sessions(sessions);
        // Find a row that is a Session and select it
        let session_idx = app
            .list_rows
            .iter()
            .position(|r| matches!(r, ListRow::Session(_)))
            .expect("should have at least one session row");
        app.selected_index = session_idx;
        assert!(app.selected_session().is_some());
    }

    #[test]
    fn test_selected_group_returns_group() {
        let sessions = vec![make_session("s1", "Alpha", "my-sessions", "")];
        let mut app = app_with_sessions(sessions);
        // First row should be the group header
        app.selected_index = 0;
        // It's the group header when cursor is on it
        if matches!(app.list_rows.first(), Some(ListRow::Group { .. })) {
            assert!(app.selected_group().is_some());
        } else {
            // No group row visible — skip assertion
        }
    }

    #[test]
    fn test_selected_session_none_on_group_row() {
        let sessions = vec![make_session("s1", "Alpha", "my-sessions", "")];
        let mut app = app_with_sessions(sessions);
        // Select group header row (index 0)
        app.selected_index = 0;
        if matches!(app.list_rows.first(), Some(ListRow::Group { .. })) {
            assert!(app.selected_session().is_none());
        }
    }

    #[test]
    fn test_search_matches_empty_when_no_query() {
        let sessions = vec![make_session("s1", "Alpha", "my-sessions", "")];
        let app = app_with_sessions(sessions);
        assert!(app.search_matches().is_empty());
    }

    #[test]
    fn test_search_matches_finds_by_title() {
        let sessions = vec![
            make_session("s1", "AlphaProject", "my-sessions", ""),
            make_session("s2", "BetaWork", "my-sessions", ""),
        ];
        let mut app = app_with_sessions(sessions);
        app.search_query = Some("alpha".to_string());
        let matches = app.search_matches();
        assert!(!matches.is_empty());
        // All matches should point to session rows containing "alpha" in title (case-insensitive)
        for idx in &matches {
            if let Some(ListRow::Session(s)) = app.list_rows.get(*idx) {
                assert!(s.title.to_lowercase().contains("alpha"));
            }
        }
    }

    #[test]
    fn test_search_matches_case_insensitive() {
        let sessions = vec![make_session("s1", "AlphaProject", "my-sessions", "")];
        let mut app = app_with_sessions(sessions);
        app.search_query = Some("ALPHA".to_string());
        assert!(!app.search_matches().is_empty());
    }

    #[test]
    fn test_search_matches_empty_query_returns_none() {
        let sessions = vec![make_session("s1", "Alpha", "my-sessions", "")];
        let mut app = app_with_sessions(sessions);
        app.search_query = Some(String::new());
        assert!(app.search_matches().is_empty());
    }

    #[test]
    fn test_search_matches_no_match() {
        let sessions = vec![make_session("s1", "Alpha", "my-sessions", "")];
        let mut app = app_with_sessions(sessions);
        app.search_query = Some("zzznomatch".to_string());
        assert!(app.search_matches().is_empty());
    }

    #[test]
    fn test_command_palette_default_shows_all_items() {
        let palette = CommandPalette::new();
        assert_eq!(palette.filtered.len(), palette.items.len());
    }

    #[test]
    fn test_command_palette_includes_waiting_marker_toggle() {
        let palette = CommandPalette::new();
        assert!(palette.items.iter().any(|item| {
            item.label == "Toggle Waiting Marker"
                && item.key_hint == "w"
                && item.action == CommandAction::ToggleUserWaiting
        }));
    }

    #[test]
    fn test_command_palette_includes_mcp_sync() {
        let palette = CommandPalette::new();
        assert!(palette.items.iter().any(|item| {
            item.label == "Sync MCP Servers" && item.action == CommandAction::SyncMcpServers
        }));
    }

    #[test]
    fn test_command_palette_includes_mcp_profile_management() {
        let palette = CommandPalette::new();
        assert!(palette.items.iter().any(|item| {
            item.label == "Manage MCP Profiles" && item.action == CommandAction::ManageMcpProfiles
        }));
    }

    #[test]
    fn test_mcp_profile_form_create_saves_current_selection() {
        let catalog = vec![
            crate::core::mcp::McpServerCatalogEntry::server_level(
                crate::types::Tool::Claude,
                "GitLabMITRE",
            ),
            crate::core::mcp::McpServerCatalogEntry::server_level(
                crate::types::Tool::Codex,
                "GitLabMITRE",
            ),
            crate::core::mcp::McpServerCatalogEntry::server_level(
                crate::types::Tool::Claude,
                "wavecrest",
            ),
        ];
        let mut form = crate::app::McpProfilesForm::new(Vec::new(), catalog);

        form.start_create_from_selection(crate::core::mcp::McpSelection::default());
        form.name_input = "Rust Project".to_string();
        form.toggle_server("wavecrest");
        let saved = form.save_edit().unwrap();

        assert_eq!(saved.id, "rust-project");
        assert_eq!(saved.name, "Rust Project");
        assert_eq!(form.profiles.len(), 1);
        assert_eq!(form.profiles[0].id, "rust-project");
        assert_eq!(form.profiles[0].selection.profile_id, None);
        assert_eq!(form.profiles[0].selection.servers.len(), 2);
        assert!(form.profiles[0]
            .selection
            .servers
            .iter()
            .any(|server| server.id == "GitLabMITRE" && server.enabled));
        assert!(form.profiles[0]
            .selection
            .servers
            .iter()
            .any(|server| server.id == "wavecrest" && !server.enabled));
    }

    #[test]
    fn test_mcp_profile_form_edit_keeps_missing_servers_visible() {
        let profile = crate::core::mcp::McpProfile {
            id: "legacy".to_string(),
            name: "Legacy".to_string(),
            selection: crate::core::mcp::McpSelection {
                profile_id: None,
                servers: vec![crate::core::mcp::McpServerSelection {
                    id: "retired".to_string(),
                    enabled: true,
                    selected_tools: None,
                }],
            },
        };
        let mut form = crate::app::McpProfilesForm::new(
            vec![profile],
            vec![crate::core::mcp::McpServerCatalogEntry::server_level(
                crate::types::Tool::Claude,
                "GitLabMITRE",
            )],
        );

        form.start_edit_selected().unwrap();
        let rows = form.server_rows();

        assert!(rows.iter().any(|row| {
            row.id == "retired" && row.missing && row.enabled && row.display_name == "retired"
        }));
        assert!(rows
            .iter()
            .any(|row| { row.id == "GitLabMITRE" && !row.missing && row.enabled }));
    }

    #[test]
    fn test_mcp_profile_form_duplicate_generates_distinct_profile() {
        let profile = crate::core::mcp::McpProfile {
            id: "rust".to_string(),
            name: "Rust".to_string(),
            selection: crate::core::mcp::McpSelection::default(),
        };
        let mut form = crate::app::McpProfilesForm::new(Vec::from([profile]), Vec::new());

        form.start_duplicate_selected().unwrap();
        let saved = form.save_edit().unwrap();

        assert_eq!(saved.id, "rust-copy");
        assert_eq!(saved.name, "Rust Copy");
        assert_eq!(form.profiles.len(), 2);
        assert!(form.profiles.iter().any(|profile| profile.id == "rust"));
        assert!(form
            .profiles
            .iter()
            .any(|profile| profile.id == "rust-copy"));
    }

    #[test]
    fn test_command_palette_filter_narrows_results() {
        let mut palette = CommandPalette::new();
        palette.query = "new".to_string();
        palette.filter();
        assert!(!palette.filtered.is_empty());
        for &idx in &palette.filtered {
            assert!(palette.items[idx].label.to_lowercase().contains("new"));
        }
    }

    #[test]
    fn test_command_palette_filter_empty_query_restores_all() {
        let mut palette = CommandPalette::new();
        palette.query = "quit".to_string();
        palette.filter();
        let narrowed = palette.filtered.len();
        palette.query = String::new();
        palette.filter();
        assert_eq!(palette.filtered.len(), palette.items.len());
        assert!(narrowed < palette.items.len());
    }

    #[test]
    fn test_command_palette_filter_resets_selected_to_zero() {
        let mut palette = CommandPalette::new();
        palette.selected = 5;
        palette.query = "quit".to_string();
        palette.filter();
        assert_eq!(palette.selected, 0);
    }

    #[test]
    fn test_command_palette_no_match_gives_empty_filtered() {
        let mut palette = CommandPalette::new();
        palette.query = "xyzzy_no_such_command".to_string();
        palette.filter();
        assert!(palette.filtered.is_empty());
    }

    #[test]
    fn test_sort_mode_cycles_via_rebuild() {
        let sessions = vec![
            make_session("s1", "Alpha", "my-sessions", ""),
            make_session("s2", "Beta", "my-sessions", ""),
        ];
        let mut app = app_with_sessions(sessions.clone());
        app.sort_mode = SortMode::Name;
        app.rebuild_list_rows();
        // After rebuild the list should still have the same number of rows
        let count = app.list_rows.len();
        app.sort_mode = SortMode::StatusPriority;
        app.rebuild_list_rows();
        assert_eq!(app.list_rows.len(), count);
    }

    #[test]
    fn test_active_tab_toggles() {
        let mut app = App::new(false);
        assert_eq!(app.active_tab, ActiveTab::Sessions);
        app.toggle_tab();
        assert_eq!(app.active_tab, ActiveTab::Routines);
        app.toggle_tab();
        assert_eq!(app.active_tab, ActiveTab::Costs);
        app.toggle_tab();
        assert_eq!(app.active_tab, ActiveTab::Sessions);
    }

    #[test]
    fn cost_period_defaults_to_week() {
        let app = App::new(false);
        assert_eq!(app.cost_period, crate::core::cost::CostPeriod::Week);
    }

    #[test]
    fn test_select_all_visible() {
        let sessions = vec![
            make_session("s1", "Alpha", "my-sessions", ""),
            make_session("s2", "Beta", "my-sessions", ""),
        ];
        let mut app = app_with_sessions(sessions);
        app.select_all_visible();
        // At least the sessions should be selected
        assert!(app.bulk.selected.contains("s1"));
        assert!(app.bulk.selected.contains("s2"));
    }
}
