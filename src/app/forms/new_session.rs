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
