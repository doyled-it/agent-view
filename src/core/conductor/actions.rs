use crate::core::storage::Storage;
use crate::types::{ConductorActionRequest, ConductorActionType, Session};

pub fn parse_action_request(input: &str) -> Result<ConductorActionRequest, String> {
    serde_json::from_str(input).map_err(|e| format!("invalid conductor action JSON: {}", e))
}

pub fn enqueue_action_from_json(
    storage: &Storage,
    conductor_session_id: &str,
    input: &str,
) -> Result<String, String> {
    let request = parse_action_request(input)?;
    storage
        .enqueue_conductor_action(conductor_session_id, &request)
        .map_err(|e| e.to_string())
}

#[allow(dead_code)]
pub fn execute_action(
    storage: &Storage,
    conductor_session_id: &str,
    request: &ConductorActionRequest,
) -> Result<String, String> {
    match request.action_type {
        ConductorActionType::RecordChildSummary => {
            let child = load_conductor_child(
                storage,
                conductor_session_id,
                request.child_session_id.as_deref(),
            )?;
            let summary = request
                .payload
                .get("summary")
                .and_then(|value| value.as_str())
                .ok_or_else(|| "payload.summary is required".to_string())?
                .trim()
                .to_string();
            storage
                .insert_conductor_event(
                    conductor_session_id,
                    Some(&child.id),
                    "summary",
                    &summary,
                    &request.payload,
                )
                .map_err(|e| e.to_string())?;
            Ok("summary recorded".to_string())
        }
        ConductorActionType::MarkChildNeedsUser => {
            let child = load_conductor_child(
                storage,
                conductor_session_id,
                request.child_session_id.as_deref(),
            )?;
            storage
                .set_user_waiting(&child.id, true)
                .map_err(|e| e.to_string())?;
            Ok("child marked for user".to_string())
        }
        ConductorActionType::SendChildResponse => {
            let child = load_conductor_child(
                storage,
                conductor_session_id,
                request.child_session_id.as_deref(),
            )?;
            let message = request
                .payload
                .get("message")
                .and_then(|value| value.as_str())
                .ok_or_else(|| "payload.message is required".to_string())?;
            if message.trim().is_empty() {
                return Err("payload.message is required".to_string());
            }
            crate::core::tmux::send_keys(&child.tmux_session, message)
                .map_err(|e| e.to_string())?;
            Ok("response sent".to_string())
        }
        _ => Ok("action accepted".to_string()),
    }
}

fn load_conductor_child(
    storage: &Storage,
    conductor_session_id: &str,
    child_session_id: Option<&str>,
) -> Result<Session, String> {
    let child_session_id =
        child_session_id.ok_or_else(|| "child_session_id is required".to_string())?;
    let child = storage
        .get_session(child_session_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("child session not found: {}", child_session_id))?;
    if child.parent_session_id != conductor_session_id {
        return Err("child session does not belong to conductor".to_string());
    }
    Ok(child)
}

#[cfg(test)]
mod tests {
    use crate::core::storage::test_helpers::{make_test_session, test_storage};
    use crate::types::{ConductorActionRequest, ConductorActionType, SessionRole};
    use serde_json::json;

