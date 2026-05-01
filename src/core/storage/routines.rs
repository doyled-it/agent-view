use rusqlite::params;
use rusqlite::Result as SqlResult;

use super::Storage;
use crate::types::Routine;

impl Storage {
    /// Save a routine (insert or update).
    /// Uses ON CONFLICT UPDATE instead of INSERT OR REPLACE to avoid
    /// triggering FK CASCADE deletes on routine_runs.
    pub fn save_routine(&self, routine: &Routine) -> SqlResult<()> {
        let steps_json = serde_json::to_string(&routine.steps).unwrap_or_else(|_| "[]".to_string());
        self.conn.execute(
            "INSERT INTO routines (
                id, name, group_path, sort_order, working_dir, default_tool,
                schedule, steps, enabled, created_at, last_run_at, next_run_at,
                run_count, pinned, notify, step_timeout_secs
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
            ON CONFLICT(id) DO UPDATE SET
                name=excluded.name, group_path=excluded.group_path,
                sort_order=excluded.sort_order, working_dir=excluded.working_dir,
                default_tool=excluded.default_tool, schedule=excluded.schedule,
                steps=excluded.steps, enabled=excluded.enabled,
                created_at=excluded.created_at, last_run_at=excluded.last_run_at,
                next_run_at=excluded.next_run_at, run_count=excluded.run_count,
                pinned=excluded.pinned, notify=excluded.notify,
                step_timeout_secs=excluded.step_timeout_secs",
            params![
                routine.id,
                routine.name,
                routine.group_path,
                routine.sort_order,
                routine.working_dir,
                routine.default_tool,
                routine.schedule,
                steps_json,
                routine.enabled as i32,
                routine.created_at,
                routine.last_run_at,
                routine.next_run_at,
                routine.run_count,
                routine.pinned as i32,
                routine.notify as i32,
                routine.step_timeout_secs,
            ],
        )?;
        Ok(())
    }

