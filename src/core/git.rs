//! Git worktree operations

use std::path::Path;
use std::process::Command;

/// Represents a git worktree entry
#[derive(Debug, Clone, PartialEq)]
pub struct Worktree {
    pub path: String,
    pub branch: String,
    pub commit: String,
    pub bare: bool,
}

/// Check if a directory is inside a git repository
pub fn is_git_repo(dir: &str) -> bool {
    Command::new("git")
        .args(["-C", dir, "rev-parse", "--git-dir"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get the repository root
pub fn get_repo_root(dir: &str) -> Result<String, String> {
    let output = Command::new("git")
        .args(["-C", dir, "rev-parse", "--show-toplevel"])
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if !output.status.success() {
        return Err("Not a git repository".to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Validate a branch name following git's naming rules.
/// Returns None if valid, or Some(error_message) if invalid.
pub fn validate_branch_name(name: &str) -> Option<String> {
    if name.is_empty() {
        return Some("branch name cannot be empty".to_string());
    }
    if name.trim() != name {
        return Some("branch name cannot have leading or trailing spaces".to_string());
    }
    if name.contains("..") {
        return Some("branch name cannot contain '..'".to_string());
    }
    if name.starts_with('.') {
        return Some("branch name cannot start with '.'".to_string());
    }
    if name.ends_with(".lock") {
        return Some("branch name cannot end with '.lock'".to_string());
    }
    let invalid = [' ', '\t', '~', '^', ':', '?', '*', '[', '\\'];
    for c in &invalid {
        if name.contains(*c) {
            return Some(format!("branch name cannot contain '{}'", c));
        }
    }
    if name.contains("@{") {
        return Some("branch name cannot contain '@{'".to_string());
    }
    if name == "@" {
        return Some("branch name cannot be just '@'".to_string());
    }
    None
}

/// Check if a branch exists in the repository
pub fn branch_exists(repo_dir: &str, branch: &str) -> bool {
    Command::new("git")
        .args([
            "-C",
            repo_dir,
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{}", branch),
        ])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Generate a worktree path: <repo>/.worktrees/<branch-sanitized>
pub fn generate_worktree_path(repo_dir: &str, branch: &str) -> String {
    let sanitized: String = branch.replace('/', "-").replace(' ', "-");
    Path::new(repo_dir)
        .join(".worktrees")
        .join(&sanitized)
        .to_string_lossy()
        .to_string()
}

/// Create a git worktree. Returns the worktree path on success.
/// If base_branch is provided and the branch does not yet exist, the new
/// branch is created from base_branch instead of HEAD.
pub fn create_worktree(
    repo_dir: &str,
    branch: &str,
    base_branch: Option<&str>,
) -> Result<String, String> {
    if let Some(err) = validate_branch_name(branch) {
        return Err(format!("Invalid branch name: {}", err));
    }
    if !is_git_repo(repo_dir) {
        return Err("Not a git repository".to_string());
    }

    let wt_path = generate_worktree_path(repo_dir, branch);

    let output = if branch_exists(repo_dir, branch) {
        // Attach to the existing branch
        Command::new("git")
            .args(["-C", repo_dir, "worktree", "add", &wt_path, branch])
            .output()
            .map_err(|e| format!("Failed to run git: {}", e))?
    } else {
        // Create a new branch, optionally from a base
        let base = base_branch.unwrap_or("HEAD");
        Command::new("git")
            .args(["-C", repo_dir, "worktree", "add", "-b", branch, &wt_path, base])
            .output()
            .map_err(|e| format!("Failed to run git: {}", e))?
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to create worktree: {}", stderr));
    }

    Ok(wt_path)
}

/// List all worktrees for the repository at repo_dir
pub fn list_worktrees(repo_dir: &str) -> Result<Vec<Worktree>, String> {
    if !is_git_repo(repo_dir) {
        return Err("Not a git repository".to_string());
    }

    let output = Command::new("git")
        .args(["-C", repo_dir, "worktree", "list", "--porcelain"])
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to list worktrees: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_worktree_list(&stdout))
}

/// Parse the output of `git worktree list --porcelain`
fn parse_worktree_list(output: &str) -> Vec<Worktree> {
    let mut worktrees = Vec::new();
    let mut path = String::new();
    let mut branch = String::new();
    let mut commit = String::new();
    let mut bare = false;

    for line in output.lines() {
        if line.is_empty() {
            if !path.is_empty() {
                worktrees.push(Worktree {
                    path: path.clone(),
                    branch: branch.clone(),
                    commit: commit.clone(),
                    bare,
                });
            }
            path.clear();
            branch.clear();
            commit.clear();
            bare = false;
        } else if let Some(rest) = line.strip_prefix("worktree ") {
            path = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("HEAD ") {
            commit = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("branch ") {
            branch = rest
                .strip_prefix("refs/heads/")
                .unwrap_or(rest)
                .to_string();
        } else if line == "bare" {
            bare = true;
        } else if line == "detached" {
            branch.clear();
        }
    }

    // Handle last entry if output does not end with a blank line
    if !path.is_empty() {
        worktrees.push(Worktree {
            path,
            branch,
            commit,
            bare,
        });
    }

    worktrees
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_branch_name_valid() {
        assert!(validate_branch_name("feature/new-thing").is_none());
        assert!(validate_branch_name("fix-123").is_none());
        assert!(validate_branch_name("main").is_none());
        assert!(validate_branch_name("release/v1.0.0").is_none());
    }

    #[test]
    fn test_validate_branch_name_invalid() {
        assert!(validate_branch_name("").is_some());
        assert!(validate_branch_name("has space").is_some());
        assert!(validate_branch_name("has..dots").is_some());
        assert!(validate_branch_name(".starts-with-dot").is_some());
        assert!(validate_branch_name("ends.lock").is_some());
        assert!(validate_branch_name("has~tilde").is_some());
        assert!(validate_branch_name("has^caret").is_some());
        assert!(validate_branch_name("has:colon").is_some());
        assert!(validate_branch_name("has?question").is_some());
        assert!(validate_branch_name("has*star").is_some());
        assert!(validate_branch_name("has[bracket").is_some());
        assert!(validate_branch_name("has\\backslash").is_some());
        assert!(validate_branch_name("has@{brace").is_some());
        assert!(validate_branch_name("@").is_some());
    }

    #[test]
    fn test_validate_branch_name_leading_trailing_spaces() {
        assert!(validate_branch_name(" leading").is_some());
        assert!(validate_branch_name("trailing ").is_some());
        assert!(validate_branch_name(" both ").is_some());
    }

    #[test]
    fn test_generate_worktree_path() {
        let path = generate_worktree_path("/repo", "feature/my-branch");
        assert!(path.contains(".worktrees"));
        assert!(path.contains("feature-my-branch"));
    }

    #[test]
    fn test_generate_worktree_path_spaces() {
        let path = generate_worktree_path("/repo", "my branch");
        assert!(path.contains(".worktrees"));
        assert!(path.contains("my-branch"));
    }

    #[test]
    fn test_parse_worktree_list_basic() {
        let output = "\
worktree /home/user/repo
HEAD abc123def456
branch refs/heads/main

worktree /home/user/repo/.worktrees/feature-foo
HEAD 111222333444
branch refs/heads/feature/foo

";
        let wts = parse_worktree_list(output);
        assert_eq!(wts.len(), 2);
        assert_eq!(wts[0].path, "/home/user/repo");
        assert_eq!(wts[0].branch, "main");
        assert_eq!(wts[0].commit, "abc123def456");
        assert!(!wts[0].bare);
        assert_eq!(wts[1].branch, "feature/foo");
    }

    #[test]
    fn test_parse_worktree_list_bare() {
        let output = "\
worktree /home/user/bare.git
HEAD 000000000000
bare

";
        let wts = parse_worktree_list(output);
        assert_eq!(wts.len(), 1);
        assert!(wts[0].bare);
        assert_eq!(wts[0].branch, "");
    }

    #[test]
    fn test_parse_worktree_list_detached() {
        let output = "\
worktree /home/user/repo
HEAD deadbeef1234
detached

";
        let wts = parse_worktree_list(output);
        assert_eq!(wts.len(), 1);
        assert_eq!(wts[0].branch, "");
        assert_eq!(wts[0].commit, "deadbeef1234");
    }

    #[test]
    fn test_parse_worktree_list_no_trailing_newline() {
        let output = "\
worktree /home/user/repo
HEAD abc123
branch refs/heads/main";
        let wts = parse_worktree_list(output);
        assert_eq!(wts.len(), 1);
        assert_eq!(wts[0].branch, "main");
    }
}
