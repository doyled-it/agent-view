use std::collections::HashMap;

use crate::core::storage::Storage;
use crate::core::tmux;
use crate::core::tmux::{SessionCache, TmuxError};
use crate::types::{Session, SessionCreateOptions, SessionStatus, StatusHistoryEntry, Tool};

use super::crash::build_restart_command;
use super::generate_title;

#[derive(thiserror::Error, Debug)]
pub enum SessionError {
    #[error("storage error: {0}")]
    Storage(String),
    #[error("session not found")]
    NotFound,
    #[error("tmux error: {0}")]
    Tmux(#[from] TmuxError),
    #[error("worktree error: {0}")]
    Worktree(#[from] crate::core::git::GitError),
}

pub type SessionResult<T> = Result<T, SessionError>;

/// Session lifecycle operations (create, stop, delete, restart).
/// Stateless — lives on the main thread.
pub struct SessionOps;

impl SessionOps {
    /// Create a new session (creates tmux session and saves to storage).
    /// Returns (session, optional non-fatal warning) — currently warnings
    /// only originate from the worktree-setup hook.
    pub fn create_session(
        &self,
        storage: &Storage,
        cache: &mut SessionCache,
        options: SessionCreateOptions,
    ) -> SessionResult<(Session, Option<String>)> {
        let title = options.title.unwrap_or_else(generate_title);
        let id = uuid::Uuid::new_v4().to_string();
        let tmux_name = tmux::generate_session_name(&title);
        let command = options
            .command
            .unwrap_or_else(|| options.tool.command().to_string());

        let now = chrono::Utc::now().timestamp_millis();

        // Resolve worktree, if requested. The tmux session uses the worktree
        // as its working directory; worktree_repo retains the original repo
        // path so cleanup later knows where to invoke `git worktree remove`.
        let (working_dir, worktree_path, worktree_repo, worktree_branch) =
            if let Some(wt) = options.worktree.as_ref() {
                let path = crate::core::git::create_worktree(
                    &options.project_path,
                    &wt.branch,
                    wt.base.as_deref(),
                )?;
                (
                    path.clone(),
                    path,
                    options.project_path.clone(),
                    wt.branch.clone(),
                )
            } else {
                (
                    options.project_path.clone(),
                    String::new(),
                    String::new(),
                    String::new(),
                )
            };

        // Run the optional post-create hook (non-fatal).
        let hook_warning = if options.worktree.is_some() {
            match crate::core::session::hooks::run_worktree_setup_hook(
                &options.project_path,
                &working_dir,
            ) {
                Ok(()) => None,
                Err(e) => Some(format!("worktree-setup.sh failed: {}", e)),
            }
        } else {
            None
        };

        let mut env = HashMap::new();
        env.insert("AGENT_ORCHESTRATOR_SESSION".to_string(), id.clone());

        // NOTE: if tmux::create_session fails here, a freshly created worktree
        // at `working_dir` is leaked on disk. Task 8's orphan sweep is the
        // recovery path; no inline rollback to keep the failure message simple.
        tmux::create_session(&tmux_name, Some(&command), Some(&working_dir), Some(&env))?;

        cache.register(&tmux_name);

        let session = Session {
            id: id.clone(),
            title,
            project_path: options.project_path,
            group_path: options
                .group_path
                .unwrap_or_else(|| "my-sessions".to_string()),
            order: storage.load_sessions().unwrap_or_default().len() as i32,
            command,
            wrapper: String::new(),
            tool: options.tool,
            status: SessionStatus::Running,
            tmux_session: tmux_name,
            created_at: now,
            last_accessed: now,
            parent_session_id: String::new(),
            worktree_path,
            worktree_repo,
            worktree_branch,
            tool_data: "{}".to_string(),
            acknowledged: false,
            notify: false,
            follow_up: false,
            status_changed_at: now,
            restart_count: 0,
            last_started_at: now,
            notes: vec![],
            status_history: vec![StatusHistoryEntry {
                status: "running".to_string(),
                timestamp: now,
            }],
            pinned: false,
            tokens_used: 0,
        };

        storage
            .save_session(&session)
            .map_err(|e| SessionError::Storage(format!("Failed to save session: {}", e)))?;
        storage.touch().ok();

        Ok((session, hook_warning))
    }

    /// Stop a session (kill tmux but keep the record)
    pub fn stop_session(&self, storage: &Storage, session_id: &str) -> SessionResult<()> {
        let session = storage
            .get_session(session_id)
            .map_err(|e| SessionError::Storage(format!("DB error: {}", e)))?
            .ok_or(SessionError::NotFound)?;

        if !session.tmux_session.is_empty() {
            tmux::kill_session(&session.tmux_session)?;
        }

        storage
            .write_status(session_id, SessionStatus::Stopped, session.tool)
            .map_err(|e| SessionError::Storage(format!("DB error: {}", e)))?;
        storage.touch().ok();

        Ok(())
    }

    /// Delete a session (kill tmux and remove from storage)
    pub fn delete_session(
        &self,
        storage: &Storage,
        cache: &mut SessionCache,
        session_id: &str,
    ) -> SessionResult<()> {
        let session = storage
            .get_session(session_id)
            .map_err(|e| SessionError::Storage(format!("DB error: {}", e)))?;

        if let Some(session) = session {
            if !session.tmux_session.is_empty() {
                tmux::kill_session(&session.tmux_session)?;
                cache.remove(&session.tmux_session);
            }
        }

        storage
            .delete_session(session_id)
            .map_err(|e| SessionError::Storage(format!("DB error: {}", e)))?;
        storage.touch().ok();

        Ok(())
    }

    /// Restart a session (kill and recreate tmux session)
    pub fn restart_session(
        &self,
        storage: &Storage,
        cache: &mut SessionCache,
        session_id: &str,
    ) -> SessionResult<Session> {
        let mut session = storage
            .get_session(session_id)
            .map_err(|e| SessionError::Storage(format!("DB error: {}", e)))?
            .ok_or(SessionError::NotFound)?;

        if !session.tmux_session.is_empty() {
            if tmux::session_exists(&session.tmux_session) {
                tmux::kill_session(&session.tmux_session)?;
            }
            cache.remove(&session.tmux_session);
        }

        let new_tmux_name = tmux::generate_session_name(&session.title);
        let mut env = HashMap::new();
        env.insert("AGENT_ORCHESTRATOR_SESSION".to_string(), session.id.clone());

        let restart_cmd = build_restart_command(session.tool, &session.command, &session.tool_data);
        tmux::create_session(
            &new_tmux_name,
            Some(&restart_cmd),
            Some(&session.project_path),
            Some(&env),
        )?;

        cache.register(&new_tmux_name);

        session.tmux_session = new_tmux_name;
        session.status = SessionStatus::Running;
        let now = chrono::Utc::now().timestamp_millis();
        session.last_accessed = now;
        session.last_started_at = now;

        // Clear old Claude session ID — new session will get a new one
        if session.tool == Tool::Claude {
            if let Ok(mut data) = serde_json::from_str::<serde_json::Value>(&session.tool_data) {
                data.as_object_mut().map(|o| o.remove("claude_session_id"));
                session.tool_data = data.to_string();
            }
        }

        storage
            .save_session(&session)
            .map_err(|e| SessionError::Storage(format!("DB error: {}", e)))?;
        storage
            .increment_restart_count(session_id)
            .map_err(|e| SessionError::Storage(format!("DB error: {}", e)))?;
        storage.touch().ok();

        Ok(session)
    }

    /// Return worktree paths under `repo_dir` that have no matching session
    /// record. Excludes the primary worktree (the repo itself).
    pub fn find_orphan_worktrees(
        &self,
        storage: &Storage,
        repo_dir: &str,
    ) -> SessionResult<Vec<String>> {
        let worktrees = crate::core::git::list_worktrees(repo_dir)?;
        let sessions = storage
            .load_sessions()
            .map_err(|e| SessionError::Storage(format!("DB error: {}", e)))?;
        let known: std::collections::HashSet<String> = sessions
            .iter()
            .map(|s| s.worktree_path.clone())
            .filter(|p| !p.is_empty())
            .collect();

        let canonical_repo = std::fs::canonicalize(repo_dir)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| repo_dir.to_string());

        Ok(worktrees
            .into_iter()
            .filter(|w| !w.bare)
            .map(|w| w.path)
            .filter(|p| *p != canonical_repo && p != repo_dir)
            .filter(|p| !known.contains(p))
            .collect())
    }

    /// Force-remove an orphan worktree.
    pub fn remove_orphan_worktree(&self, repo_dir: &str, worktree_path: &str) -> SessionResult<()> {
        crate::core::git::remove_worktree(repo_dir, worktree_path, true)?;
        Ok(())
    }

    /// Finish a session: kill tmux, remove the worktree (force, to nuke any
    /// uncommitted scratch), and optionally delete the branch when it has
    /// been merged into the repository's default upstream (`main` or
    /// `master`). Always deletes the session record.
    pub fn finish_session(
        &self,
        storage: &Storage,
        cache: &mut SessionCache,
        session_id: &str,
        delete_branch: bool,
    ) -> SessionResult<FinishOutcome> {
        let session = storage
            .get_session(session_id)
            .map_err(|e| SessionError::Storage(format!("DB error: {}", e)))?
            .ok_or(SessionError::NotFound)?;

        if !session.tmux_session.is_empty() && tmux::session_exists(&session.tmux_session) {
            tmux::kill_session(&session.tmux_session)?;
            cache.remove(&session.tmux_session);
        }

        let mut outcome = FinishOutcome {
            worktree_removed: false,
            branch_deleted: false,
            branch_skipped_unmerged: false,
        };

        if !session.worktree_path.is_empty() && !session.worktree_repo.is_empty() {
            crate::core::git::remove_worktree(
                &session.worktree_repo,
                &session.worktree_path,
                /*force=*/ true,
            )?;
            outcome.worktree_removed = true;

            if delete_branch && !session.worktree_branch.is_empty() {
                if let Some(upstream) =
                    crate::core::git::default_upstream_branch(&session.worktree_repo)
                {
                    let merged = crate::core::git::is_branch_merged(
                        &session.worktree_repo,
                        &session.worktree_branch,
                        &upstream,
                    )
                    .unwrap_or(false);
                    if merged {
                        crate::core::git::delete_branch(
                            &session.worktree_repo,
                            &session.worktree_branch,
                            false,
                        )?;
                        outcome.branch_deleted = true;
                    } else {
                        outcome.branch_skipped_unmerged = true;
                    }
                }
            }
        }

        storage
            .delete_session(session_id)
            .map_err(|e| SessionError::Storage(format!("DB error: {}", e)))?;
        storage.touch().ok();

        Ok(outcome)
    }
}

#[derive(Debug, Clone, Default)]
pub struct FinishOutcome {
    pub worktree_removed: bool,
    pub branch_deleted: bool,
    pub branch_skipped_unmerged: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tmux::SessionCache;
    use crate::types::{SessionCreateOptions, Tool, WorktreeCreateOptions};
    use std::process::Command as Cmd;

    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        Cmd::new("git")
            .args(["-C", path, "init", "-q", "-b", "main"])
            .status()
            .unwrap();
        Cmd::new("git")
            .args(["-C", path, "config", "user.email", "t@t"])
            .status()
            .unwrap();
        Cmd::new("git")
            .args(["-C", path, "config", "user.name", "t"])
            .status()
            .unwrap();
        std::fs::write(dir.path().join("a.txt"), "hi").unwrap();
        Cmd::new("git")
            .args(["-C", path, "add", "."])
            .status()
            .unwrap();
        Cmd::new("git")
            .args(["-C", path, "commit", "-qm", "init"])
            .status()
            .unwrap();
        dir
    }