    pub fn load_routines(&self) -> SqlResult<Vec<Routine>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, group_path, sort_order, working_dir, default_tool,
                    schedule, steps, enabled, created_at, last_run_at, next_run_at,
                    run_count, pinned, notify, step_timeout_secs
             FROM routines ORDER BY sort_order",
        )?;
        let rows = stmt.query_map([], |row| {
            let steps_json: String = row.get(7)?;
            Ok(Routine {
                id: row.get(0)?,
                name: row.get(1)?,
                group_path: row.get(2)?,
                sort_order: row.get(3)?,
                working_dir: row.get(4)?,
                default_tool: row.get(5)?,
                schedule: row.get(6)?,
                steps: serde_json::from_str(&steps_json).unwrap_or_default(),
                enabled: row.get::<_, i32>(8)? == 1,
                created_at: row.get(9)?,
                last_run_at: row.get(10)?,
                next_run_at: row.get(11)?,
                run_count: row.get(12)?,
                pinned: row.get::<_, i32>(13)? == 1,
                notify: row.get::<_, i32>(14)? == 1,
                step_timeout_secs: row.get(15)?,
                expanded: false,
            })
        })?;
        rows.collect()
    }

    pub fn get_routine(&self, id: &str) -> SqlResult<Option<Routine>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, group_path, sort_order, working_dir, default_tool,
                    schedule, steps, enabled, created_at, last_run_at, next_run_at,
                    run_count, pinned, notify, step_timeout_secs
             FROM routines WHERE id = ?1",
        )?;
        let result = stmt.query_row(params![id], |row| {
            let steps_json: String = row.get(7)?;
            Ok(Routine {
                id: row.get(0)?,
                name: row.get(1)?,
                group_path: row.get(2)?,
                sort_order: row.get(3)?,
                working_dir: row.get(4)?,
                default_tool: row.get(5)?,
                schedule: row.get(6)?,
                steps: serde_json::from_str(&steps_json).unwrap_or_default(),
                enabled: row.get::<_, i32>(8)? == 1,
                created_at: row.get(9)?,
                last_run_at: row.get(10)?,
                next_run_at: row.get(11)?,
                run_count: row.get(12)?,
                pinned: row.get::<_, i32>(13)? == 1,
                notify: row.get::<_, i32>(14)? == 1,
                step_timeout_secs: row.get(15)?,
                expanded: false,
            })
        });
        match result {
            Ok(routine) => Ok(Some(routine)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn delete_routine(&self, id: &str) -> SqlResult<()> {
        self.conn
            .execute("DELETE FROM routines WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn set_routine_enabled(&self, id: &str, enabled: bool) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE routines SET enabled = ?1 WHERE id = ?2",
            params![enabled as i32, id],
        )?;
        Ok(())
    }

    pub fn set_routine_pinned(&self, id: &str, pinned: bool) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE routines SET pinned = ?1 WHERE id = ?2",
            params![pinned as i32, id],
        )?;
        Ok(())
    }

    pub fn rename_routine(&self, id: &str, new_name: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE routines SET name = ?1 WHERE id = ?2",
            params![new_name, id],
        )?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn move_routine_to_group(&self, id: &str, group_path: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE routines SET group_path = ?1 WHERE id = ?2",
            params![group_path, id],
        )?;
        Ok(())
    }

    pub fn record_routine_execution(
        &self,
        id: &str,
        last_run_at: i64,
        next_run_at: Option<i64>,
    ) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE routines SET last_run_at = ?1, next_run_at = ?2, run_count = run_count + 1 WHERE id = ?3",
            params![last_run_at, next_run_at, id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::*;
    use crate::types::{RoutineRun, RunStatus};

    #[test]
    fn test_save_and_load_routine() {
        let (storage, _dir) = test_storage();
        let routine = make_test_routine("r1");
        storage.save_routine(&routine).unwrap();

        let loaded = storage.load_routines().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "r1");
        assert_eq!(loaded[0].name, "Routine r1");
        assert_eq!(loaded[0].schedule, "0 9 * * *");
        assert_eq!(loaded[0].steps.len(), 1);
    }

    #[test]
    fn test_get_routine_by_id() {
        let (storage, _dir) = test_storage();
        let routine = make_test_routine("r1");
        storage.save_routine(&routine).unwrap();

        let found = storage.get_routine("r1").unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "Routine r1");

        let missing = storage.get_routine("nonexistent").unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn test_delete_routine() {
        let (storage, _dir) = test_storage();
        let routine = make_test_routine("r1");
        storage.save_routine(&routine).unwrap();
        storage.delete_routine("r1").unwrap();

        let loaded = storage.load_routines().unwrap();
        assert_eq!(loaded.len(), 0);
    }

    #[test]
    fn test_delete_routine_cascades_runs() {
        let (storage, _dir) = test_storage();
        let routine = make_test_routine("r1");
        storage.save_routine(&routine).unwrap();

        let run = RoutineRun {
            id: "run1".to_string(),
            routine_id: "r1".to_string(),
            started_at: 1700000000000,
            finished_at: Some(1700000001000),
            status: RunStatus::Completed,
            steps_completed: 1,
            steps_total: 1,
            log_path: None,
            tmux_session: None,
            tool_data: "{}".to_string(),
            promoted_session_id: None,
        };
        storage.save_routine_run(&run).unwrap();

        storage.delete_routine("r1").unwrap();
        let runs = storage.load_routine_runs("r1").unwrap();
        assert_eq!(runs.len(), 0);
    }

    #[test]
    fn test_routine_set_enabled() {
        let (storage, _dir) = test_storage();
        let routine = make_test_routine("r1");
        storage.save_routine(&routine).unwrap();

        storage.set_routine_enabled("r1", true).unwrap();
        let loaded = storage.get_routine("r1").unwrap().unwrap();
        assert!(loaded.enabled);

        storage.set_routine_enabled("r1", false).unwrap();
        let loaded = storage.get_routine("r1").unwrap().unwrap();
        assert!(!loaded.enabled);
    }

    #[test]
    fn test_routine_set_pinned() {
        let (storage, _dir) = test_storage();
        let routine = make_test_routine("r1");
        storage.save_routine(&routine).unwrap();

        storage.set_routine_pinned("r1", true).unwrap();
        let loaded = storage.get_routine("r1").unwrap().unwrap();
        assert!(loaded.pinned);
    }
}