    fn conductor_event_count(storage: &crate::core::storage::Storage) -> i64 {
        storage
            .conn()
            .query_row("SELECT COUNT(*) FROM conductor_events", [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    #[test]
    fn parses_action_request_json() {
        let request = super::parse_action_request(
            r#"{"action_type":"record_child_summary","child_session_id":"child1","payload":{"summary":"done"}}"#,
        )
        .unwrap();

        assert_eq!(request.action_type, ConductorActionType::RecordChildSummary);
        assert_eq!(request.child_session_id.as_deref(), Some("child1"));
        assert_eq!(request.payload["summary"], "done");
    }

    #[test]
    fn enqueue_action_from_json_stores_reconstructable_request() {
        let (storage, _dir) = test_storage();
        let mut conductor = make_test_session("conductor-1");
        conductor.role = SessionRole::Conductor;
        storage.save_session(&conductor).unwrap();

        let action_id = super::enqueue_action_from_json(
            &storage,
            "conductor-1",
            r#"{"action_type":"record_child_summary","child_session_id":"child1","payload":{"summary":"done"}}"#,
        )
        .unwrap();

        let stored_payload: String = storage
            .conn()
            .query_row(
                "SELECT payload FROM conductor_actions WHERE id = ?1",
                [&action_id],
                |row| row.get(0),
            )
            .unwrap();
        let stored_request: ConductorActionRequest = serde_json::from_str(&stored_payload).unwrap();

        assert_eq!(
            stored_request.action_type,
            ConductorActionType::RecordChildSummary
        );
        assert_eq!(stored_request.child_session_id.as_deref(), Some("child1"));
        assert_eq!(stored_request.payload["summary"], "done");
    }

    #[test]
    fn executor_records_child_summary_event() {
        let (storage, _dir) = test_storage();
        let mut conductor = make_test_session("conductor-1");
        conductor.role = SessionRole::Conductor;
        storage.save_session(&conductor).unwrap();
        let mut child = make_test_session("child1");
        child.parent_session_id = conductor.id.clone();
        storage.save_session(&child).unwrap();
        let request = ConductorActionRequest {
            action_type: ConductorActionType::RecordChildSummary,
            child_session_id: Some("child1".to_string()),
            payload: json!({"summary": " done "}),
        };

        let result = super::execute_action(&storage, "conductor-1", &request).unwrap();

        assert_eq!(result, "summary recorded");
        let event = storage
            .conn()
            .query_row(
                "SELECT child_session_id, event_type, message, payload
                 FROM conductor_events WHERE conductor_session_id = ?1",
                ["conductor-1"],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .unwrap();

        assert_eq!(event.0, "child1");
        assert_eq!(event.1, "summary");
        assert_eq!(event.2, "done");
        assert_eq!(event.3, r#"{"summary":" done "}"#);
    }

    #[test]
    fn record_child_summary_requires_child_session_id() {
        let (storage, _dir) = test_storage();
        let mut conductor = make_test_session("conductor-1");
        conductor.role = SessionRole::Conductor;
        storage.save_session(&conductor).unwrap();
        let request = ConductorActionRequest {
            action_type: ConductorActionType::RecordChildSummary,
            child_session_id: None,
            payload: json!({"summary": "done"}),
        };

        let err = super::execute_action(&storage, "conductor-1", &request).unwrap_err();

        assert!(err.contains("child_session_id is required"));
        assert_eq!(conductor_event_count(&storage), 0);
    }

    #[test]
    fn record_child_summary_rejects_unowned_child_without_event() {
        let (storage, _dir) = test_storage();
        let mut conductor = make_test_session("conductor-1");
        conductor.role = SessionRole::Conductor;
        storage.save_session(&conductor).unwrap();
        let mut child = make_test_session("child1");
        child.parent_session_id = "other-conductor".to_string();
        storage.save_session(&child).unwrap();
        let request = ConductorActionRequest {
            action_type: ConductorActionType::RecordChildSummary,
            child_session_id: Some("child1".to_string()),
            payload: json!({"summary": "done"}),
        };

        let err = super::execute_action(&storage, "conductor-1", &request).unwrap_err();

        assert!(err.contains("child session does not belong to conductor"));
        assert_eq!(conductor_event_count(&storage), 0);
    }

    #[test]
    fn executor_marks_child_needs_user() {
        let (storage, _dir) = test_storage();
        let mut conductor = make_test_session("conductor-1");
        conductor.role = SessionRole::Conductor;
        storage.save_session(&conductor).unwrap();
        let mut child = make_test_session("child1");
        child.parent_session_id = conductor.id.clone();
        storage.save_session(&child).unwrap();
        let request = ConductorActionRequest {
            action_type: ConductorActionType::MarkChildNeedsUser,
            child_session_id: Some("child1".to_string()),
            payload: json!({}),
        };

        let result = super::execute_action(&storage, "conductor-1", &request).unwrap();

        assert_eq!(result, "child marked for user");
        let loaded = storage.get_session("child1").unwrap().unwrap();
        assert!(loaded.user_waiting);
    }

    #[test]
    fn mark_child_needs_user_rejects_missing_child() {
        let (storage, _dir) = test_storage();
        let mut conductor = make_test_session("conductor-1");
        conductor.role = SessionRole::Conductor;
        storage.save_session(&conductor).unwrap();
        let request = ConductorActionRequest {
            action_type: ConductorActionType::MarkChildNeedsUser,
            child_session_id: Some("missing-child".to_string()),
            payload: json!({}),
        };

        let err = super::execute_action(&storage, "conductor-1", &request).unwrap_err();

        assert!(err.contains("child session not found"));
    }

    #[test]
    fn mark_child_needs_user_rejects_unowned_child_without_setting_waiting() {
        let (storage, _dir) = test_storage();
        let mut conductor = make_test_session("conductor-1");
        conductor.role = SessionRole::Conductor;
        storage.save_session(&conductor).unwrap();
        let mut child = make_test_session("child1");
        child.parent_session_id = "other-conductor".to_string();
        storage.save_session(&child).unwrap();
        let request = ConductorActionRequest {
            action_type: ConductorActionType::MarkChildNeedsUser,
            child_session_id: Some("child1".to_string()),
            payload: json!({}),
        };

        let err = super::execute_action(&storage, "conductor-1", &request).unwrap_err();

        assert!(err.contains("child session does not belong to conductor"));
        let loaded = storage.get_session("child1").unwrap().unwrap();
        assert!(!loaded.user_waiting);
    }

    #[test]
    fn send_child_response_requires_child_session_id() {
        let (storage, _dir) = test_storage();
        let request = ConductorActionRequest {
            action_type: ConductorActionType::SendChildResponse,
            child_session_id: None,
            payload: json!({"message": "continue"}),
        };

        let err = super::execute_action(&storage, "conductor-1", &request).unwrap_err();

        assert!(err.contains("child_session_id"));
    }

    #[test]
    fn send_child_response_requires_message() {
        let (storage, _dir) = test_storage();
        let mut child = make_test_session("child1");
        child.parent_session_id = "conductor-1".to_string();
        storage.save_session(&child).unwrap();
        let request = ConductorActionRequest {
            action_type: ConductorActionType::SendChildResponse,
            child_session_id: Some("child1".to_string()),
            payload: json!({}),
        };

        let err = super::execute_action(&storage, "conductor-1", &request).unwrap_err();

        assert!(err.contains("payload.message"));
    }

    #[test]
    fn send_child_response_rejects_unowned_child_before_tmux_send() {
        let (storage, _dir) = test_storage();
        let mut child = make_test_session("child1");
        child.parent_session_id = "other-conductor".to_string();
        child.tmux_session = "nonexistent_session_xyz".to_string();
        storage.save_session(&child).unwrap();
        let request = ConductorActionRequest {
            action_type: ConductorActionType::SendChildResponse,
            child_session_id: Some("child1".to_string()),
            payload: json!({"message": "continue"}),
        };

        let err = super::execute_action(&storage, "conductor-1", &request).unwrap_err();

        assert!(err.contains("child session does not belong to conductor"));
    }
}
