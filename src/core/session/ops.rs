use std::collections::{HashMap, HashSet};

use crate::core::mcp::McpSelection;
use crate::core::runner::{runner_for, RunnerLaunch, RunnerLaunchContext, RunnerLaunchError};
use crate::core::storage::Storage;
use crate::core::tmux;
use crate::core::tmux::{SessionCache, TmuxError};
use crate::types::{
    ConductorConfig, Session, SessionCreateOptions, SessionRole, SessionStatus, StatusHistoryEntry,
    Tool,
};

use super::generate_title;

#[cfg(test)]
mod test_support {
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SKIP_TMUX_CREATE_COUNT: AtomicUsize = AtomicUsize::new(0);

    pub struct SkipTmuxCreateGuard;

    impl Drop for SkipTmuxCreateGuard {
        fn drop(&mut self) {
            SKIP_TMUX_CREATE_COUNT.fetch_sub(1, Ordering::SeqCst);
        }
    }

    pub fn skip_tmux_create() -> SkipTmuxCreateGuard {
        SKIP_TMUX_CREATE_COUNT.fetch_add(1, Ordering::SeqCst);
        SkipTmuxCreateGuard
    }

    pub fn should_skip_tmux_create() -> bool {
        SKIP_TMUX_CREATE_COUNT.load(Ordering::SeqCst) > 0
    }
}

