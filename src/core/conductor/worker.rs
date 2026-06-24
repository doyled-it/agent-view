use rusqlite::Result as SqlResult;
use std::thread;
use std::time::Duration;

use crate::core::conductor::actions::execute_action;
use crate::core::conductor::policy::{validate_action, PolicyDecision};
use crate::core::storage::{QueuedConductorAction, Storage};
use crate::types::{ConductorActionStatus, Session, SessionRole};

const WORKER_INTERVAL_SECS: u64 = 5;

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
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
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
    fn run_once_claims_due_heartbeat_without_sending_to_runner_pane() {
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

        super::run_once(&storage, now_ms).unwrap();

        assert!(super::due_conductors(&storage, now_ms).unwrap().is_empty());
        let config = storage
            .get_conductor_config("conductor-1")
            .unwrap()
            .unwrap();
        assert_eq!(config.last_heartbeat_at, now_ms);
    }
}
