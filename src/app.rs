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
    CycleSort,
    Quit,
}

impl CommandPalette {
    pub fn new() -> Self {
        let items = vec![
            CommandItem { label: "New Session".to_string(), key_hint: "n".to_string(), action: CommandAction::NewSession },
            CommandItem { label: "Stop Session".to_string(), key_hint: "s".to_string(), action: CommandAction::StopSession },
            CommandItem { label: "Restart Session".to_string(), key_hint: "r".to_string(), action: CommandAction::RestartSession },
            CommandItem { label: "Delete Session".to_string(), key_hint: "d".to_string(), action: CommandAction::DeleteSession },
            CommandItem { label: "Rename".to_string(), key_hint: "R".to_string(), action: CommandAction::RenameSession },
            CommandItem { label: "Move to Group".to_string(), key_hint: "m".to_string(), action: CommandAction::MoveSession },
            CommandItem { label: "Toggle Notifications".to_string(), key_hint: "!".to_string(), action: CommandAction::ToggleNotify },
            CommandItem { label: "Toggle Follow-up".to_string(), key_hint: "i".to_string(), action: CommandAction::ToggleFollowUp },
            CommandItem { label: "Export Log".to_string(), key_hint: "e".to_string(), action: CommandAction::ExportLog },
            CommandItem { label: "Create Group".to_string(), key_hint: "g".to_string(), action: CommandAction::CreateGroup },
            CommandItem { label: "Search Sessions".to_string(), key_hint: "/".to_string(), action: CommandAction::Search },
            CommandItem { label: "Cycle Sort Mode".to_string(), key_hint: "S".to_string(), action: CommandAction::CycleSort },
            CommandItem { label: "Quit".to_string(), key_hint: "q".to_string(), action: CommandAction::Quit },
        ];
        let filtered: Vec<usize> = (0..items.len()).collect();
        Self { query: String::new(), items, filtered, selected: 0 }
    }

    pub fn filter(&mut self) {
        let q = self.query.to_lowercase();
        if q.is_empty() {
            self.filtered = (0..self.items.len()).collect();
        } else {
            self.filtered = self.items.iter().enumerate()
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
    pub sort_mode: crate::types::SortMode,
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
            sort_mode: crate::types::SortMode::StatusPriority,
        }
    }

    /// Rebuild the flattened list from current sessions and groups
    pub fn rebuild_list_rows(&mut self) {
        let groups = crate::core::groups::ensure_default_group(&self.groups);
        self.list_rows = crate::core::groups::flatten_group_tree(&self.sessions, &groups, self.sort_mode);
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
