#[derive(Debug, Clone, PartialEq)]
pub struct NewSessionForm {
    /// The runner this session will use. Cycled via Left/Right when the
    /// runner field is focused (focused_field == 0).
    pub runner: crate::types::Tool,
    /// Snapshot of `runner::implemented_tools()` taken at form construction
    /// so per-frame renders don't re-query the runner registry.
    pub runners: Vec<crate::types::Tool>,
    pub title: String,
    pub project_path: String,
    /// 0 = runner, 1 = title, 2 = project path, 3 = worktree branch, 4 = base ref
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
            runner: crate::types::Tool::Claude,
            runners: crate::core::runner::implemented_tools(),
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

    /// Move the runner selection to the next entry in `runners`, wrapping.
    #[allow(dead_code)] // wired in Task 6 (input handler)
    pub fn cycle_runner_next(&mut self) {
        if self.runners.is_empty() {
            return;
        }
        let idx = self
            .runners
            .iter()
            .position(|t| *t == self.runner)
            .unwrap_or(0);
        self.runner = self.runners[(idx + 1) % self.runners.len()];
    }

    /// Move the runner selection to the previous entry in `runners`, wrapping.
    #[allow(dead_code)] // wired in Task 6 (input handler)
    pub fn cycle_runner_prev(&mut self) {
        if self.runners.is_empty() {
            return;
        }
        let idx = self
            .runners
            .iter()
            .position(|t| *t == self.runner)
            .unwrap_or(0);
        let n = self.runners.len();
        self.runner = self.runners[(idx + n - 1) % n];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Tool;

    #[test]
    fn test_form_default_runner_is_claude() {
        let f = NewSessionForm::new();
        assert_eq!(f.runner, Tool::Claude);
        assert_eq!(f.focused_field, 0);
    }

    #[test]
    fn test_runners_field_populated_at_construction() {
        let f = NewSessionForm::new();
        assert!(f.runners.contains(&Tool::Claude));
        assert!(f.runners.contains(&Tool::Shell));
        assert!(!f.runners.is_empty());
    }

    #[test]
    fn test_cycle_runner_next_wraps() {
        let mut f = NewSessionForm::new();
        let n = f.runners.len();
        for _ in 0..n {
            f.cycle_runner_next();
        }
        assert_eq!(f.runner, Tool::Claude);
    }

    #[test]
    fn test_cycle_runner_prev_wraps() {
        let mut f = NewSessionForm::new();
        f.cycle_runner_prev();
        assert_eq!(f.runner, *f.runners.last().unwrap());
    }

    #[test]
    fn test_cycle_runner_next_advances_one() {
        let mut f = NewSessionForm::new();
        assert_eq!(f.runner, Tool::Claude);
        f.cycle_runner_next();
        assert_eq!(f.runner, f.runners[1]);
    }
}
