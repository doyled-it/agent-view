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
    DeleteGroup(String),
    DeleteRoutine(String),
    FinishSession(String),
}
