//! Optional shell hooks invoked during session lifecycle events.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

/// Run `<repo_root>/.agent-view/worktree-setup.sh` if present and executable.
/// Sets `AGENT_VIEW_REPO_ROOT` and `AGENT_VIEW_WORKTREE_PATH` and runs with
/// `worktree_path` as the working directory. Returns `Ok(())` when the hook
/// is missing or not executable. Returns `Err` with combined stderr/stdout
/// when the hook exits non-zero or fails to spawn.
pub fn run_worktree_setup_hook(repo_root: &str, worktree_path: &str) -> Result<(), String> {
    let hook = Path::new(repo_root)
        .join(".agent-view")
        .join("worktree-setup.sh");

    let metadata = match std::fs::metadata(&hook) {
        Ok(m) => m,
        Err(_) => return Ok(()), // no hook is a no-op
    };
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Ok(()); // not executable — skip silently
    }

    let output = Command::new(&hook)
        .env("AGENT_VIEW_REPO_ROOT", repo_root)
        .env("AGENT_VIEW_WORKTREE_PATH", worktree_path)
        .current_dir(worktree_path)
        .output()
        .map_err(|e| format!("Failed to spawn worktree-setup.sh: {}", e))?;

    if !output.status.success() {
        let mut msg = format!(
            "worktree-setup.sh exited {}",
            output.status.code().unwrap_or(-1)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !stderr.is_empty() {
            msg.push_str(&format!("\nstderr: {}", stderr));
        }
        if !stdout.is_empty() {
            msg.push_str(&format!("\nstdout: {}", stdout));
        }
        return Err(msg);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn test_no_hook_present_returns_ok() {
        let dir = tempfile::tempdir().unwrap();
        let res = run_worktree_setup_hook(dir.path().to_str().unwrap(), "/tmp/wt");
        assert!(res.is_ok());
    }

    #[test]
    fn test_hook_runs_with_envs() {
        let dir = tempfile::tempdir().unwrap();
        let av = dir.path().join(".agent-view");
        fs::create_dir(&av).unwrap();
        let hook = av.join("worktree-setup.sh");
        let marker = dir.path().join("ran");
        fs::write(
            &hook,
            format!(
                "#!/bin/sh\necho \"$AGENT_VIEW_REPO_ROOT $AGENT_VIEW_WORKTREE_PATH\" > {}\n",
                marker.display()
            ),
        )
        .unwrap();
        let mut perms = fs::metadata(&hook).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&hook, perms).unwrap();

        run_worktree_setup_hook(dir.path().to_str().unwrap(), dir.path().to_str().unwrap())
            .unwrap();
        let body = fs::read_to_string(&marker).unwrap();
        assert!(body.contains(dir.path().to_str().unwrap()));
    }

    #[test]
    fn test_non_executable_hook_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let av = dir.path().join(".agent-view");
        fs::create_dir(&av).unwrap();
        fs::write(av.join("worktree-setup.sh"), "#!/bin/sh\nexit 1\n").unwrap();
        // No chmod +x → must skip silently (Ok).
        assert!(run_worktree_setup_hook(dir.path().to_str().unwrap(), "/tmp/wt").is_ok());
    }

    #[test]
    fn test_hook_failure_returns_err() {
        let dir = tempfile::tempdir().unwrap();
        let av = dir.path().join(".agent-view");
        fs::create_dir(&av).unwrap();
        let hook = av.join("worktree-setup.sh");
        fs::write(&hook, "#!/bin/sh\necho oops 1>&2\nexit 2\n").unwrap();
        let mut perms = fs::metadata(&hook).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&hook, perms).unwrap();

        let err =
            run_worktree_setup_hook(dir.path().to_str().unwrap(), dir.path().to_str().unwrap())
                .unwrap_err();
        assert!(err.contains("oops") || err.contains("exit") || err.contains("2"));
    }
}
