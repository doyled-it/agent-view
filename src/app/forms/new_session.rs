#[derive(Debug, Clone, PartialEq)]
pub struct NewSessionForm {
    pub title: String,
    pub project_path: String,
    /// 0 = title, 1 = project path, 2 = worktree branch, 3 = base ref
    pub focused_field: usize,
    pub completions: Vec<String>,
    pub completion_index: Option<usize>,
    /// Directory prefix the path completions were drawn from (preserves the
    /// user's original form, e.g. `~/projects/`). Used to rebuild the path on
    /// each cycle without stripping more segments than intended.
    pub completion_base: String,
    pub worktree_branch: String,
    /// When true, branch is created fresh; when false, attach to an existing
    /// branch. Toggle wired via Ctrl-T in the input handler.
    pub worktree_new_branch: bool,
    pub worktree_base: String,
    /// Inline validation error rendered under the worktree row.
    pub error: Option<String>,
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
            completion_base: String::new(),
            worktree_branch: String::new(),
            worktree_new_branch: true,
            worktree_base: String::new(),
            error: None,
        }
    }

    /// Drop any in-flight completion state. Call when changing fields or when
    /// the path is edited so the next Tab refetches from a clean slate.
    pub fn clear_completions(&mut self) {
        self.completions.clear();
        self.completion_index = None;
        self.completion_base.clear();
    }
}