#[derive(thiserror::Error, Debug)]
pub enum SessionError {
    #[error("storage error: {0}")]
    Storage(String),
    #[error("session not found")]
    NotFound,
    #[error("runner launch error: {0}")]
    RunnerLaunch(#[from] RunnerLaunchError),
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
    /// Returns (session, optional non-fatal warning) from hook or runner setup.
    pub fn create_session(
        &self,
        storage: &Storage,
        cache: &mut SessionCache,
        options: SessionCreateOptions,
    ) -> SessionResult<(Session, Option<String>)> {
        let title = options.title.unwrap_or_else(generate_title);
        let explicit_command = options.command;
        let requested_mcp_selection = options.mcp_selection.clone();
        let mcp_selection = requested_mcp_selection.clone().unwrap_or_default();
        let tool = options.tool;
        let role = options.role;
        let parent_session = if let Some(parent_id) = options.parent_session_id.as_deref() {
            match storage
                .get_session(parent_id)
                .map_err(|e| SessionError::Storage(format!("DB error: {}", e)))?
            {
                Some(session) if session.role == SessionRole::Conductor => Some(session),
                _ => {
                    return Err(SessionError::Storage(
                        "Child sessions require a conductor parent".to_string(),
                    ));
                }
            }
        } else {
            None
        };
        let parent_session_id = options.parent_session_id.clone().unwrap_or_default();
        let group_path = parent_session
            .as_ref()
            .map(|session| session.group_path.clone())
            .or_else(|| options.group_path.clone())
            .unwrap_or_else(|| crate::core::groups::DEFAULT_GROUP_PATH.to_string());
        let conductor_config = options.conductor_config.clone();
        let id = uuid::Uuid::new_v4().to_string();
        let tmux_name = tmux::generate_session_name(&title);

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

        let launch_ctx =
            build_runner_launch_context(working_dir.clone(), id.clone(), requested_mcp_selection);
        let launch = build_session_launch(tool, explicit_command, &launch_ctx, hook_warning)?;

        // NOTE: if tmux::create_session fails here, a freshly created worktree
        // at `working_dir` is leaked on disk. Task 8's orphan sweep is the
        // recovery path; no inline rollback to keep the failure message simple.
        create_tmux_session(
            &tmux_name,
            launch.command.as_deref(),
            Some(&working_dir),
            Some(&launch.env),
        )?;

        cache.register(&tmux_name);

        let session = Session {
            id: id.clone(),
            title,
            project_path: options.project_path,
            group_path,
            order: storage.load_sessions().unwrap_or_default().len() as i32,
            command: launch.command.unwrap_or_default(),
            wrapper: String::new(),
            tool,
            status: SessionStatus::Running,
            tmux_session: tmux_name,
            created_at: now,
            last_accessed: now,
            parent_session_id,
            role,
            conductor_expanded: role == SessionRole::Conductor,
            worktree_path,
            worktree_repo,
            worktree_branch,
            tool_data: "{}".to_string(),
            mcp_selection,
            acknowledged: false,
            notify: false,
            follow_up: false,
            user_waiting: false,
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
        if role == SessionRole::Conductor {
            let mut config = conductor_config
                .unwrap_or_else(|| ConductorConfig::default_for_session(session.id.clone()));
            config.session_id = session.id.clone();
            storage.save_conductor_config(&config).map_err(|e| {
                SessionError::Storage(format!("Failed to save conductor config: {}", e))
            })?;
        }
        storage.touch().ok();

        Ok((session, launch.warning))
    }

    /// Stop a session (kill tmux but keep the record)
    pub fn stop_session(&self, storage: &Storage, session_id: &str) -> SessionResult<()> {
        let session = storage
            .get_session(session_id)
            .map_err(|e| SessionError::Storage(format!("DB error: {}", e)))?
            .ok_or(SessionError::NotFound)?;

        if !session.tmux_session.is_empty() {
            tmux::kill_session(&session.tmux_session)?;
            delete_event_files(session_id);
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
            delete_event_files(session_id);
        }

        storage.delete_cost_events_for_session(session_id).ok();
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

        let launch_ctx = build_runner_launch_context(
            session.project_path.clone(),
            session.id.clone(),
            Some(session.mcp_selection.clone()),
        );
        let launch = build_restart_launch(
            session.tool,
            &session.command,
            &session.tool_data,
            &launch_ctx,
        )?;

        if !session.tmux_session.is_empty() {
            if tmux::session_exists(&session.tmux_session) {
                tmux::kill_session(&session.tmux_session)?;
            }
            cache.remove(&session.tmux_session);
        }

        let new_tmux_name = tmux::generate_session_name(&session.title);
        // Empty string is the storage sentinel for "no command, use tmux
        // default-shell" — see `Tool::Shell` and `ShellRunner::launch_command`.
        tmux::create_session(
            &new_tmux_name,
            launch.command.as_deref(),
            Some(&session.project_path),
            Some(&launch.env),
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

    /// Find detached Agent View tmux sessions that no stored session references.
    pub fn find_orphan_tmux_sessions(&self, storage: &Storage) -> SessionResult<Vec<String>> {
        let tmux_sessions = tmux::list_sessions()?;
        self.find_orphan_tmux_sessions_from(storage, &tmux_sessions)
    }

    fn find_orphan_tmux_sessions_from(
        &self,
        storage: &Storage,
        tmux_sessions: &[tmux::TmuxSessionInfo],
    ) -> SessionResult<Vec<String>> {
        let sessions = storage
            .load_sessions()
            .map_err(|e| SessionError::Storage(format!("DB error: {}", e)))?;
        let known: HashSet<String> = sessions
            .iter()
            .map(|s| s.tmux_session.clone())
            .filter(|name| !name.is_empty())
            .collect();

        Ok(tmux_sessions
            .iter()
            .filter(|session| session.name.starts_with(tmux::SESSION_PREFIX))
            .filter(|session| !session.attached)
            .filter(|session| !known.contains(&session.name))
            .map(|session| session.name.clone())
            .collect())
    }

    /// Kill detached Agent View tmux sessions that are no longer in storage.
    pub fn cleanup_orphan_tmux_sessions(
        &self,
        storage: &Storage,
        cache: &mut SessionCache,
    ) -> SessionResult<Vec<String>> {
        let orphan_sessions = self.find_orphan_tmux_sessions(storage)?;
        for name in &orphan_sessions {
            tmux::kill_session(name)?;
            cache.remove(name);
        }
        Ok(orphan_sessions)
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
            delete_event_files(session_id);
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

        storage.delete_cost_events_for_session(session_id).ok();
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

fn delete_event_files(session_id: &str) {
    delete_event_files_in(
        &crate::core::paths::hooks_dir(),
        &crate::core::paths::cost_events_dir(),
        session_id,
    );
}

/// Same as [`delete_event_files`] but with explicit directories. Splitting
/// the directories out lets unit tests run against a tempdir without
/// touching `~/.agent-orchestrator`.
fn delete_event_files_in(
    hooks_dir: &std::path::Path,
    cost_dir: &std::path::Path,
    session_id: &str,
) {
    let hook = hooks_dir.join(format!("{}.json", session_id));
    let _ = std::fs::remove_file(&hook);

    if let Ok(entries) = std::fs::read_dir(cost_dir) {
        let prefix = format!("{}_", session_id);
        for e in entries.flatten() {
            if let Some(name) = e.file_name().to_str() {
                if name.starts_with(&prefix) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }
}

fn build_runner_launch_context(
    working_dir: String,
    session_id: String,
    mcp_selection: Option<McpSelection>,
) -> RunnerLaunchContext {
    RunnerLaunchContext {
        working_dir,
        session_id,
        mcp_selection,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionLaunch {
    command: Option<String>,
    env: HashMap<String, String>,
    warning: Option<String>,
}

fn create_tmux_session(
    name: &str,
    command: Option<&str>,
    cwd: Option<&str>,
    env: Option<&HashMap<String, String>>,
) -> SessionResult<()> {
    #[cfg(test)]
    if test_support::should_skip_tmux_create() {
        return Ok(());
    }

    tmux::create_session(name, command, cwd, env)?;
    Ok(())
}

fn build_session_launch(
    tool: Tool,
    explicit_command: Option<String>,
    ctx: &RunnerLaunchContext,
    hook_warning: Option<String>,
) -> Result<SessionLaunch, RunnerLaunchError> {
    let runner_launch = if let Some(command) = explicit_command {
        RunnerLaunch {
            command: Some(command),
            env: HashMap::new(),
            warning: None,
        }
    } else {
        runner_for(tool).build_launch(ctx)?
    };

    Ok(merge_runner_launch(
        &ctx.session_id,
        runner_launch,
        hook_warning,
    ))
}

fn build_restart_launch(
    tool: Tool,
    original_command: &str,
    tool_data: &str,
    ctx: &RunnerLaunchContext,
) -> Result<SessionLaunch, RunnerLaunchError> {
    let runner = runner_for(tool);
    let restart_command = runner.restart_command(original_command, tool_data);
    let runner_launch = runner.build_launch(ctx)?;
    let command =
        compose_restart_command(runner_launch.command, original_command, &restart_command);

    Ok(merge_runner_launch(
        &ctx.session_id,
        RunnerLaunch {
            command,
            env: runner_launch.env,
            warning: runner_launch.warning,
        },
        None,
    ))
}

fn compose_restart_command(
    launch_command: Option<String>,
    original_command: &str,
    restart_command: &str,
) -> Option<String> {
    if restart_command.is_empty() {
        return None;
    }

    if restart_command == original_command {
        return launch_command.or_else(|| Some(restart_command.to_string()));
    }

    let Some(launch_command) = launch_command else {
        return Some(restart_command.to_string());
    };

    let suffix = command_suffix_after_program(restart_command);
    Some(format!("{}{}", launch_command, suffix))
}

fn command_suffix_after_program(command: &str) -> &str {
    command
        .char_indices()
        .find(|(_, c)| c.is_whitespace())
        .map(|(idx, _)| &command[idx..])
        .unwrap_or("")
}

fn merge_runner_launch(
    session_id: &str,
    runner_launch: RunnerLaunch,
    hook_warning: Option<String>,
) -> SessionLaunch {
    let mut env = runner_launch.env;
    env.insert(
        "AGENT_ORCHESTRATOR_SESSION".to_string(),
        session_id.to_string(),
    );
    env.insert("AGENT_VIEW_SESSION_ID".to_string(), session_id.to_string());

    SessionLaunch {
        command: runner_launch.command,
        env,
        warning: combine_warnings(hook_warning, runner_launch.warning),
    }
}

fn combine_warnings(first: Option<String>, second: Option<String>) -> Option<String> {
    match (first, second) {
        (Some(first), Some(second)) => Some(format!("{}; {}", first, second)),
        (Some(first), None) => Some(first),
        (None, Some(second)) => Some(second),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tmux::SessionCache;
    use crate::types::{
        ConductorMode, SessionCreateOptions, SessionRole, Tool, WorktreeCreateOptions,
    };
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
    fn test_find_orphan_tmux_sessions_filters_to_detached_untracked_agent_sessions() {
        let (storage, _db_dir) = crate::core::storage::test_helpers::test_storage();
        let mut known = crate::core::storage::test_helpers::make_test_session("known");
        known.tmux_session = "agentorch_known".to_string();
        storage.save_session(&known).unwrap();

        let tmux_sessions = vec![
            crate::core::tmux::TmuxSessionInfo {
                name: "agentorch_known".to_string(),
                attached: false,
            },
            crate::core::tmux::TmuxSessionInfo {
                name: "agentorch_orphan".to_string(),
                attached: false,
            },
            crate::core::tmux::TmuxSessionInfo {
                name: "agentorch_attached-orphan".to_string(),
                attached: true,
            },
            crate::core::tmux::TmuxSessionInfo {
                name: "__agentview_meta_usage".to_string(),
                attached: false,
            },
            crate::core::tmux::TmuxSessionInfo {
                name: "personal".to_string(),
                attached: false,
            },
        ];

        let orphans = SessionOps
            .find_orphan_tmux_sessions_from(&storage, &tmux_sessions)
            .unwrap();

        assert_eq!(orphans, vec!["agentorch_orphan".to_string()]);
    }

    fn create_options(title: &str, role: SessionRole) -> SessionCreateOptions {
        SessionCreateOptions {
            title: Some(title.to_string()),
            project_path: "/tmp".to_string(),
            group_path: None,
            tool: Tool::Shell,
            command: Some("true".to_string()),
            mcp_selection: None,
            role,
            parent_session_id: None,
            conductor_config: None,
            worktree: None,
        }
    }

    #[test]
    fn test_create_conductor_session_persists_default_config() {
        let _guard = test_support::skip_tmux_create();
        let (storage, _db_dir) = crate::core::storage::test_helpers::test_storage();
        let mut cache = SessionCache::new();
        let ops = SessionOps;

        let (session, _) = ops
            .create_session(
                &storage,
                &mut cache,
                create_options("conductor-test", SessionRole::Conductor),
            )
            .unwrap();

        let config = storage.get_conductor_config(&session.id).unwrap();
        let loaded = storage.get_session(&session.id).unwrap().unwrap();

        let config = config.unwrap();
        assert_eq!(config.session_id, session.id);
        assert_eq!(config.mode, ConductorMode::Autonomous);
        assert_eq!(config.heartbeat_secs, 900);
        assert_eq!(loaded.role, SessionRole::Conductor);
        assert!(loaded.conductor_expanded);
    }

    #[test]
    fn test_create_child_session_persists_parent() {
        let _guard = test_support::skip_tmux_create();
        let (storage, _db_dir) = crate::core::storage::test_helpers::test_storage();
        let mut cache = SessionCache::new();
        let ops = SessionOps;

        let mut parent_options = create_options("parent-conductor", SessionRole::Conductor);
        parent_options.group_path = Some("conductors/project-a".to_string());
        let (parent, _) = ops
            .create_session(&storage, &mut cache, parent_options)
            .unwrap();

        let mut child_options = create_options("child-session", SessionRole::Normal);
        child_options.parent_session_id = Some(parent.id.clone());
        let (child, _) = ops
            .create_session(&storage, &mut cache, child_options)
            .unwrap();

        let loaded_child = storage.get_session(&child.id).unwrap().unwrap();

        assert_eq!(loaded_child.parent_session_id, parent.id);
        assert_eq!(loaded_child.group_path, parent.group_path);
    }

    #[test]
    fn test_create_child_session_requires_conductor_parent() {
        let _guard = test_support::skip_tmux_create();
        let (storage, _db_dir) = crate::core::storage::test_helpers::test_storage();
        let mut cache = SessionCache::new();
        let ops = SessionOps;

        let (parent, _) = ops
            .create_session(
                &storage,
                &mut cache,
                create_options("normal-parent", SessionRole::Normal),
            )
            .unwrap();

        let mut child_options = create_options("child-session", SessionRole::Normal);
        child_options.parent_session_id = Some(parent.id.clone());
        let result = ops.create_session(&storage, &mut cache, child_options);

        let err = result.unwrap_err();
        assert!(
            matches!(err, SessionError::Storage(message) if message.contains("conductor parent"))
        );
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
                    mcp_selection: None,
                    role: crate::types::SessionRole::Normal,
                    parent_session_id: None,
                    conductor_config: None,
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
                    mcp_selection: None,
                    role: crate::types::SessionRole::Normal,
                    parent_session_id: None,
                    conductor_config: None,
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

    #[test]
    fn test_delete_event_files_in_removes_hook_and_cost_files() {
        use std::fs;
        let dir = tempfile::tempdir().unwrap();
        let hooks = dir.path().join("hooks");
        let costs = dir.path().join("cost-events");
        fs::create_dir(&hooks).unwrap();
        fs::create_dir(&costs).unwrap();

        let id = "sess-X";
        let hook = hooks.join(format!("{}.json", id));
        let cost1 = costs.join(format!("{}_1.json", id));
        let cost2 = costs.join(format!("{}_2.json", id));
        // A cost file for an UNRELATED session must NOT be touched.
        let other = costs.join("sess-Y_1.json");

        for p in [&hook, &cost1, &cost2, &other] {
            fs::write(p, "{}").unwrap();
        }

        super::delete_event_files_in(&hooks, &costs, id);

        assert!(!hook.exists());
        assert!(!cost1.exists());
        assert!(!cost2.exists());
        assert!(other.exists(), "files for other sessions must be preserved");
    }

    #[test]
    fn test_delete_event_files_in_handles_missing_dirs() {
        // Should not panic when neither dir exists yet.
        let dir = tempfile::tempdir().unwrap();
        let hooks = dir.path().join("does-not-exist-hooks");
        let costs = dir.path().join("does-not-exist-costs");
        super::delete_event_files_in(&hooks, &costs, "anything");
    }

    mod launch_tests {
        use crate::core::mcp::{McpSelection, McpServerSelection};
        use crate::core::runner::RunnerLaunch;
        use crate::types::Tool;
        use std::collections::HashMap;

        #[test]
        fn build_runner_launch_context_carries_mcp_selection() {
            let selection = McpSelection {
                profile_id: Some("profile-rust".to_string()),
                servers: vec![McpServerSelection {
                    id: "GitLabMITRE".to_string(),
                    enabled: true,
                    selected_tools: Some(vec!["list_issues".to_string()]),
                }],
            };

            let ctx = super::super::build_runner_launch_context(
                "/tmp/project".to_string(),
                "session-123".to_string(),
                Some(selection.clone()),
            );

            assert_eq!(ctx.working_dir, "/tmp/project");
            assert_eq!(ctx.session_id, "session-123");
            assert_eq!(ctx.mcp_selection, Some(selection));
        }

        #[test]
        fn build_session_launch_uses_explicit_command() {
            let ctx = super::super::build_runner_launch_context(
                "/tmp/project".to_string(),
                "session-123".to_string(),
                None,
            );

            let launch = super::super::build_session_launch(
                Tool::Claude,
                Some("echo explicit".to_string()),
                &ctx,
                None,
            )
            .unwrap();

            assert_eq!(launch.command.as_deref(), Some("echo explicit"));
            assert_eq!(launch.env["AGENT_VIEW_SESSION_ID"], "session-123");
            assert_eq!(launch.env["AGENT_ORCHESTRATOR_SESSION"], "session-123");
            assert_eq!(launch.warning, None);
        }

        #[test]
        fn merge_runner_launch_preserves_base_env_and_combines_warnings() {
            let mut runner_env = HashMap::new();
            runner_env.insert("RUNNER_ONLY".to_string(), "1".to_string());
            runner_env.insert("AGENT_VIEW_SESSION_ID".to_string(), "runner".to_string());

            let launch = super::super::merge_runner_launch(
                "session-123",
                RunnerLaunch {
                    command: Some("claude".to_string()),
                    env: runner_env,
                    warning: Some("runner warning".to_string()),
                },
                Some("hook warning".to_string()),
            );

            assert_eq!(launch.command.as_deref(), Some("claude"));
            assert_eq!(launch.env["RUNNER_ONLY"], "1");
            assert_eq!(launch.env["AGENT_VIEW_SESSION_ID"], "session-123");
            assert_eq!(launch.env["AGENT_ORCHESTRATOR_SESSION"], "session-123");
            assert_eq!(
                launch.warning.as_deref(),
                Some("hook warning; runner warning")
            );
        }

        #[test]
        fn build_restart_launch_reapplies_mcp_launch_when_restart_falls_back_to_original() {
            let ctx = super::super::build_runner_launch_context(
                "/tmp/project".to_string(),
                "session-123".to_string(),
                Some(McpSelection {
                    profile_id: Some("no-browser".to_string()),
                    servers: vec![McpServerSelection {
                        id: "browser".to_string(),
                        enabled: false,
                        selected_tools: None,
                    }],
                }),
            );

            let launch =
                super::super::build_restart_launch(Tool::Codex, "codex", "{}", &ctx).unwrap();

            assert_eq!(
                launch.command.as_deref(),
                Some("codex -c mcp_servers.browser.enabled=false")
            );
            assert_eq!(launch.env["AGENT_VIEW_SESSION_ID"], "session-123");
            assert_eq!(launch.env["AGENT_ORCHESTRATOR_SESSION"], "session-123");
        }
    }
}
