//! Application state and event dispatch

use crate::core::groups::ListRow;
use crate::types::{Group, Session};
use crate::ui::theme::Theme;
use std::collections::HashSet;
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveTab {
    Sessions,
    Routines,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Overlay {
    None,
    NewSession(NewSessionForm),
    NewRoutine(NewRoutineForm),
    Confirm(ConfirmDialog),
    Rename(RenameForm),
    Move(MoveForm),
    GroupManage(GroupForm),
    CommandPalette(CommandPalette),
    Help,
    ThemeSelect(ThemeSelectForm),
    AddNote(NoteForm),
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

#[derive(Debug, Clone, PartialEq)]
pub struct NoteForm {
    pub session_id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThemeSelectForm {
    pub options: Vec<String>,
    pub selected: usize,
    pub original_theme_name: String,
}

impl ThemeSelectForm {
    pub fn new(current_theme: &str) -> Self {
        let options: Vec<String> = crate::ui::theme::Theme::available()
            .iter()
            .map(|s| s.to_string())
            .collect();
        let selected = options.iter().position(|o| o == current_theme).unwrap_or(0);
        Self {
            options,
            selected,
            original_theme_name: current_theme.to_string(),
        }
    }
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
    PinSession,
    ShowHelp,
    SelectTheme,
    CyclePanel,
    Quit,
    NewRoutine,
    ToggleRoutine,
    DeleteRoutine,
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
                label: "Cycle Sort Mode".to_string(),
                key_hint: "S".to_string(),
                action: CommandAction::CycleSort,
            },
            CommandItem {
                label: "Pin/Unpin Session".to_string(),
                key_hint: "p".to_string(),
                action: CommandAction::PinSession,
            },
            CommandItem {
                label: "Select Theme".to_string(),
                key_hint: "t".to_string(),
                action: CommandAction::SelectTheme,
            },
            CommandItem {
                label: "Cycle Panel".to_string(),
                key_hint: "v".to_string(),
                action: CommandAction::CyclePanel,
            },
            CommandItem {
                label: "Show Help".to_string(),
                key_hint: "?".to_string(),
                action: CommandAction::ShowHelp,
            },
            CommandItem {
                label: "Quit".to_string(),
                key_hint: "q".to_string(),
                action: CommandAction::Quit,
            },
            CommandItem {
                label: "New Routine".to_string(),
                key_hint: "n".to_string(),
                action: CommandAction::NewRoutine,
            },
            CommandItem {
                label: "Toggle Routine".to_string(),
                key_hint: "Space".to_string(),
                action: CommandAction::ToggleRoutine,
            },
            CommandItem {
                label: "Delete Routine".to_string(),
                key_hint: "d".to_string(),
                action: CommandAction::DeleteRoutine,
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
    Routine,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleFrequency {
    Hourly,
    Daily,
    Weekly,
    Monthly,
    Yearly,
    Advanced,
}

impl ScheduleFrequency {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Hourly => "Hourly",
            Self::Daily => "Daily",
            Self::Weekly => "Weekly",
            Self::Monthly => "Monthly",
            Self::Yearly => "Yearly",
            Self::Advanced => "Advanced",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            Self::Hourly => Self::Daily,
            Self::Daily => Self::Weekly,
            Self::Weekly => Self::Monthly,
            Self::Monthly => Self::Yearly,
            Self::Yearly => Self::Advanced,
            Self::Advanced => Self::Hourly,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            Self::Hourly => Self::Advanced,
            Self::Daily => Self::Hourly,
            Self::Weekly => Self::Daily,
            Self::Monthly => Self::Weekly,
            Self::Yearly => Self::Monthly,
            Self::Advanced => Self::Yearly,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewRoutineForm {
    pub name: String,
    pub default_tool: String,
    pub working_dir: String,
    pub frequency: ScheduleFrequency,
    pub hour: u8,
    pub minute: u8,
    pub weekdays: [bool; 7], // Sun=0 through Sat=6
    pub month_day: u8,
    pub month: u8,
    pub cron_raw: String,
    pub steps: Vec<crate::types::RoutineStep>,
    pub editing_step: Option<String>, // text buffer when adding a step
    pub notify: bool,
    pub step_timeout_secs: i32,
    pub focused_field: usize,
    pub completions: Vec<String>,
    pub completion_index: Option<usize>,
    pub edit_routine_id: Option<String>, // Some(id) when editing existing routine
}

impl NewRoutineForm {
    pub fn new() -> Self {
        let home = dirs::home_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/tmp".to_string());
        Self {
            name: String::new(),
            default_tool: "claude".to_string(),
            working_dir: home,
            frequency: ScheduleFrequency::Daily,
            hour: 9,
            minute: 0,
            weekdays: [false, true, true, true, true, true, false], // Mon-Fri
            month_day: 1,
            month: 1,
            cron_raw: String::new(),
            steps: Vec::new(),
            editing_step: None,
            notify: true,
            step_timeout_secs: 1800,
            focused_field: 0,
            completions: Vec::new(),
            completion_index: None,
            edit_routine_id: None,
        }
    }

    pub fn from_routine(routine: &crate::types::Routine) -> Self {
        let mut form = Self::new();
        form.name = routine.name.clone();
        form.default_tool = routine.default_tool.clone();
        form.working_dir = routine.working_dir.clone();
        form.steps = routine.steps.clone();
        form.notify = routine.notify;
        form.step_timeout_secs = routine.step_timeout_secs;
        form.edit_routine_id = Some(routine.id.clone());
        // Try to parse the cron expression back into frequency fields
        form.cron_raw = routine.schedule.clone();
        form.frequency = ScheduleFrequency::Advanced;
        form
    }

    pub fn cron_expression(&self) -> String {
        match self.frequency {
            ScheduleFrequency::Hourly => crate::core::schedule::build_hourly(self.minute),
            ScheduleFrequency::Daily => crate::core::schedule::build_daily(self.hour, self.minute),
            ScheduleFrequency::Weekly => {
                let days: Vec<crate::core::schedule::Weekday> = self
                    .weekdays
                    .iter()
                    .enumerate()
                    .filter(|(_, &selected)| selected)
                    .map(|(i, _)| crate::core::schedule::Weekday::all()[i])
                    .collect();
                if days.is_empty() {
                    crate::core::schedule::build_daily(self.hour, self.minute)
                } else {
                    crate::core::schedule::build_weekly(&days, self.hour, self.minute)
                }
            }
            ScheduleFrequency::Monthly => {
                crate::core::schedule::build_monthly_by_day(self.month_day, self.hour, self.minute)
            }
            ScheduleFrequency::Yearly => crate::core::schedule::build_yearly(
                self.month,
                self.month_day,
                self.hour,
                self.minute,
            ),
            ScheduleFrequency::Advanced => self.cron_raw.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewSessionForm {
    pub title: String,
    pub project_path: String,
    pub focused_field: usize,
    pub completions: Vec<String>,
    pub completion_index: Option<usize>,
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
            completions: Vec::new(),
            completion_index: None,
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
    BulkDelete,
    BulkStop,
    DeleteRoutine(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DetailPanelMode {
    None,
    Preview,
    Metadata,
    Both,
}

impl DetailPanelMode {
    pub fn next(self) -> Self {
        match self {
            Self::None => Self::Preview,
            Self::Preview => Self::Metadata,
            Self::Metadata => Self::Both,
            Self::Both => Self::None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Off",
            Self::Preview => "Preview",
            Self::Metadata => "Details",
            Self::Both => "Both",
        }
    }

    pub fn shows_preview(self) -> bool {
        matches!(self, Self::Preview | Self::Both)
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "none" => Self::None,
            "preview" => Self::Preview,
            "both" => Self::Both,
            _ => Self::Metadata,
        }
    }

    pub fn as_config_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Preview => "preview",
            Self::Metadata => "metadata",
            Self::Both => "both",
        }
    }
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
    pub toast_message: Option<String>,
    pub toast_expire: Option<std::time::Instant>,
    pub sort_mode: crate::types::SortMode,
    pub activity_feed: VecDeque<crate::types::ActivityEvent>,
    pub show_activity_feed: bool,
    pub bulk_selected: HashSet<String>,
    pub config_changed: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub detail_mode: DetailPanelMode,
    pub preview_content: String,
    pub preview_last_session: Option<String>,
    pub preview_last_capture: Option<std::time::Instant>,
    pub active_tab: ActiveTab,
    pub routines: Vec<crate::types::Routine>,
    pub routine_runs_cache: std::collections::HashMap<String, Vec<crate::types::RoutineRun>>,
    pub routine_list_rows: Vec<RoutineListRow>,
    pub routine_selected_index: usize,
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
            toast_message: None,
            toast_expire: None,
            sort_mode: crate::types::SortMode::StatusPriority,
            activity_feed: VecDeque::new(),
            show_activity_feed: true,
            bulk_selected: HashSet::new(),
            config_changed: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            detail_mode: DetailPanelMode::Metadata,
            preview_content: String::new(),
            preview_last_session: None,
            preview_last_capture: None,
            active_tab: ActiveTab::Sessions,
            routines: Vec::new(),
            routine_runs_cache: std::collections::HashMap::new(),
            routine_list_rows: Vec::new(),
            routine_selected_index: 0,
        }
    }

    pub fn push_activity(&mut self, event: crate::types::ActivityEvent) {
        self.activity_feed.push_front(event);
        if self.activity_feed.len() > 100 {
            self.activity_feed.pop_back();
        }
    }

    pub fn toggle_bulk_select(&mut self, session_id: &str) {
        if self.bulk_selected.contains(session_id) {
            self.bulk_selected.remove(session_id);
        } else {
            self.bulk_selected.insert(session_id.to_string());
        }
    }

    pub fn clear_bulk_selection(&mut self) {
        self.bulk_selected.clear();
    }

    pub fn select_all_visible(&mut self) {
        for row in &self.list_rows {
            if let crate::core::groups::ListRow::Session(s) = row {
                self.bulk_selected.insert(s.id.clone());
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

    pub fn clamp_selection(&mut self) {
        if self.list_rows.is_empty() {
            self.selected_index = 0;
        } else if self.selected_index >= self.list_rows.len() {
            self.selected_index = self.list_rows.len() - 1;
        }
    }

    pub fn toggle_tab(&mut self) {
        self.active_tab = match self.active_tab {
            ActiveTab::Sessions => ActiveTab::Routines,
            ActiveTab::Routines => ActiveTab::Sessions,
        };
    }

    /// Rebuild the flattened routine list from current routines and their runs
    pub fn rebuild_routine_list_rows(&mut self) {
        let mut rows: Vec<RoutineListRow> = Vec::new();

        // Group routines by group_path
        let mut groups_map: std::collections::HashMap<String, Vec<&crate::types::Routine>> =
            std::collections::HashMap::new();
        for routine in &self.routines {
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
                        if let Some(runs) = self.routine_runs_cache.get(&routine.id) {
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

        self.routine_list_rows = rows;
        self.clamp_routine_selection();
    }

    pub fn clamp_routine_selection(&mut self) {
        if self.routine_list_rows.is_empty() {
            self.routine_selected_index = 0;
        } else if self.routine_selected_index >= self.routine_list_rows.len() {
            self.routine_selected_index = self.routine_list_rows.len() - 1;
        }
    }

    #[allow(dead_code)]
    pub fn selected_routine(&self) -> Option<&crate::types::Routine> {
        match self.routine_list_rows.get(self.routine_selected_index) {
            Some(RoutineListRow::Routine(r)) => Some(r),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn selected_run(&self) -> Option<&crate::types::RoutineRun> {
        match self.routine_list_rows.get(self.routine_selected_index) {
            Some(RoutineListRow::Run { run, .. }) => Some(run),
            _ => None,
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
            acknowledged: false,
            notify: false,
            follow_up: false,
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
        use crate::app::DetailPanelMode;
        assert_eq!(DetailPanelMode::None.next(), DetailPanelMode::Preview);
        assert_eq!(DetailPanelMode::Preview.next(), DetailPanelMode::Metadata);
        assert_eq!(DetailPanelMode::Metadata.next(), DetailPanelMode::Both);
        assert_eq!(DetailPanelMode::Both.next(), DetailPanelMode::None);
    }

    #[test]
    fn test_detail_panel_mode_labels() {
        use crate::app::DetailPanelMode;
        assert_eq!(DetailPanelMode::None.label(), "Off");
        assert_eq!(DetailPanelMode::Preview.label(), "Preview");
        assert_eq!(DetailPanelMode::Metadata.label(), "Details");
        assert_eq!(DetailPanelMode::Both.label(), "Both");
    }

    #[test]
    fn test_detail_panel_mode_from_str() {
        use crate::app::DetailPanelMode;
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
        use crate::app::DetailPanelMode;
        assert_eq!(DetailPanelMode::None.as_config_str(), "none");
        assert_eq!(DetailPanelMode::Preview.as_config_str(), "preview");
        assert_eq!(DetailPanelMode::Metadata.as_config_str(), "metadata");
        assert_eq!(DetailPanelMode::Both.as_config_str(), "both");
    }

    #[test]
    fn test_toggle_bulk_selection() {
        let mut app = App::new(false);
        app.toggle_bulk_select("s1");
        assert!(app.bulk_selected.contains("s1"));
        app.toggle_bulk_select("s1");
        assert!(!app.bulk_selected.contains("s1"));
    }

    #[test]
    fn test_clear_bulk_selection() {
        let mut app = App::new(false);
        app.toggle_bulk_select("s1");
        app.toggle_bulk_select("s2");
        app.clear_bulk_selection();
        assert!(app.bulk_selected.is_empty());
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
        if matches!(app.list_rows.get(0), Some(ListRow::Group { .. })) {
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
        if matches!(app.list_rows.get(0), Some(ListRow::Group { .. })) {
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
        assert_eq!(app.active_tab, ActiveTab::Sessions);
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
        assert!(app.bulk_selected.contains("s1"));
        assert!(app.bulk_selected.contains("s2"));
    }
}
