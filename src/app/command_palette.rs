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
    ToggleUserWaiting,
    SyncMcpServers,
    ExportLog,
    CreateGroup,
    DeleteGroup,
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
    FinishSession,
    SweepOrphanWorktrees,
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
                label: "Finish Session (remove worktree)".to_string(),
                key_hint: "f".to_string(),
                action: CommandAction::FinishSession,
            },
            CommandItem {
                label: "Sweep Orphan Worktrees".to_string(),
                key_hint: String::new(),
                action: CommandAction::SweepOrphanWorktrees,
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
                label: "Toggle Waiting Marker".to_string(),
                key_hint: "w".to_string(),
                action: CommandAction::ToggleUserWaiting,
            },
            CommandItem {
                label: "Sync MCP Servers".to_string(),
                key_hint: String::new(),
                action: CommandAction::SyncMcpServers,
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
                label: "Delete Group".to_string(),
                key_hint: "G".to_string(),
                action: CommandAction::DeleteGroup,
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
