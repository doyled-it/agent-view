use super::command_palette::CommandPalette;
use super::forms::{ConfirmDialog, GroupForm, MoveForm, NoteForm, RenameForm, ThemeSelectForm};
use super::forms::{NewRoutineForm, NewSessionForm};

#[derive(Debug, Clone, PartialEq)]
pub enum Overlay {
    None,
    NewSession(NewSessionForm),
    NewRoutine(NewRoutineForm),
    Confirm(ConfirmDialog),
    Rename(RenameForm),
    Move(MoveForm),
    GroupManage(GroupForm),
    RoutineWarning,
    CommandPalette(CommandPalette),
    Help,
    ThemeSelect(ThemeSelectForm),
    AddNote(NoteForm),
}
