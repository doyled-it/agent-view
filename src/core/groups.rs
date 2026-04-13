//! Group flattening logic — converts groups + sessions into a navigable list

use crate::types::{Group, Session};
use std::collections::HashMap;

pub const DEFAULT_GROUP_PATH: &str = "my-sessions";
pub const DEFAULT_GROUP_NAME: &str = "Ungrouped";

/// A row in the flattened list — either a group header or a session
#[derive(Debug, Clone)]
pub enum ListRow {
    Group {
        group: Group,
        session_count: usize,
        running_count: usize,
        waiting_count: usize,
    },
    Session(Session),
}

/// Ensure the default "Ungrouped" group exists in the list.
/// Returns the groups with the default group inserted if missing.
pub fn ensure_default_group(groups: &[Group]) -> Vec<Group> {
    if groups.iter().any(|g| g.path == DEFAULT_GROUP_PATH) {
        return groups.to_vec();
    }

    let default = Group {
        path: DEFAULT_GROUP_PATH.to_string(),
        name: DEFAULT_GROUP_NAME.to_string(),
        expanded: true,
        order: 0,
        default_path: String::new(),
    };

    let mut result = vec![default];
    for g in groups {
        let mut g = g.clone();
        g.order += 1;
        result.push(g);
    }
    result
}

