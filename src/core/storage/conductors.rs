use rusqlite::{params, Result as SqlResult};
use std::collections::HashMap;

use super::Storage;
use crate::types::{
    ConductorActionRequest, ConductorActionStatus, ConductorConfig, ConductorMode, Session,
};

#[derive(Debug, Clone)]
pub struct QueuedConductorAction {
    pub id: String,
    pub conductor_session_id: String,
    pub payload: String,
}

impl QueuedConductorAction {
    pub fn to_request(&self) -> Result<ConductorActionRequest, String> {
        serde_json::from_str(&self.payload)
            .map_err(|e| format!("invalid conductor action payload for {}: {}", self.id, e))
    }
}

impl Storage {
    pub fn save_conductor_config(&self, config: &ConductorConfig) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO conductor_configs (
                session_id, mode, heartbeat_secs, last_heartbeat_at, max_children, max_actions_per_tick,
                allow_spawn_child, allow_send_child_response, enabled, failure_count
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                config.session_id,
                config.mode.as_str(),
                config.heartbeat_secs,
                config.last_heartbeat_at,
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
    pub fn insert_conductor_event(
        &self,
        conductor_session_id: &str,
        child_session_id: Option<&str>,
        event_type: &str,
        message: &str,
        payload: &serde_json::Value,
    ) -> SqlResult<String> {
        let event_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp_millis();
        let payload = serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string());

        self.conn.execute(
            "INSERT INTO conductor_events (
                id, conductor_session_id, child_session_id, event_type, message, payload,
                created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                event_id,
                conductor_session_id,
                child_session_id.unwrap_or(""),
                event_type,
                message,
                payload,
                now,
            ],
        )?;

