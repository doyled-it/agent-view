use rusqlite::params;
use rusqlite::Result as SqlResult;

use super::Storage;

impl Storage {
    pub fn save_routine_run(&self, run: &crate::types::RoutineRun) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO routine_runs (
                id, routine_id, started_at, finished_at, status,
                steps_completed, steps_total, log_path, tmux_session,
                tool_data, promoted_session_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                run.id,
                run.routine_id,
                run.started_at,
                run.finished_at,
                run.status.as_str(),
                run.steps_completed,
                run.steps_total,
                run.log_path,
                run.tmux_session,
                run.tool_data,
                run.promoted_session_id,
            ],
        )?;
        Ok(())
    }

    pub fn load_routine_runs(&self, routine_id: &str) -> SqlResult<Vec<crate::types::RoutineRun>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, routine_id, started_at, finished_at, status,
                    steps_completed, steps_total, log_path, tmux_session,
                    tool_data, promoted_session_id
             FROM routine_runs WHERE routine_id = ?1 ORDER BY started_at DESC",
        )?;
        let rows = stmt.query_map(params![routine_id], |row| {
            let status_str: String = row.get(4)?;
            Ok(crate::types::RoutineRun {
                id: row.get(0)?,
                routine_id: row.get(1)?,
                started_at: row.get(2)?,
                finished_at: row.get(3)?,
                status: crate::types::RunStatus::from_str(&status_str),
                steps_completed: row.get(5)?,
                steps_total: row.get(6)?,
                log_path: row.get(7)?,
                tmux_session: row.get(8)?,
                tool_data: row.get(9)?,
                promoted_session_id: row.get(10)?,
            })
        })?;
        rows.collect()
    }

    pub fn update_routine_run_status(
        &self,
        run_id: &str,
        status: crate::types::RunStatus,
        finished_at: Option<i64>,
    ) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE routine_runs SET status = ?1, finished_at = ?2 WHERE id = ?3",
            params![status.as_str(), finished_at, run_id],
        )?;
        Ok(())
    }

    pub fn increment_run_steps_completed(&self, run_id: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE routine_runs SET steps_completed = steps_completed + 1 WHERE id = ?1",
            params![run_id],
        )?;
        Ok(())
    }

    pub fn has_active_run(&self, routine_id: &str) -> SqlResult<bool> {
        let count: i32 = self.conn.query_row(
            "SELECT COUNT(*) FROM routine_runs WHERE routine_id = ?1 AND finished_at IS NULL",
            params![routine_id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    #[allow(dead_code)]
    pub fn get_latest_run(&self, routine_id: &str) -> SqlResult<Option<crate::types::RoutineRun>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, routine_id, started_at, finished_at, status,
                    steps_completed, steps_total, log_path, tmux_session,
                    tool_data, promoted_session_id
             FROM routine_runs WHERE routine_id = ?1 ORDER BY started_at DESC LIMIT 1",
        )?;
        let result = stmt.query_row(params![routine_id], |row| {
            let status_str: String = row.get(4)?;
            Ok(crate::types::RoutineRun {
                id: row.get(0)?,
                routine_id: row.get(1)?,
                started_at: row.get(2)?,
                finished_at: row.get(3)?,
                status: crate::types::RunStatus::from_str(&status_str),
                steps_completed: row.get(5)?,
                steps_total: row.get(6)?,
                log_path: row.get(7)?,
                tmux_session: row.get(8)?,
                tool_data: row.get(9)?,
                promoted_session_id: row.get(10)?,
            })
        });
        match result {
            Ok(run) => Ok(Some(run)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn delete_routine_run(&self, run_id: &str) -> SqlResult<()> {
        self.conn
            .execute("DELETE FROM routine_runs WHERE id = ?1", params![run_id])?;
        Ok(())
    }

    pub fn set_run_promoted(&self, run_id: &str, session_id: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE routine_runs SET promoted_session_id = ?1 WHERE id = ?2",
            params![session_id, run_id],
        )?;
        Ok(())
    }

    pub fn update_run_tool_data(&self, run_id: &str, tool_data: &str) -> SqlResult<()> {
        self.conn.execute(
            "UPDATE routine_runs SET tool_data = ?1 WHERE id = ?2",
            params![tool_data, run_id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::*;

    #[test]
    fn test_save_and_load_routine_run() {
        let (storage, _dir) = test_storage();
        let routine = make_test_routine("r1");
        storage.save_routine(&routine).unwrap();

        let run = crate::types::RoutineRun {
            id: "run1".to_string(),
            routine_id: "r1".to_string(),
            started_at: 1700000000000,
            finished_at: Some(1700000001000),
            status: crate::types::RunStatus::Completed,
            steps_completed: 1,
            steps_total: 1,
            log_path: Some("/tmp/log".to_string()),
            tmux_session: Some("agentorch_routine_test".to_string()),
            tool_data: "{}".to_string(),
            promoted_session_id: None,
        };
        storage.save_routine_run(&run).unwrap();

        let runs = storage.load_routine_runs("r1").unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].id, "run1");
        assert_eq!(runs[0].status, crate::types::RunStatus::Completed);
        assert_eq!(runs[0].log_path, Some("/tmp/log".to_string()));
    }

    #[test]
    fn test_update_routine_run_status() {
        let (storage, _dir) = test_storage();
        let routine = make_test_routine("r1");
        storage.save_routine(&routine).unwrap();

        let run = crate::types::RoutineRun {
            id: "run1".to_string(),
            routine_id: "r1".to_string(),
            started_at: 1700000000000,
            finished_at: None,
            status: crate::types::RunStatus::Running,
            steps_completed: 0,
            steps_total: 2,
            log_path: None,
            tmux_session: None,
            tool_data: "{}".to_string(),
            promoted_session_id: None,
        };
        storage.save_routine_run(&run).unwrap();

        storage
            .update_routine_run_status("run1", crate::types::RunStatus::Failed, Some(1700000002000))
            .unwrap();

        let runs = storage.load_routine_runs("r1").unwrap();
        assert_eq!(runs[0].status, crate::types::RunStatus::Failed);
        assert_eq!(runs[0].finished_at, Some(1700000002000));
    }

    #[test]
    fn test_has_active_run() {
        let (storage, _dir) = test_storage();
        let routine = make_test_routine("r1");
        storage.save_routine(&routine).unwrap();

        assert!(!storage.has_active_run("r1").unwrap());

        let run = crate::types::RoutineRun {
            id: "run1".to_string(),
            routine_id: "r1".to_string(),
            started_at: 1700000000000,
            finished_at: None,
            status: crate::types::RunStatus::Running,
            steps_completed: 0,
            steps_total: 1,
            log_path: None,
            tmux_session: None,
            tool_data: "{}".to_string(),
            promoted_session_id: None,
        };
        storage.save_routine_run(&run).unwrap();

        assert!(storage.has_active_run("r1").unwrap());
    }

    #[test]
    fn test_get_latest_run() {
        let (storage, _dir) = test_storage();
        let routine = make_test_routine("r1");
        storage.save_routine(&routine).unwrap();

        assert!(storage.get_latest_run("r1").unwrap().is_none());

        let run1 = crate::types::RoutineRun {
            id: "run1".to_string(),
            routine_id: "r1".to_string(),
            started_at: 1700000000000,
            finished_at: Some(1700000001000),
            status: crate::types::RunStatus::Completed,
            steps_completed: 1,
            steps_total: 1,
            log_path: None,
            tmux_session: None,
            tool_data: "{}".to_string(),
            promoted_session_id: None,
        };
        let run2 = crate::types::RoutineRun {
            id: "run2".to_string(),
            routine_id: "r1".to_string(),
            started_at: 1700000002000,
            finished_at: Some(1700000003000),
            status: crate::types::RunStatus::Failed,
            steps_completed: 0,
            steps_total: 1,
            log_path: None,
            tmux_session: None,
            tool_data: "{}".to_string(),
            promoted_session_id: None,
        };
        storage.save_routine_run(&run1).unwrap();
        storage.save_routine_run(&run2).unwrap();

        let latest = storage.get_latest_run("r1").unwrap().unwrap();
        assert_eq!(latest.id, "run2");
    }
}