/// Flatten groups and sessions into a navigable list.
/// Groups appear as headers; if expanded, their sessions follow.
/// Orphan sessions (in groups that don't exist) get an implicit group.
pub fn flatten_group_tree(sessions: &[Session], groups: &[Group]) -> Vec<ListRow> {
    let mut result = Vec::new();

    let mut sorted_groups = groups.to_vec();
    sorted_groups.sort_by_key(|g| g.order);

    // Build map: group_path -> Vec<Session>
    let mut by_group: HashMap<String, Vec<&Session>> = HashMap::new();
    for session in sessions {
        let path = if session.group_path.is_empty() {
            DEFAULT_GROUP_PATH.to_string()
        } else {
            session.group_path.clone()
        };
        by_group.entry(path).or_default().push(session);
    }

    // Sort sessions within each group by created_at descending
    for group_sessions in by_group.values_mut() {
        group_sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    }

    let known_paths: std::collections::HashSet<&str> =
        sorted_groups.iter().map(|g| g.path.as_str()).collect();

    for group in &sorted_groups {
        let group_sessions = by_group.get(&group.path).map(|v| v.as_slice()).unwrap_or(&[]);

        // Hide default group when empty
        if group.path == DEFAULT_GROUP_PATH && group_sessions.is_empty() {
            continue;
        }

        let running = group_sessions
            .iter()
            .filter(|s| s.status == crate::types::SessionStatus::Running)
            .count();
        let waiting = group_sessions
            .iter()
            .filter(|s| s.status == crate::types::SessionStatus::Waiting)
            .count();

        result.push(ListRow::Group {
            group: group.clone(),
            session_count: group_sessions.len(),
            running_count: running,
            waiting_count: waiting,
        });

        if group.expanded {
            for session in group_sessions {
                result.push(ListRow::Session((*session).clone()));
            }
        }
    }

    // Orphan sessions in unknown groups
    for (path, orphans) in &by_group {
        if known_paths.contains(path.as_str()) {
            continue;
        }
        let running = orphans
            .iter()
            .filter(|s| s.status == crate::types::SessionStatus::Running)
            .count();
        let waiting = orphans
            .iter()
            .filter(|s| s.status == crate::types::SessionStatus::Waiting)
            .count();

        result.push(ListRow::Group {
            group: Group {
                path: path.clone(),
                name: path.clone(),
                expanded: true,
                order: 999,
                default_path: String::new(),
            },
            session_count: orphans.len(),
            running_count: running,
            waiting_count: waiting,
        });

        for session in orphans {
            result.push(ListRow::Session((*session).clone()));
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{SessionStatus, Tool};

    fn make_session(id: &str, group: &str, status: SessionStatus) -> Session {
        Session {
            id: id.to_string(),
            title: format!("Session {}", id),
            project_path: "/tmp".to_string(),
            group_path: group.to_string(),
            order: 0,
            command: String::new(),
            wrapper: String::new(),
            tool: Tool::Claude,
            status,
            tmux_session: String::new(),
            created_at: 1700000000000,
            last_accessed: 0,
            parent_session_id: String::new(),
            worktree_path: String::new(),
            worktree_repo: String::new(),
            worktree_branch: String::new(),
            tool_data: "{}".to_string(),
            acknowledged: false,
            notify: false,
            follow_up: false,
            status_changed_at: 0,
            restart_count: 0,
            status_history: vec![],
            pinned: false,
            tokens_used: 0,
        }
    }

    fn make_group(path: &str, name: &str, order: i32) -> Group {
        Group {
            path: path.to_string(),
            name: name.to_string(),
            expanded: true,
            order,
            default_path: String::new(),
        }
    }

    #[test]
    fn test_ensure_default_group_adds_when_missing() {
        let groups = vec![make_group("work", "Work", 0)];
        let result = ensure_default_group(&groups);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].path, DEFAULT_GROUP_PATH);
    }

    #[test]
    fn test_ensure_default_group_noop_when_present() {
        let groups = vec![make_group(DEFAULT_GROUP_PATH, DEFAULT_GROUP_NAME, 0)];
        let result = ensure_default_group(&groups);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_flatten_basic() {
        let groups = vec![make_group("work", "Work", 0)];
        let sessions = vec![
            make_session("s1", "work", SessionStatus::Running),
            make_session("s2", "work", SessionStatus::Idle),
        ];
        let rows = flatten_group_tree(&sessions, &groups);
        assert_eq!(rows.len(), 3); // 1 group header + 2 sessions
        assert!(matches!(rows[0], ListRow::Group { .. }));
        assert!(matches!(rows[1], ListRow::Session(_)));
    }

    #[test]
    fn test_flatten_collapsed_group_hides_sessions() {
        let mut group = make_group("work", "Work", 0);
        group.expanded = false;
        let sessions = vec![make_session("s1", "work", SessionStatus::Idle)];
        let rows = flatten_group_tree(&sessions, &[group]);
        assert_eq!(rows.len(), 1); // only group header
    }

    #[test]
    fn test_flatten_orphan_sessions_get_implicit_group() {
        let groups = vec![make_group("work", "Work", 0)];
        let sessions = vec![make_session("s1", "unknown", SessionStatus::Idle)];
        let rows = flatten_group_tree(&sessions, &groups);
        // work group (empty, but it's not default so still shows) + unknown group + session
        assert!(rows.len() >= 2);
    }

    #[test]
    fn test_flatten_empty_default_group_hidden() {
        let groups = vec![
            make_group(DEFAULT_GROUP_PATH, DEFAULT_GROUP_NAME, 0),
            make_group("work", "Work", 1),
        ];
        let sessions = vec![make_session("s1", "work", SessionStatus::Idle)];
        let rows = flatten_group_tree(&sessions, &groups);
        // Default group hidden (empty), work group + session
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn test_flatten_counts_statuses() {
        let groups = vec![make_group("work", "Work", 0)];
        let sessions = vec![
            make_session("s1", "work", SessionStatus::Running),
            make_session("s2", "work", SessionStatus::Waiting),
            make_session("s3", "work", SessionStatus::Idle),
        ];
        let rows = flatten_group_tree(&sessions, &groups);
        if let ListRow::Group {
            running_count,
            waiting_count,
            session_count,
            ..
        } = &rows[0]
        {
            assert_eq!(*running_count, 1);
            assert_eq!(*waiting_count, 1);
            assert_eq!(*session_count, 3);
        } else {
            panic!("Expected group row");
        }
    }
}
