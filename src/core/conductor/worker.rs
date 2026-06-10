use rusqlite::Result as SqlResult;
use std::thread;
use std::time::Duration;

use crate::core::conductor::actions::execute_action;
use crate::core::conductor::policy::{validate_action, PolicyDecision};
use crate::core::storage::{QueuedConductorAction, Storage};
use crate::types::{ConductorActionStatus, Session, SessionRole};

const WORKER_INTERVAL_SECS: u64 = 5;
const MAX_HEARTBEAT_PROMPT_CHARS: usize = 2_000;
const MAX_HEARTBEAT_CHILDREN: usize = 12;

pub fn due_conductors(storage: &Storage, now_ms: i64) -> SqlResult<Vec<Session>> {
    let mut due = Vec::new();
    for session in storage.load_sessions()? {
        if session.role != SessionRole::Conductor {
            continue;
        }
        let Some(config) = storage.get_conductor_config(&session.id)? else {
            continue;
        };
        let interval_ms = config.heartbeat_secs.max(0).saturating_mul(1_000);
        let next_heartbeat = config.last_heartbeat_at.saturating_add(interval_ms);
        if config.enabled && next_heartbeat <= now_ms {
            due.push(session);
        }
    }
    Ok(due)
}

pub fn process_queued_actions(storage: &Storage) -> Result<(), String> {
    let actions = storage
        .claim_queued_conductor_actions()
        .map_err(|e| e.to_string())?;
    let mut errors = Vec::new();

    for action in actions {
        let result = process_queued_action(storage, &action);
        if let Err(error) = result {
            errors.push(error);
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn process_queued_action(storage: &Storage, action: &QueuedConductorAction) -> Result<(), String> {
    let config = match storage.get_conductor_config(&action.conductor_session_id) {
        Ok(Some(config)) => config,
        Ok(None) => {
            return mark_action(
                storage,
                &action.id,
                ConductorActionStatus::Failed,
                "conductor config not found",
            );
        }
        Err(error) => {
            return mark_action(
                storage,
                &action.id,
                ConductorActionStatus::Failed,
                &error.to_string(),
            );
        }
    };

    let request = match action.to_request() {
        Ok(request) => request,
        Err(error) => {
            return mark_action(storage, &action.id, ConductorActionStatus::Failed, &error);
        }
    };

    let child_count = match storage.count_child_sessions(&action.conductor_session_id) {
        Ok(count) => count,
        Err(error) => {
            return mark_action(
                storage,
                &action.id,
                ConductorActionStatus::Failed,
                &error.to_string(),
            );
        }
    };

    match validate_action(&config, &request, child_count) {
        PolicyDecision::Allowed => {
            match execute_action(storage, &action.conductor_session_id, &request) {
                Ok(result) => mark_action(
                    storage,
                    &action.id,
                    ConductorActionStatus::Completed,
                    &result,
                ),
                Err(error) => {
                    mark_action(storage, &action.id, ConductorActionStatus::Failed, &error)
                }
            }
        }
        PolicyDecision::Blocked(reason) => {
            mark_action(storage, &action.id, ConductorActionStatus::Blocked, &reason)
        }
    }
}

fn mark_action(
    storage: &Storage,
    action_id: &str,
    status: ConductorActionStatus,
    result: &str,
) -> Result<(), String> {
    storage
        .update_conductor_action_status(action_id, status, result)
        .map_err(|e| format!("failed to update conductor action {}: {}", action_id, e))
}

pub fn build_heartbeat_prompt(conductor_id: &str, children: &[Session]) -> String {
    let mut child_lines = Vec::new();
    for child in children.iter().take(MAX_HEARTBEAT_CHILDREN) {
        child_lines.push(format!(
            "{}: {} [{}]",
            child.id,
            child.title.trim(),
            child.status.as_str()
        ));
    }
    if children.len() > MAX_HEARTBEAT_CHILDREN {
        child_lines.push(format!(
            "... {} more child session(s)",
            children.len() - MAX_HEARTBEAT_CHILDREN
        ));
    }
    let child_summary = if child_lines.is_empty() {
        "No child sessions.".to_string()
    } else {
        child_lines.join("; ")
    };
    let prompt = format!(
        concat!(
            "Conductor heartbeat. Do not use runner-native background agents for durable child work; ",
            "use Agent View child sessions so the user can see, enter, answer, and clean them up. ",
            "Queue JSON actions only when useful with: agent-view conductor-action {} '<request-json>'. ",
            "Spawn child example: {{\"action_type\":\"spawn_child\",\"payload\":{{\"title\":\"Short task name\",\"prompt\":\"Task instructions\"}}}}. ",
            "Other actions: mark_child_needs_user, send_child_response, record_child_summary. ",
            "Children: {}"
        ),
        conductor_id, child_summary
    );
    truncate_chars(&prompt, MAX_HEARTBEAT_PROMPT_CHARS)
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut truncated: String = value.chars().take(max_chars.saturating_sub(3)).collect();
    truncated.push_str("...");
    truncated
}

pub fn send_heartbeat_prompt(storage: &Storage, conductor: &Session) -> Result<(), String> {
    let children = storage
        .child_sessions(&conductor.id)
        .map_err(|e| e.to_string())?;
    let prompt = build_heartbeat_prompt(&conductor.id, &children);
    crate::core::tmux::send_keys(&conductor.tmux_session, &prompt).map_err(|e| e.to_string())
}

pub fn run_once(storage: &Storage, now_ms: i64) -> Result<(), String> {
    process_queued_actions(storage)?;
    let due = due_conductors(storage, now_ms).map_err(|e| e.to_string())?;
    let mut errors = Vec::new();
    for conductor in due {
        match storage.claim_due_conductor_heartbeat(&conductor.id, now_ms) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(error) => {
                errors.push(format!(
                    "failed to claim conductor heartbeat {}: {}",
                    conductor.id, error
                ));
                continue;
            }
        }

        match send_heartbeat_prompt(storage, &conductor) {
            Ok(()) => {}
            Err(error) => {
                if let Err(reset_error) =
                    make_heartbeat_due_for_retry(storage, &conductor.id, now_ms)
                {
                    errors.push(format!(
                        "failed to reset conductor heartbeat {} after send failure: {}",
                        conductor.id, reset_error
                    ));
                }
                errors.push(format!(
                    "failed to send conductor heartbeat {}: {}",
                    conductor.id, error
                ));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn make_heartbeat_due_for_retry(
    storage: &Storage,
    conductor_id: &str,
    now_ms: i64,
) -> Result<(), String> {
    let config = storage
        .get_conductor_config(conductor_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "conductor config not found".to_string())?;
    let interval_ms = config.heartbeat_secs.max(0).saturating_mul(1_000);
    let retry_due_timestamp = now_ms.saturating_sub(interval_ms);
    storage
        .update_conductor_last_heartbeat(conductor_id, retry_due_timestamp)
        .map_err(|e| e.to_string())
}

pub fn spawn() -> thread::JoinHandle<()> {
    thread::spawn(|| loop {
        match Storage::open_default() {
            Ok(storage) => {
                if let Err(error) = storage.migrate() {
                    eprintln!("agent-view: conductor worker migration warning: {}", error);
                } else {
                    let now_ms = chrono::Utc::now().timestamp_millis();
                    if let Err(error) = run_once(&storage, now_ms) {
                        eprintln!("agent-view: conductor worker warning: {}", error);
                    }
                }
            }
            Err(error) => eprintln!("agent-view: conductor worker storage warning: {}", error),
        }
        thread::sleep(Duration::from_secs(WORKER_INTERVAL_SECS));
    })
}

#[cfg(test)]
mod tests {
    use crate::core::storage::test_helpers::{make_test_session, test_storage};
    use crate::types::{
        ConductorActionRequest, ConductorActionStatus, ConductorActionType, ConductorConfig,
        SessionRole,
    };
    use serde_json::json;

    fn save_conductor(storage: &crate::core::storage::Storage, id: &str, config: ConductorConfig) {
        let mut conductor = make_test_session(id);
        conductor.role = SessionRole::Conductor;
        storage.save_session(&conductor).unwrap();
        storage.save_conductor_config(&config).unwrap();
    }

    #[test]
    fn due_conductors_returns_enabled_conductors_with_due_heartbeat() {
        let (storage, _dir) = test_storage();
        let mut config = ConductorConfig::default_for_session("conductor-1".to_string());
        config.heartbeat_secs = 5;
        config.last_heartbeat_at = 5_000;
        save_conductor(&storage, "conductor-1", config);

        let due = super::due_conductors(&storage, 10_000).unwrap();

        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, "conductor-1");
    }

    #[test]
    fn due_conductors_skips_conductors_with_future_heartbeat_deadline() {
        let (storage, _dir) = test_storage();
        let mut config = ConductorConfig::default_for_session("conductor-1".to_string());
        config.heartbeat_secs = 5;
        config.last_heartbeat_at = 6_000;
        save_conductor(&storage, "conductor-1", config);

        let due = super::due_conductors(&storage, 10_000).unwrap();

        assert!(due.is_empty());
    }

    #[test]
    fn due_conductors_skips_disabled_conductors() {
        let (storage, _dir) = test_storage();
        let mut config = ConductorConfig::default_for_session("conductor-1".to_string());
        config.heartbeat_secs = 5;
        config.last_heartbeat_at = 0;
        config.enabled = false;
        save_conductor(&storage, "conductor-1", config);

        let due = super::due_conductors(&storage, 10_000).unwrap();

        assert!(due.is_empty());
    }

    #[test]
    fn heartbeat_prompt_instructs_conductors_to_use_agent_view_child_sessions() {
        let mut child = make_test_session("child-1");
        child.title = "Investigate issue".to_string();
        child.status = crate::types::SessionStatus::Waiting;

        let prompt = super::build_heartbeat_prompt("conductor-1", &[child]);

        assert!(prompt.contains("Do not use runner-native background agents"));
        assert!(prompt.contains("agent-view conductor-action conductor-1"));
        assert!(prompt.contains(r#""action_type":"spawn_child""#));
        assert!(prompt.contains(r#""title":"Short task name""#));
        assert!(prompt.contains(r#""prompt":"Task instructions""#));
        assert!(prompt.contains("child-1: Investigate issue [waiting]"));
    }

    #[test]
    fn process_queued_actions_completes_allowed_record_child_summary() {
        let (storage, _dir) = test_storage();
        let config = ConductorConfig::default_for_session("conductor-1".to_string());
        save_conductor(&storage, "conductor-1", config);
        let mut child = make_test_session("child-1");
        child.parent_session_id = "conductor-1".to_string();
        storage.save_session(&child).unwrap();
        let request = ConductorActionRequest {
            action_type: ConductorActionType::RecordChildSummary,
            child_session_id: Some("child-1".to_string()),
            payload: json!({"summary": "ready"}),
        };
        let action_id = storage
            .enqueue_conductor_action("conductor-1", &request)
            .unwrap();

        super::process_queued_actions(&storage).unwrap();

        let row: (String, String) = storage
            .conn()
            .query_row(
                "SELECT status, result FROM conductor_actions WHERE id = ?1",
                [&action_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(row.0, ConductorActionStatus::Completed.as_str());
        assert_eq!(row.1, "summary recorded");

        let event: (String, String, String) = storage
            .conn()
            .query_row(
                "SELECT child_session_id, event_type, message
                 FROM conductor_events WHERE conductor_session_id = 'conductor-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(event.0, "child-1");
        assert_eq!(event.1, "summary");
        assert_eq!(event.2, "ready");
    }

    #[test]
    fn process_queued_actions_completes_allowed_spawn_child() {
        let _guard = crate::core::session::test_support::skip_tmux_create();
        let (storage, _dir) = test_storage();
        let config = ConductorConfig::default_for_session("conductor-1".to_string());
        save_conductor(&storage, "conductor-1", config);
        let request = ConductorActionRequest {
            action_type: ConductorActionType::SpawnChild,
            child_session_id: None,
            payload: json!({
                "title": "Worker",
                "prompt": "Do the child task."
            }),
        };
        let action_id = storage
            .enqueue_conductor_action("conductor-1", &request)
            .unwrap();

        super::process_queued_actions(&storage).unwrap();

        let row: (String, String) = storage
            .conn()
            .query_row(
                "SELECT status, result FROM conductor_actions WHERE id = ?1",
                [&action_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(row.0, ConductorActionStatus::Completed.as_str());
        assert!(row.1.starts_with("child spawned: "));
        let child_id = row.1.trim_start_matches("child spawned: ");
        let child = storage.get_session(child_id).unwrap().unwrap();
        assert_eq!(child.title, "Worker");
        assert_eq!(child.parent_session_id, "conductor-1");
    }

    #[test]
    fn process_queued_actions_does_not_process_already_claimed_action() {
        let (storage, _dir) = test_storage();
        let config = ConductorConfig::default_for_session("conductor-1".to_string());
        save_conductor(&storage, "conductor-1", config);
        let mut child = make_test_session("child-1");
        child.parent_session_id = "conductor-1".to_string();
        storage.save_session(&child).unwrap();
        let request = ConductorActionRequest {
            action_type: ConductorActionType::RecordChildSummary,
            child_session_id: Some("child-1".to_string()),
            payload: json!({"summary": "ready"}),
        };
        let action_id = storage
            .enqueue_conductor_action("conductor-1", &request)
            .unwrap();
        let claimed = storage.claim_queued_conductor_actions().unwrap();
        assert_eq!(claimed.len(), 1);

        super::process_queued_actions(&storage).unwrap();

        let status: String = storage
            .conn()
            .query_row(
                "SELECT status FROM conductor_actions WHERE id = ?1",
                [&action_id],
                |row| row.get(0),
            )
            .unwrap();
        let event_count: i64 = storage
            .conn()
            .query_row("SELECT COUNT(*) FROM conductor_events", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(status, ConductorActionStatus::Processing.as_str());
        assert_eq!(event_count, 0);
    }

    #[test]
    fn process_queued_actions_marks_malformed_claimed_action_failed() {
        let (storage, _dir) = test_storage();
        let config = ConductorConfig::default_for_session("conductor-1".to_string());
        save_conductor(&storage, "conductor-1", config);
        storage
            .conn()
            .execute(
                "INSERT INTO conductor_actions (
                    id, conductor_session_id, action_type, payload, status, created_at,
                    updated_at, result
                ) VALUES ('bad-action', 'conductor-1', 'record_child_summary',
                    '{not-json', 'queued', 1, 1, '')",
                [],
            )
            .unwrap();

        super::process_queued_actions(&storage).unwrap();

        let row: (String, String) = storage
            .conn()
            .query_row(
                "SELECT status, result FROM conductor_actions WHERE id = 'bad-action'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(row.0, ConductorActionStatus::Failed.as_str());
        assert!(row.1.contains("invalid conductor action payload"));
    }

    #[test]
    fn claim_due_conductor_heartbeat_advances_timestamp_before_send() {
        let (storage, _dir) = test_storage();
        let mut config = ConductorConfig::default_for_session("conductor-1".to_string());
        config.heartbeat_secs = 5;
        config.last_heartbeat_at = 5_000;
        save_conductor(&storage, "conductor-1", config);
        let now_ms = 10_000;
        assert_eq!(super::due_conductors(&storage, now_ms).unwrap().len(), 1);

        let claimed = storage
            .claim_due_conductor_heartbeat("conductor-1", now_ms)
            .unwrap();

        assert!(claimed);
        assert!(super::due_conductors(&storage, now_ms).unwrap().is_empty());
        let config = storage
            .get_conductor_config("conductor-1")
            .unwrap()
            .unwrap();
        assert_eq!(config.last_heartbeat_at, now_ms);
        let claimed_again = storage
            .claim_due_conductor_heartbeat("conductor-1", now_ms)
            .unwrap();
        assert!(!claimed_again);
    }

    #[test]
    fn process_queued_actions_blocks_disallowed_send_child_response() {
        let (storage, _dir) = test_storage();
        let mut config = ConductorConfig::default_for_session("conductor-1".to_string());
        config.allow_send_child_response = false;
        save_conductor(&storage, "conductor-1", config);
        let request = ConductorActionRequest {
            action_type: ConductorActionType::SendChildResponse,
            child_session_id: Some("child-1".to_string()),
            payload: json!({"message": "continue"}),
        };
        let action_id = storage
            .enqueue_conductor_action("conductor-1", &request)
            .unwrap();

        super::process_queued_actions(&storage).unwrap();

        let row: (String, String) = storage
            .conn()
            .query_row(
                "SELECT status, result FROM conductor_actions WHERE id = ?1",
                [&action_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(row.0, ConductorActionStatus::Blocked.as_str());
        assert!(row.1.contains("send_child_response is disabled"));
    }

    #[test]
    fn process_queued_actions_respects_max_actions_per_tick() {
        let (storage, _dir) = test_storage();
        let mut config = ConductorConfig::default_for_session("conductor-1".to_string());
        config.max_actions_per_tick = 1;
        save_conductor(&storage, "conductor-1", config);
        for child_id in ["child-1", "child-2"] {
            let mut child = make_test_session(child_id);
            child.parent_session_id = "conductor-1".to_string();
            storage.save_session(&child).unwrap();
            let request = ConductorActionRequest {
                action_type: ConductorActionType::RecordChildSummary,
                child_session_id: Some(child_id.to_string()),
                payload: json!({"summary": child_id}),
            };
            storage
                .enqueue_conductor_action("conductor-1", &request)
                .unwrap();
        }

        super::process_queued_actions(&storage).unwrap();

        let completed_count: i64 = storage
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM conductor_actions WHERE status = ?1",
                [ConductorActionStatus::Completed.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        let queued_count: i64 = storage
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM conductor_actions WHERE status = ?1",
                [ConductorActionStatus::Queued.as_str()],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(completed_count, 1);
        assert_eq!(queued_count, 1);
    }

    #[test]
    fn failed_heartbeat_send_remains_due_for_retry() {
        let (storage, _dir) = test_storage();
        let mut config = ConductorConfig::default_for_session("conductor-1".to_string());
        config.heartbeat_secs = 5;
        config.last_heartbeat_at = 5_000;
        let mut conductor = make_test_session("conductor-1");
        conductor.role = SessionRole::Conductor;
        conductor.tmux_session = "nonexistent_session_xyz".to_string();
        storage.save_session(&conductor).unwrap();
        storage.save_conductor_config(&config).unwrap();
        let now_ms = 10_000;

        let err = super::run_once(&storage, now_ms).unwrap_err();

        assert!(err.contains("failed to send conductor heartbeat"));
        assert_eq!(super::due_conductors(&storage, now_ms).unwrap().len(), 1);
    }
}
