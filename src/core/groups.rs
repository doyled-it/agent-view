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
    Session(Box<Session>),
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

/// Sort a slice of session references according to the given sort mode.
/// Pinned sessions always float to the top regardless of sort mode.
pub fn sort_sessions(sessions: &mut [&Session], mode: crate::types::SortMode) {
    sessions.sort_by(|a, b| {
        // Pinned sessions always come first
        match (b.pinned, a.pinned) {
            (true, false) => return std::cmp::Ordering::Greater,
            (false, true) => return std::cmp::Ordering::Less,
            _ => {}
        }
        match mode {
            crate::types::SortMode::StatusPriority => {
                a.status.sort_priority().cmp(&b.status.sort_priority())
            }
            crate::types::SortMode::LastActivity => b.status_changed_at.cmp(&a.status_changed_at),
            crate::types::SortMode::Name => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
            crate::types::SortMode::Created => b.created_at.cmp(&a.created_at),
        }
    });
}

/// Flatten groups and sessions into a navigable list.
/// Groups appear as headers; if expanded, their sessions follow.
/// Orphan sessions (in groups that don't exist) get an implicit group.
pub fn flatten_group_tree(
    sessions: &[Session],
    groups: &[Group],
    sort_mode: crate::types::SortMode,
) -> Vec<ListRow> {
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

    // Sort sessions within each group by the requested sort mode
    for group_sessions in by_group.values_mut() {
        sort_sessions(group_sessions, sort_mode);
    }

    let known_paths: std::collections::HashSet<&str> =
        sorted_groups.iter().map(|g| g.path.as_str()).collect();

    for group in &sorted_groups {
        let group_sessions = by_group
            .get(&group.path)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);

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
                result.push(ListRow::Session(Box::new((*session).clone())));
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
            result.push(ListRow::Session(Box::new((*session).clone())));
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{SessionStatus, SortMode, Tool};

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
        let rows = flatten_group_tree(&sessions, &groups, SortMode::Created);
        assert_eq!(rows.len(), 3); // 1 group header + 2 sessions
        assert!(matches!(rows[0], ListRow::Group { .. }));
        assert!(matches!(rows[1], ListRow::Session(_)));
    }

    #[test]
    fn test_flatten_collapsed_group_hides_sessions() {
        let mut group = make_group("work", "Work", 0);
        group.expanded = false;
        let sessions = vec![make_session("s1", "work", SessionStatus::Idle)];
        let rows = flatten_group_tree(&sessions, &[group], SortMode::Created);
        assert_eq!(rows.len(), 1); // only group header
    }

    #[test]
    fn test_flatten_orphan_sessions_get_implicit_group() {
        let groups = vec![make_group("work", "Work", 0)];
        let sessions = vec![make_session("s1", "unknown", SessionStatus::Idle)];
        let rows = flatten_group_tree(&sessions, &groups, SortMode::Created);
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
        let rows = flatten_group_tree(&sessions, &groups, SortMode::Created);
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
        let rows = flatten_group_tree(&sessions, &groups, SortMode::Created);
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

    #[test]
    fn test_pinned_sessions_sort_first() {
        let s1 = make_session("s1", "work", SessionStatus::Idle);
        let mut s2 = make_session("s2", "work", SessionStatus::Idle);
        s2.pinned = true;
        let groups = vec![make_group("work", "Work", 0)];
        let rows = flatten_group_tree(&[s1, s2], &groups, SortMode::Created);
        // s2 is pinned so should appear first after group header
        if let ListRow::Session(first) = &rows[1] {
            assert_eq!(first.id, "s2");
            assert!(first.pinned);
        } else {
            panic!("Expected session row");
        }
    }

    #[test]
    fn test_sort_sessions_by_status_priority() {
        let mut s1 = make_session("s1", "work", SessionStatus::Idle);
        s1.created_at = 1700000000003;
        let mut s2 = make_session("s2", "work", SessionStatus::Waiting);
        s2.created_at = 1700000000002;
        let mut s3 = make_session("s3", "work", SessionStatus::Running);
        s3.created_at = 1700000000001;
        let groups = vec![make_group("work", "Work", 0)];
        let rows = flatten_group_tree(&[s1, s2, s3], &groups, SortMode::StatusPriority);
        // Group header + 3 sessions
        if let ListRow::Session(first) = &rows[1] {
            assert_eq!(first.id, "s2");
        } // waiting first
        if let ListRow::Session(second) = &rows[2] {
            assert_eq!(second.id, "s3");
        } // running second
        if let ListRow::Session(third) = &rows[3] {
            assert_eq!(third.id, "s1");
        } // idle last
    }

    #[test]
    fn test_group_ordering_by_order_field() {
        // Groups should appear in order of their `order` field
        let groups = vec![
            make_group("group-b", "B Group", 2),
            make_group("group-a", "A Group", 1),
            make_group("group-c", "C Group", 3),
        ];
        let sessions = vec![
            make_session("s1", "group-a", SessionStatus::Idle),
            make_session("s2", "group-b", SessionStatus::Idle),
            make_session("s3", "group-c", SessionStatus::Idle),
        ];
        let rows = flatten_group_tree(&sessions, &groups, SortMode::Name);
        // All group rows in order
        let group_names: Vec<&str> = rows
            .iter()
            .filter_map(|r| {
                if let ListRow::Group { group, .. } = r {
                    Some(group.name.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(group_names, vec!["A Group", "B Group", "C Group"]);
    }

    #[test]
    fn test_sessions_partitioned_to_correct_groups() {
        let groups = vec![
            make_group("team-a", "Team A", 0),
            make_group("team-b", "Team B", 1),
        ];
        let sessions = vec![
            make_session("s1", "team-a", SessionStatus::Idle),
            make_session("s2", "team-a", SessionStatus::Running),
            make_session("s3", "team-b", SessionStatus::Waiting),
        ];
        let rows = flatten_group_tree(&sessions, &groups, SortMode::Name);

        // Find Team A header and verify its session_count
        let team_a_row = rows
            .iter()
            .find(|r| matches!(r, ListRow::Group { group, .. } if group.path == "team-a"));
        assert!(team_a_row.is_some());
        if let Some(ListRow::Group { session_count, .. }) = team_a_row {
            assert_eq!(*session_count, 2);
        }

        // Find Team B header and verify its session_count
        let team_b_row = rows
            .iter()
            .find(|r| matches!(r, ListRow::Group { group, .. } if group.path == "team-b"));
        if let Some(ListRow::Group { session_count, .. }) = team_b_row {
            assert_eq!(*session_count, 1);
        }
    }

    #[test]
    fn test_non_default_empty_group_still_shown() {
        // Non-default groups with no sessions should still appear in the list
        let groups = vec![make_group("work", "Work", 0)];
        let sessions: Vec<_> = vec![]; // no sessions
        let rows = flatten_group_tree(&sessions, &groups, SortMode::Name);
        // The work group has no sessions but should still show (only default is hidden when empty)
        let has_work = rows
            .iter()
            .any(|r| matches!(r, ListRow::Group { group, .. } if group.path == "work"));
        assert!(has_work, "non-default empty group should still be visible");
    }

    #[test]
    fn test_default_group_hidden_when_empty() {
        // Default group should not appear when it has no sessions
        let groups = vec![make_group(DEFAULT_GROUP_PATH, DEFAULT_GROUP_NAME, 0)];
        let sessions: Vec<_> = vec![];
        let rows = flatten_group_tree(&sessions, &groups, SortMode::Name);
        assert!(rows.is_empty(), "default group should be hidden when empty");
    }

    #[test]
    fn test_default_group_routing_empty_group_path() {
        // Sessions with empty group_path should land in the default group
        let sessions = vec![make_session("s1", "", SessionStatus::Idle)]; // empty group_path
        let groups = vec![make_group(DEFAULT_GROUP_PATH, DEFAULT_GROUP_NAME, 0)];
        let rows = flatten_group_tree(&sessions, &groups, SortMode::Name);
        // Default group should now appear (non-empty) and contain our session
        assert!(!rows.is_empty());
        let has_default = rows
            .iter()
            .any(|r| matches!(r, ListRow::Group { group, .. } if group.path == DEFAULT_GROUP_PATH));
        assert!(
            has_default,
            "session with empty group_path should route to default group"
        );
        let session_row = rows
            .iter()
            .find(|r| matches!(r, ListRow::Session(s) if s.id == "s1"));
        assert!(
            session_row.is_some(),
            "session should appear under default group"
        );
    }

    #[test]
    fn test_sort_sessions_by_name() {
        let s1 = make_session("s1", "work", SessionStatus::Idle);
        let s2 = make_session("s2", "work", SessionStatus::Idle);
        let mut s1_ref = s1.clone();
        let mut s2_ref = s2.clone();
        s1_ref.title = "Zephyr".to_string();
        s2_ref.title = "Aardvark".to_string();
        let groups = vec![make_group("work", "Work", 0)];
        let rows = flatten_group_tree(&[s1_ref, s2_ref], &groups, SortMode::Name);
        if let (Some(ListRow::Session(first)), Some(ListRow::Session(second))) =
            (rows.get(1), rows.get(2))
        {
            assert_eq!(first.title, "Aardvark");
            assert_eq!(second.title, "Zephyr");
        } else {
            panic!("Expected two session rows after group header");
        }
    }

    #[test]
    fn test_sort_sessions_by_last_activity() {
        let mut s1 = make_session("s1", "work", SessionStatus::Idle);
        s1.status_changed_at = 1000;
        let mut s2 = make_session("s2", "work", SessionStatus::Idle);
        s2.status_changed_at = 2000;
        let groups = vec![make_group("work", "Work", 0)];
        let rows = flatten_group_tree(&[s1, s2], &groups, SortMode::LastActivity);
        // Higher timestamp (more recent) comes first
        if let (Some(ListRow::Session(first)), Some(ListRow::Session(second))) =
            (rows.get(1), rows.get(2))
        {
            assert_eq!(first.id, "s2"); // status_changed_at = 2000
            assert_eq!(second.id, "s1"); // status_changed_at = 1000
        } else {
            panic!("Expected two session rows");
        }
    }

    #[test]
    fn test_sort_sessions_by_created() {
        let mut s1 = make_session("s1", "work", SessionStatus::Idle);
        s1.created_at = 1000;
        let mut s2 = make_session("s2", "work", SessionStatus::Idle);
        s2.created_at = 2000;
        let groups = vec![make_group("work", "Work", 0)];
        let rows = flatten_group_tree(&[s1, s2], &groups, SortMode::Created);
        // Newer created_at (descending) comes first
        if let (Some(ListRow::Session(first)), Some(ListRow::Session(second))) =
            (rows.get(1), rows.get(2))
        {
            assert_eq!(first.id, "s2"); // created_at = 2000
            assert_eq!(second.id, "s1"); // created_at = 1000
        } else {
            panic!("Expected two session rows");
        }
    }

    #[test]
    fn test_ensure_default_group_increments_existing_order() {
        let groups = vec![make_group("work", "Work", 0)];
        let result = ensure_default_group(&groups);
        // Default group inserted at front; existing group order incremented by 1
        let work = result.iter().find(|g| g.path == "work").unwrap();
        assert_eq!(work.order, 1);
    }
}