    #[test]
    fn test_find_orphan_worktrees_returns_unknown_paths() {
        let dir = init_repo();
        let path = dir.path().to_str().unwrap().to_string();
        let _ = crate::core::git::create_worktree(&path, "orphan-1", None).unwrap();
        let (storage, _db_dir) = crate::core::storage::test_helpers::test_storage();

        let orphans = SessionOps.find_orphan_worktrees(&storage, &path).unwrap();
        // Main repo worktree is excluded; orphan-1 has no session row → orphan.
        assert!(orphans.iter().any(|w| w.contains("orphan-1")));
        assert!(!orphans.iter().any(|w| w == &path));
    }

    #[test]
    #[ignore = "creates a real tmux session — run locally with `cargo test -- --ignored`"]
    fn test_create_session_with_worktree_populates_fields_and_uses_wt_path() {
        let repo = init_repo();
        let repo_path = repo.path().to_str().unwrap().to_string();
        let (storage, _db_dir) = crate::core::storage::test_helpers::test_storage();
        let mut cache = SessionCache::new();
        let ops = SessionOps;

        let (session, _warn) = ops
            .create_session(
                &storage,
                &mut cache,
                SessionCreateOptions {
                    title: Some("wt-test".to_string()),
                    project_path: repo_path.clone(),
                    group_path: None,
                    tool: Tool::Shell,
                    command: Some("sleep 1".to_string()),
                    worktree: Some(WorktreeCreateOptions {
                        branch: "wt-feature".to_string(),
                        new_branch: true,
                        base: None,
                    }),
                },
            )
            .unwrap();

        assert_eq!(session.worktree_repo, repo_path);
        assert_eq!(session.worktree_branch, "wt-feature");
        assert!(session.worktree_path.contains(".worktrees"));
        assert!(session.worktree_path.contains("wt-feature"));
        assert!(std::path::Path::new(&session.worktree_path).exists());

        // Cleanup tmux + worktree
        let _ = crate::core::tmux::kill_session(&session.tmux_session);
        let _ = crate::core::git::remove_worktree(&repo_path, &session.worktree_path, true);
    }