        Ok(event_id)
    }

    #[allow(dead_code)]
    pub fn get_conductor_config(&self, session_id: &str) -> SqlResult<Option<ConductorConfig>> {
        let result = self.conn.query_row(
            "SELECT session_id, mode, heartbeat_secs, last_heartbeat_at, max_children, max_actions_per_tick,
                    allow_spawn_child, allow_send_child_response, enabled, failure_count
             FROM conductor_configs WHERE session_id = ?1",
            params![session_id],
            |row| {
                let mode: String = row.get(1)?;
                Ok(ConductorConfig {
                    session_id: row.get(0)?,
                    mode: ConductorMode::from_str(&mode),
                    heartbeat_secs: row.get(2)?,
                    last_heartbeat_at: row.get(3)?,
                    max_children: row.get(4)?,
                    max_actions_per_tick: row.get(5)?,
                    allow_spawn_child: row.get::<_, i32>(6)? == 1,
                    allow_send_child_response: row.get::<_, i32>(7)? == 1,
                    enabled: row.get::<_, i32>(8)? == 1,
                    failure_count: row.get(9)?,
                })
            },
        );

        match result {
            Ok(config) => Ok(Some(config)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    #[allow(dead_code)]
    pub fn load_queued_conductor_actions(&self) -> SqlResult<Vec<QueuedConductorAction>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, conductor_session_id, payload
             FROM conductor_actions
             WHERE status = ?1
             ORDER BY created_at, id",
        )?;
        let rows = stmt.query_map([ConductorActionStatus::Queued.as_str()], |row| {
            Ok(QueuedConductorAction {
                id: row.get(0)?,
                conductor_session_id: row.get(1)?,
                payload: row.get(2)?,
            })
        })?;
        rows.collect()
    }

    pub fn claim_queued_conductor_actions(&self) -> SqlResult<Vec<QueuedConductorAction>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, conductor_session_id
             FROM conductor_actions
             WHERE status = ?1
             ORDER BY created_at, id",
        )?;
        let actions = stmt
            .query_map([ConductorActionStatus::Queued.as_str()], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<SqlResult<Vec<(String, String)>>>()?;
        drop(stmt);

        let mut claimed = Vec::new();
        let mut claimed_by_conductor: HashMap<String, usize> = HashMap::new();
        for (id, conductor_session_id) in actions {
            if let Some(config) = self.get_conductor_config(&conductor_session_id)? {
                let limit = usize::try_from(config.max_actions_per_tick).unwrap_or(0);
                let claimed_count = claimed_by_conductor
                    .get(&conductor_session_id)
                    .copied()
                    .unwrap_or(0);
                if claimed_count >= limit {
                    continue;
                }
            }

            let now = chrono::Utc::now().timestamp_millis();
            let affected = self.conn.execute(
                "UPDATE conductor_actions
                 SET status = ?1, updated_at = ?2
                 WHERE id = ?3 AND status = ?4",
                params![
                    ConductorActionStatus::Processing.as_str(),
                    now,
                    id,
                    ConductorActionStatus::Queued.as_str(),
                ],
            )?;
            if affected == 1 {
                if let Some(action) = self.load_conductor_action(&id)? {
                    *claimed_by_conductor
                        .entry(conductor_session_id.clone())
                        .or_insert(0) += 1;
                    claimed.push(action);
                }
            }
        }

        Ok(claimed)
    }

    fn load_conductor_action(&self, action_id: &str) -> SqlResult<Option<QueuedConductorAction>> {
        let result = self.conn.query_row(
            "SELECT id, conductor_session_id, payload
             FROM conductor_actions
             WHERE id = ?1",
            params![action_id],
            |row| {
                Ok(QueuedConductorAction {
                    id: row.get(0)?,
                    conductor_session_id: row.get(1)?,
                    payload: row.get(2)?,
                })
            },
        );

        match result {
            Ok(action) => Ok(Some(action)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn count_child_sessions(&self, conductor_session_id: &str) -> SqlResult<usize> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE parent_session_id = ?1",
            params![conductor_session_id],
            |row| row.get(0),
        )?;
        Ok(usize::try_from(count).unwrap_or(0))
    }

    pub fn child_sessions(&self, conductor_session_id: &str) -> SqlResult<Vec<Session>> {
        let mut sessions = self.load_sessions()?;
        sessions.retain(|session| session.parent_session_id == conductor_session_id);
        Ok(sessions)
    }

    #[allow(dead_code)]
    pub fn update_conductor_last_heartbeat(&self, session_id: &str, now_ms: i64) -> SqlResult<()> {
        let affected = self.conn.execute(
            "UPDATE conductor_configs SET last_heartbeat_at = ?1 WHERE session_id = ?2",
            params![now_ms, session_id],
        )?;
        if affected == 0 {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        Ok(())
    }

    pub fn claim_due_conductor_heartbeat(&self, session_id: &str, now_ms: i64) -> SqlResult<bool> {
        let affected = self.conn.execute(
            "UPDATE conductor_configs
             SET last_heartbeat_at = ?1
             WHERE session_id = ?2
               AND enabled = 1
               AND last_heartbeat_at
                    + ((CASE WHEN heartbeat_secs > 0 THEN heartbeat_secs ELSE 0 END) * 1000)
                    <= ?1",
            params![now_ms, session_id],
        )?;
        Ok(affected == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::*;
    use crate::types::{
        ConductorActionRequest, ConductorActionStatus, ConductorActionType, ConductorConfig,
        SessionRole,
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
            (ConductorActionStatus::Processing, "processing"),
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

    #[test]
    fn claim_queued_conductor_actions_moves_rows_to_processing_once() {
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

        let claimed = storage.claim_queued_conductor_actions().unwrap();
        let claimed_again = storage.claim_queued_conductor_actions().unwrap();

        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].id, action_id);
        assert!(claimed_again.is_empty());

        let status: String = storage
            .conn()
            .query_row(
                "SELECT status FROM conductor_actions WHERE id = ?1",
                [&action_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, ConductorActionStatus::Processing.as_str());
    }

    #[test]
    fn insert_conductor_event_creates_event_row() {
        let (storage, _dir) = test_storage();
        let mut conductor = make_test_session("conductor-1");
        conductor.role = SessionRole::Conductor;
        storage.save_session(&conductor).unwrap();

        let event_id = storage
            .insert_conductor_event(
                "conductor-1",
                Some("child1"),
                "summary",
                "done",
                &json!({"summary": "done"}),
            )
            .unwrap();

        let row = storage
            .conn()
            .query_row(
                "SELECT conductor_session_id, child_session_id, event_type, message, payload,
                        created_at
                 FROM conductor_events WHERE id = ?1",
                [&event_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
            .unwrap();

        assert_eq!(row.0, "conductor-1");
        assert_eq!(row.1, "child1");
        assert_eq!(row.2, "summary");
        assert_eq!(row.3, "done");
        assert_eq!(row.4, r#"{"summary":"done"}"#);
        assert!(row.5 > 0);
    }

    #[test]
    fn save_and_load_conductor_config_round_trips_last_heartbeat_at() {
        let (storage, _dir) = test_storage();
        let mut conductor = make_test_session("conductor-1");
        conductor.role = SessionRole::Conductor;
        storage.save_session(&conductor).unwrap();
        let mut config = ConductorConfig::default_for_session("conductor-1".to_string());
        config.last_heartbeat_at = 123_456;

        storage.save_conductor_config(&config).unwrap();

        let loaded = storage.get_conductor_config("conductor-1").unwrap();
        assert_eq!(loaded, Some(config));
    }
}
