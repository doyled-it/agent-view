//! Application state and event dispatch

use crate::core::groups::ListRow;
use crate::types::{Group, Session};
use crate::ui::theme::Theme;

#[derive(Debug, Clone, PartialEq)]
pub enum Overlay {
    None,
    NewSession(NewSessionForm),
    Confirm(ConfirmDialog),
    Rename(RenameForm),
    Move(MoveForm),
    GroupManage(GroupForm),
    CommandPalette(CommandPalette),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommandPalette {
    pub query: String,
    pub items: Vec<CommandItem>,
    pub filtered: Vec<usize>,
    pub selected: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommandItem {
    pub label: String,
    pub key_hint: String,
    pub action: CommandAction,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CommandAction {
    NewSession,
    StopSession,
    RestartSession,
    DeleteSession,
    RenameSession,
    MoveSession,
    ToggleNotify,
    ToggleFollowUp,
    ExportLog,
    CreateGroup,
    Search,
    Quit,
}

impl CommandPalette {
    pub fn new() -> Self {
        let items = vec![
            CommandItem {
                label: "New Session".to_string(),
                key_hint: "n".to_string(),
                action: CommandAction::NewSession,
            },
            CommandItem {
                label: "Stop Session".to_string(),
                key_hint: "s".to_string(),
                action: CommandAction::StopSession,
            },
            CommandItem {
                label: "Restart Session".to_string(),
                key_hint: "r".to_string(),
                action: CommandAction::RestartSession,
            },
            CommandItem {
                label: "Delete Session".to_string(),
                key_hint: "d".to_string(),
                action: CommandAction::DeleteSession,
            },
            CommandItem {
                label: "Rename".to_string(),
                key_hint: "R".to_string(),
                action: CommandAction::RenameSession,
            },
            CommandItem {
                label: "Move to Group".to_string(),
                key_hint: "m".to_string(),
                action: CommandAction::MoveSession,
            },
            CommandItem {
                label: "Toggle Notifications".to_string(),
                key_hint: "!".to_string(),
                action: CommandAction::ToggleNotify,
            },
            CommandItem {
                label: "Toggle Follow-up".to_string(),
                key_hint: "i".to_string(),
                action: CommandAction::ToggleFollowUp,
            },
            CommandItem {
                label: "Export Log".to_string(),
                key_hint: "e".to_string(),
                action: CommandAction::ExportLog,
            },
            CommandItem {
                label: "Create Group".to_string(),
                key_hint: "g".to_string(),
                action: CommandAction::CreateGroup,
            },
            CommandItem {
                label: "Search Sessions".to_string(),
                key_hint: "/".to_string(),
                action: CommandAction::Search,
            },
            CommandItem {
                label: "Quit".to_string(),
                key_hint: "q".to_string(),
                action: CommandAction::Quit,
            },
        ];
        let filtered: Vec<usize> = (0..items.len()).collect();
        Self {
            query: String::new(),
            items,
            filtered,
            selected: 0,
        }
    }

    pub fn filter(&mut self) {
        let q = self.query.to_lowercase();
        if q.is_empty() {
            self.filtered = (0..self.items.len()).collect();
        } else {
            self.filtered = self
                .items
                .iter()
                .enumerate()
                .filter(|(_, item)| item.label.to_lowercase().contains(&q))
                .map(|(i, _)| i)
                .collect();
        }
        self.selected = 0;
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MoveForm {
    pub session_id: String,
    pub session_title: String,
    pub groups: Vec<(String, String)>, // (path, name)
    pub selected: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GroupForm {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenameForm {
    pub target_id: String,
    pub target_type: RenameTarget,
    pub input: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RenameTarget {
    Session,
    Group,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewSessionForm {
    pub title: String,
    pub project_path: String,
    pub focused_field: usize,
}

impl NewSessionForm {
    pub fn new() -> Self {
        let home = dirs::home_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/tmp".to_string());
        Self {
            title: String::new(),
            project_path: home,
            focused_field: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmDialog {
    pub message: String,
    pub action: ConfirmAction,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmAction {
    DeleteSession(String),
    StopSession(String),
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
    pub search_query: Option<String>,
    pub toast_message: Option<String>,
    pub toast_expire: Option<std::time::Instant>,
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
            search_query: None,
            toast_message: None,
            toast_expire: None,
        }
    }

    /// Rebuild the flattened list from current sessions and groups
    pub fn rebuild_list_rows(&mut self) {
        let groups = crate::core::groups::ensure_default_group(&self.groups);
        self.list_rows = crate::core::groups::flatten_group_tree(&self.sessions, &groups);
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

    pub fn clamp_selection(&mut self) {
        if self.list_rows.is_empty() {
            self.selected_index = 0;
        } else if self.selected_index >= self.list_rows.len() {
            self.selected_index = self.list_rows.len() - 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::groups::ListRow;
    use crate::types::{Session, SessionStatus, Tool};

    fn make_session(id: &str) -> Session {
        Session {
            id: id.to_string(),
            title: format!("Session {}", id),
            project_path: "/tmp".to_string(),
            group_path: String::new(),
            order: 0,
            command: String::new(),
            wrapper: String::new(),
            tool: Tool::Claude,
            status: SessionStatus::Idle,
            tmux_session: String::new(),
            created_at: 1700000000000,
            last_accessed: 0,
            parent_session_id: String::new(),
            worktree_path: String::new(),
            worktree_repo: String::new(),
            worktree_branch: String::new(),
            tool_data: "{}".to_string(),
            acknowledged: false,
            notify: false,
            follow_up: false,
            status_changed_at: 0,
            restart_count: 0,
            status_history: vec![],
        }
    }

    fn make_app_with_sessions(count: usize) -> App {
        let mut app = App::new(false);
        for i in 0..count {
            app.list_rows
                .push(ListRow::Session(Box::new(make_session(&i.to_string()))));
        }
        app
    }

    // --- move_selection wrapping ---

    #[test]
    fn test_move_selection_down_wraps_to_start() {
        let mut app = make_app_with_sessions(3);
        app.selected_index = 2; // last item
        app.move_selection_down();
        assert_eq!(app.selected_index, 0, "should wrap to first item");
    }

    #[test]
    fn test_move_selection_up_wraps_to_end() {
        let mut app = make_app_with_sessions(3);
        app.selected_index = 0; // first item
        app.move_selection_up();
        assert_eq!(app.selected_index, 2, "should wrap to last item");
    }

    #[test]
    fn test_move_selection_on_empty_list_is_noop() {
        let mut app = App::new(false);
        app.selected_index = 0;
        app.move_selection_down();
        assert_eq!(app.selected_index, 0);
        app.move_selection_up();
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_move_selection_down_increments() {
        let mut app = make_app_with_sessions(3);
        app.selected_index = 0;
        app.move_selection_down();
        assert_eq!(app.selected_index, 1);
    }

    #[test]
    fn test_move_selection_up_decrements() {
        let mut app = make_app_with_sessions(3);
        app.selected_index = 2;
        app.move_selection_up();
        assert_eq!(app.selected_index, 1);
    }

    // --- clamp_selection ---

    #[test]
    fn test_clamp_selection_empty_list_sets_zero() {
        let mut app = App::new(false);
        app.selected_index = 99;
        app.clamp_selection();
        assert_eq!(app.selected_index, 0);
    }

    #[test]
    fn test_clamp_selection_out_of_bounds() {
        let mut app = make_app_with_sessions(3);
        app.selected_index = 10;
        app.clamp_selection();
        assert_eq!(app.selected_index, 2, "should clamp to last valid index");
    }

    #[test]
    fn test_clamp_selection_in_bounds_unchanged() {
        let mut app = make_app_with_sessions(3);
        app.selected_index = 1;
        app.clamp_selection();
        assert_eq!(app.selected_index, 1);
    }

    // --- selected_session / selected_group ---

    #[test]
    fn test_selected_session_returns_session_at_index() {
        let mut app = make_app_with_sessions(2);
        app.selected_index = 0;
        let s = app.selected_session();
        assert!(s.is_some());
        assert_eq!(s.unwrap().id, "0");
    }

    #[test]
    fn test_selected_session_returns_none_for_group_row() {
        use crate::types::Group;
        let mut app = App::new(false);
        app.list_rows.push(ListRow::Group {
            group: Group {
                path: "work".to_string(),
                name: "Work".to_string(),
                expanded: true,
                order: 0,
                default_path: String::new(),
            },
            session_count: 0,
            running_count: 0,
            waiting_count: 0,
        });
        app.selected_index = 0;
        assert!(app.selected_session().is_none());
    }

    #[test]
    fn test_selected_group_returns_group_at_index() {
        use crate::types::Group;
        let mut app = App::new(false);
        app.list_rows.push(ListRow::Group {
            group: Group {
                path: "work".to_string(),
                name: "Work".to_string(),
                expanded: true,
                order: 0,
                default_path: String::new(),
            },
            session_count: 0,
            running_count: 0,
            waiting_count: 0,
        });
        app.selected_index = 0;
        let g = app.selected_group();
        assert!(g.is_some());
        assert_eq!(g.unwrap().path, "work");
    }

    // --- search_matches ---

    #[test]
    fn test_search_matches_returns_empty_when_no_query() {
        let app = make_app_with_sessions(3);
        assert!(app.search_matches().is_empty());
    }

    #[test]
    fn test_search_matches_returns_empty_when_query_is_empty_string() {
        let mut app = make_app_with_sessions(3);
        app.search_query = Some(String::new());
        assert!(app.search_matches().is_empty());
    }

    #[test]
    fn test_search_matches_finds_matching_sessions() {
        let mut app = App::new(false);
        let mut s1 = make_session("a");
        s1.title = "alpha-task".to_string();
        let mut s2 = make_session("b");
        s2.title = "beta-task".to_string();
        let mut s3 = make_session("c");
        s3.title = "gamma-work".to_string();
        app.list_rows.push(ListRow::Session(Box::new(s1)));
        app.list_rows.push(ListRow::Session(Box::new(s2)));
        app.list_rows.push(ListRow::Session(Box::new(s3)));
        app.search_query = Some("task".to_string());

        let matches = app.search_matches();
        assert_eq!(matches.len(), 2);
        assert!(matches.contains(&0));
        assert!(matches.contains(&1));
    }

    #[test]
    fn test_search_matches_is_case_insensitive() {
        let mut app = App::new(false);
        let mut s = make_session("a");
        s.title = "Alpha-Task".to_string();
        app.list_rows.push(ListRow::Session(Box::new(s)));
        app.search_query = Some("alpha".to_string());
        assert_eq!(app.search_matches(), vec![0]);
    }

    // --- CommandPalette ---

    #[test]
    fn test_command_palette_new_shows_all_items() {
        let cp = CommandPalette::new();
        assert_eq!(cp.filtered.len(), cp.items.len());
        assert_eq!(cp.selected, 0);
        assert!(cp.query.is_empty());
    }

    #[test]
    fn test_command_palette_filter_empty_shows_all() {
        let mut cp = CommandPalette::new();
        cp.query = String::new();
        cp.filter();
        assert_eq!(cp.filtered.len(), cp.items.len());
    }

    #[test]
    fn test_command_palette_filter_matches_by_label() {
        let mut cp = CommandPalette::new();
        cp.query = "session".to_string();
        cp.filter();
        assert!(
            !cp.filtered.is_empty(),
            "should match items with 'session' in label"
        );
        for &idx in &cp.filtered {
            assert!(
                cp.items[idx].label.to_lowercase().contains("session"),
                "item '{}' should contain 'session'",
                cp.items[idx].label
            );
        }
    }

    #[test]
    fn test_command_palette_filter_resets_selected_to_zero() {
        let mut cp = CommandPalette::new();
        cp.selected = 5;
        cp.query = "quit".to_string();
        cp.filter();
        assert_eq!(cp.selected, 0);
    }

    #[test]
    fn test_command_palette_filter_no_match_returns_empty() {
        let mut cp = CommandPalette::new();
        cp.query = "zzznomatch".to_string();
        cp.filter();
        assert!(cp.filtered.is_empty());
    }

    #[test]
    fn test_command_palette_filter_is_case_insensitive() {
        let mut cp = CommandPalette::new();
        cp.query = "QUIT".to_string();
        cp.filter();
        assert_eq!(cp.filtered.len(), 1);
        assert_eq!(cp.items[cp.filtered[0]].label, "Quit");
    }
}
