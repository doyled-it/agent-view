use rusqlite::{params, Result as SqlResult};

use super::Storage;
use crate::types::{ConductorActionRequest, ConductorActionStatus, ConductorConfig, ConductorMode};

impl Storage {
    pub fn save_conductor_config(&self, config: &ConductorConfig) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO conductor_configs (
                session_id, mode, heartbeat_secs, max_children, max_actions_per_tick,
                allow_spawn_child, allow_send_child_response, enabled, failure_count
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                config.session_id,
                config.mode.as_str(),
                config.heartbeat_secs,
                config.max_children,
                config.max_actions_per_tick,
                config.allow_spawn_child as i32,
                config.allow_send_child_response as i32,
                config.enabled as i32,
                config.failure_count,
            ],
        )?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn enqueue_conductor_action(
        &self,
        conductor_session_id: &str,
        action: &ConductorActionRequest,
    ) -> SqlResult<String> {
        let action_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp_millis();
        let payload = serde_json::to_string(action).unwrap_or_else(|_| "{}".to_string());

        self.conn.execute(
            "INSERT INTO conductor_actions (
                id, conductor_session_id, action_type, payload, status, created_at,
                updated_at, result
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, '')",
            params![
                action_id,
                conductor_session_id,
                action.action_type.as_str(),
                payload,
                ConductorActionStatus::Queued.as_str(),
                now,
            ],
        )?;

        Ok(action_id)
    }

    #[allow(dead_code)]
    pub fn update_conductor_action_status(
        &self,
        action_id: &str,
        status: ConductorActionStatus,
        result: &str,
    ) -> SqlResult<()> {
        let now = chrono::Utc::now().timestamp_millis();
        let affected = self.conn.execute(
            "UPDATE conductor_actions SET status = ?1, result = ?2, updated_at = ?3 WHERE id = ?4",
            params![status.as_str(), result, now, action_id],
        )?;
        if affected == 0 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn get_conductor_config(&self, session_id: &str) -> SqlResult<Option<ConductorConfig>> {
        let result = self.conn.query_row(
            "SELECT session_id, mode, heartbeat_secs, max_children, max_actions_per_tick,
                    allow_spawn_child, allow_send_child_response, enabled, failure_count
             FROM conductor_configs WHERE session_id = ?1",
            params![session_id],
            |row| {
                let mode: String = row.get(1)?;
                Ok(ConductorConfig {
                    session_id: row.get(0)?,
                    mode: ConductorMode::from_str(&mode),
                    heartbeat_secs: row.get(2)?,
                    max_children: row.get(3)?,
                    max_actions_per_tick: row.get(4)?,
                    allow_spawn_child: row.get::<_, i32>(5)? == 1,
                    allow_send_child_response: row.get::<_, i32>(6)? == 1,
                    enabled: row.get::<_, i32>(7)? == 1,
                    failure_count: row.get(8)?,
                })
            },
        );

        match result {
            Ok(config) => Ok(Some(config)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::*;
    use crate::types::{
        ConductorActionRequest, ConductorActionStatus, ConductorActionType, SessionRole,
    };
    use serde_json::json;

    #[test]
    fn enqueue_conductor_action_creates_queued_row() {
        let (storage, _dir) = test_storage();
        let mut conductor = make_test_session("conductor-1");
        conductor.role = SessionRole::Conductor;
        storage.save_session(&conductor).unwrap();
        let request = ConductorActionRequest {
            action_type: ConductorActionType::RecordChildSummary,
            child_session_id: Some("child-1".to_string()),
            payload: json!({"summary": "done"}),
        };

        let action_id = storage
            .enqueue_conductor_action("conductor-1", &request)
            .unwrap();

        let row = storage
            .conn()
            .query_row(
                "SELECT conductor_session_id, action_type, payload, status, result,
                        created_at, updated_at
                 FROM conductor_actions WHERE id = ?1",
                [&action_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, i64>(6)?,
                    ))
                },
            )
            .unwrap();

        assert_eq!(row.0, "conductor-1");
        assert_eq!(row.1, "record_child_summary");
        let stored_request: ConductorActionRequest = serde_json::from_str(&row.2).unwrap();
        assert_eq!(
            stored_request.action_type,
            ConductorActionType::RecordChildSummary
        );
        assert_eq!(stored_request.child_session_id.as_deref(), Some("child-1"));
        assert_eq!(stored_request.payload["summary"], "done");
        assert_eq!(row.3, "queued");
        assert_eq!(row.4, "");
        assert!(row.5 > 0);
        assert_eq!(row.5, row.6);
    }

    #[test]
    fn enqueue_conductor_action_stores_reconstructable_request_payload() {
        let (storage, _dir) = test_storage();
        let mut conductor = make_test_session("conductor-1");
        conductor.role = SessionRole::Conductor;
        storage.save_session(&conductor).unwrap();
        let request = ConductorActionRequest {
            action_type: ConductorActionType::RecordChildSummary,
            child_session_id: Some("child-1".to_string()),
            payload: json!({"summary": "done"}),
        };

        let action_id = storage
            .enqueue_conductor_action("conductor-1", &request)
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
        assert_eq!(stored_request.child_session_id.as_deref(), Some("child-1"));
        assert_eq!(stored_request.payload["summary"], "done");
    }

    #[test]
    fn update_conductor_action_status_writes_status_and_result() {
        let (storage, _dir) = test_storage();
        let mut conductor = make_test_session("conductor-1");
        conductor.role = SessionRole::Conductor;
        storage.save_session(&conductor).unwrap();
        let request = ConductorActionRequest {
            action_type: ConductorActionType::RecordChildSummary,
            child_session_id: None,
            payload: json!({}),
        };
        let action_id = storage
            .enqueue_conductor_action("conductor-1", &request)
            .unwrap();

        storage
            .update_conductor_action_status(
                &action_id,
                ConductorActionStatus::Completed,
                r#"{"ok":true}"#,
            )
            .unwrap();

        let row = storage
            .conn()
            .query_row(
                "SELECT status, result FROM conductor_actions WHERE id = ?1",
                [&action_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .unwrap();

        assert_eq!(row.0, "completed");
        assert_eq!(row.1, r#"{"ok":true}"#);
    }

    #[test]
    fn update_conductor_action_status_missing_action_returns_error() {
        let (storage, _dir) = test_storage();

        let err = storage
            .update_conductor_action_status("missing-action", ConductorActionStatus::Failed, "nope")
            .unwrap_err();

        assert!(matches!(err, rusqlite::Error::QueryReturnedNoRows));
    }

    #[test]
    fn update_conductor_action_status_writes_typed_status_strings() {
        let (storage, _dir) = test_storage();
        let mut conductor = make_test_session("conductor-1");
        conductor.role = SessionRole::Conductor;
        storage.save_session(&conductor).unwrap();

        for (status, expected) in [
            (ConductorActionStatus::Queued, "queued"),
            (ConductorActionStatus::Completed, "completed"),
            (ConductorActionStatus::Blocked, "blocked"),
            (ConductorActionStatus::Failed, "failed"),
        ] {
            let request = ConductorActionRequest {
                action_type: ConductorActionType::RecordChildSummary,
                child_session_id: None,
                payload: json!({}),
            };
            let action_id = storage
                .enqueue_conductor_action("conductor-1", &request)
                .unwrap();

            storage
                .update_conductor_action_status(&action_id, status, "typed")
                .unwrap();

            let stored_status: String = storage
                .conn()
                .query_row(
                    "SELECT status FROM conductor_actions WHERE id = ?1",
                    [&action_id],
                    |row| row.get(0),
                )
                .unwrap();

            assert_eq!(stored_status, expected);
        }
    }
}
