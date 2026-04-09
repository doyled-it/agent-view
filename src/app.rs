//! Application state and event dispatch

use crate::core::groups::ListRow;
use crate::types::{Group, Session};
use crate::ui::theme::Theme;

#[derive(Debug, Clone, PartialEq)]
pub enum Overlay {
    None,
    NewSession(NewSessionForm),
    Confirm(ConfirmDialog),
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
    pub returning_from_attach: bool,
    pub last_status_refresh: std::time::Instant,
    pub attach_session: Option<String>,
    pub theme: Theme,
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
            returning_from_attach: false,
            last_status_refresh: std::time::Instant::now(),
            attach_session: None,
            theme: if light { Theme::light() } else { Theme::dark() },
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

    pub fn clamp_selection(&mut self) {
        if self.list_rows.is_empty() {
            self.selected_index = 0;
        } else if self.selected_index >= self.list_rows.len() {
            self.selected_index = self.list_rows.len() - 1;
        }
    }
}
