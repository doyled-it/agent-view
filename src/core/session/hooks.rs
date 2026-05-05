//! Optional shell hooks invoked during session lifecycle events.

/// Run `.agent-view/worktree-setup.sh` from the repo root if present and
/// executable. Errors are non-fatal — caller decides how to surface them.
/// Stub for Task 3; full implementation lands in Task 4.
pub fn run_worktree_setup_hook(_repo_root: &str, _worktree_path: &str) -> Result<(), String> {
    Ok(())
}
