#[derive(Debug, Clone, PartialEq)]
pub struct MoveForm {
    pub session_id: String,
    pub session_title: String,
    pub groups: Vec<(String, String)>, // (path, name)
    pub selected: usize,
}
