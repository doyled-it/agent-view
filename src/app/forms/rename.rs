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
