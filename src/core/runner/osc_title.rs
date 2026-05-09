//! Tier-2 status signal: read tmux pane title and match against a small
//! known-marker set. Returns None if tmux call fails or no marker matches.
//!
//! Marker set is intentionally tiny — verified against live Claude Code
//! during implementation (issue #45 open question 2). Easy to extend later.

use crate::types::SessionStatus;
use std::process::Command;

/// Match a trimmed pane-title string against known Claude markers.
pub fn match_marker(title: &str) -> Option<SessionStatus> {
    match title.trim() {
        // Conservative initial set; expand with verified Claude markers.
        "Working" => Some(SessionStatus::Running),
        "Idle" => Some(SessionStatus::Idle),
        _ => None,
    }
}

/// Read tmux pane title for a session and apply `match_marker`.
pub fn check_pane_title(tmux_session: &str) -> Option<SessionStatus> {
    let output = Command::new("tmux")
        .args(["display-message", "-p", "-t", tmux_session, "#{pane_title}"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let title = String::from_utf8_lossy(&output.stdout);
    match_marker(&title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_match_marker_known_strings() {
        assert_eq!(match_marker("Working"), Some(SessionStatus::Running));
        assert_eq!(match_marker(" Idle\n"), Some(SessionStatus::Idle));
    }

    #[test]
    fn test_match_marker_unknown_returns_none() {
        assert_eq!(match_marker("anything else"), None);
        assert_eq!(match_marker(""), None);
    }
}
