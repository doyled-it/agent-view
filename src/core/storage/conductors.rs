use rusqlite::{params, Result as SqlResult};

use super::Storage;
use crate::types::{ConductorConfig, ConductorMode};

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
