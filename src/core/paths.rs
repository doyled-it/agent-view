//! Paths under the agent-view data directory.
//! Layout (matches existing `core::storage` convention):
//!     ~/.agent-orchestrator/          (data root)
//!     ~/.agent-orchestrator/hooks/    (hook status files, latest-wins)
//!     ~/.agent-orchestrator/cost-events/ (cost events, append-only)

use std::path::PathBuf;

pub fn agent_orchestrator_dir() -> PathBuf {
    dirs::home_dir()
        .expect("Cannot determine home directory")
        .join(".agent-orchestrator")
}

pub fn hooks_dir() -> PathBuf {
    agent_orchestrator_dir().join("hooks")
}

pub fn cost_events_dir() -> PathBuf {
    agent_orchestrator_dir().join("cost-events")
}

pub fn rollout_state_dir() -> PathBuf {
    agent_orchestrator_dir().join("rollout-state")
}

pub fn mcp_session_config_dir() -> PathBuf {
    agent_orchestrator_dir().join("mcp-session-configs")
}

/// Ensure subdirs exist (mode 0700 on unix). Returns Ok if already present.
pub fn ensure_event_dirs() -> std::io::Result<()> {
    use std::fs;
    let h = hooks_dir();
    let c = cost_events_dir();
    let r = rollout_state_dir();
    let m = mcp_session_config_dir();
    fs::create_dir_all(&h)?;
    fs::create_dir_all(&c)?;
    fs::create_dir_all(&r)?;
    fs::create_dir_all(&m)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for p in [&h, &c, &r, &m] {
            let mut perms = fs::metadata(p)?.permissions();
            perms.set_mode(0o700);
            let _ = fs::set_permissions(p, perms);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subdirs_under_root() {
        assert!(hooks_dir().starts_with(agent_orchestrator_dir()));
        assert!(cost_events_dir().starts_with(agent_orchestrator_dir()));
        assert!(hooks_dir().ends_with("hooks"));
        assert!(cost_events_dir().ends_with("cost-events"));
    }

    #[test]
    fn rollout_state_dir_is_under_agent_orchestrator() {
        assert!(rollout_state_dir().starts_with(agent_orchestrator_dir()));
        assert!(rollout_state_dir().ends_with("rollout-state"));
    }

    #[test]
    fn mcp_session_config_dir_is_under_agent_orchestrator() {
        assert!(mcp_session_config_dir().starts_with(agent_orchestrator_dir()));
        assert!(mcp_session_config_dir().ends_with("mcp-session-configs"));
    }
}