    #[test]
    #[ignore = "creates real tmux + git worktree"]
    fn test_finish_session_removes_worktree_and_branch() {
        let repo = init_repo();
        let repo_path = repo.path().to_str().unwrap().to_string();
        let (storage, _db_dir) = crate::core::storage::test_helpers::test_storage();
        let mut cache = SessionCache::new();
        let ops = SessionOps;

        let (session, _) = ops
            .create_session(
                &storage,
                &mut cache,
                SessionCreateOptions {
                    title: Some("finish-test".to_string()),
                    project_path: repo_path.clone(),
                    group_path: None,
                    tool: Tool::Shell,
                    command: Some("sleep 5".to_string()),
                    worktree: Some(WorktreeCreateOptions {
                        branch: "merged-branch".to_string(),
                        new_branch: true,
                        base: None,
                    }),
                },
            )
            .unwrap();

        let outcome = ops
            .finish_session(
                &storage,
                &mut cache,
                &session.id,
                /*delete_branch=*/ true,
            )
            .unwrap();

        assert!(outcome.worktree_removed);
        assert!(outcome.branch_deleted);
        assert!(!outcome.branch_skipped_unmerged);
        assert!(!std::path::Path::new(&session.worktree_path).exists());
        assert!(!crate::core::git::branch_exists(
            &repo_path,
            "merged-branch"
        ));
    }
}
