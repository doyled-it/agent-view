//! Cost / token events table — populated by the hook handler from Claude
//! Code Stop events (transcript JSONL last-line usage data).

use rusqlite::{params, Result as SqlResult};

use super::Storage;

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub struct CostEvent {
    pub session_id: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub ts: i64, // unix nanos
    /// Cost in microdollars (1 USD = 1_000_000 microdollars). Populated at
    /// ingest time by `event_watcher` via the `Pricer`; 0 when the model is
    /// unknown or the value was not supplied (e.g. older JSON files written
    /// before v9).
    pub cost_microdollars: i64,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CostTotals {
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_creation: i64,
    /// Sum of `cost_microdollars` across all events for this session.
    pub microdollars: i64,
}

#[allow(dead_code)]
impl Storage {
    pub fn insert_cost_event(&self, event: &CostEvent) -> SqlResult<()> {
        self.conn.execute(
            "INSERT INTO cost_events
                (session_id, model, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens, ts, cost_microdollars)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                event.session_id,
                event.model,
                event.input_tokens,
                event.output_tokens,
                event.cache_read_tokens,
                event.cache_creation_tokens,
                event.ts,
                event.cost_microdollars,
            ],
        )?;
        Ok(())
    }

    pub fn cost_totals_for_session(&self, session_id: &str) -> SqlResult<CostTotals> {
        let mut stmt = self.conn.prepare(
            "SELECT
                COALESCE(SUM(input_tokens), 0),
                COALESCE(SUM(output_tokens), 0),
                COALESCE(SUM(cache_read_tokens), 0),
                COALESCE(SUM(cache_creation_tokens), 0),
                COALESCE(SUM(cost_microdollars), 0)
             FROM cost_events WHERE session_id = ?1",
        )?;
        let totals = stmt.query_row(params![session_id], |row| {
            Ok(CostTotals {
                input: row.get(0)?,
                output: row.get(1)?,
                cache_read: row.get(2)?,
                cache_creation: row.get(3)?,
                microdollars: row.get(4)?,
            })
        })?;
        Ok(totals)
    }

    pub fn delete_cost_events_for_session(&self, session_id: &str) -> SqlResult<()> {
        self.conn.execute(
            "DELETE FROM cost_events WHERE session_id = ?1",
            params![session_id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::storage::test_helpers::{make_test_session, test_storage};

    fn ev(session_id: &str, in_t: i64, out_t: i64, ts: i64) -> CostEvent {
        CostEvent {
            session_id: session_id.to_string(),
            model: "claude-opus-4-7".to_string(),
            input_tokens: in_t,
            output_tokens: out_t,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            ts,
            cost_microdollars: 0,
        }
    }

    #[test]
    fn test_insert_and_aggregate() {
        let (storage, _dir) = test_storage();
        let session = make_test_session("s1");
        storage.save_session(&session).unwrap();
        storage.insert_cost_event(&ev("s1", 10, 100, 1)).unwrap();
        storage.insert_cost_event(&ev("s1", 20, 200, 2)).unwrap();
        storage.insert_cost_event(&ev("s1", 5, 50, 3)).unwrap();
        let totals = storage.cost_totals_for_session("s1").unwrap();
        assert_eq!(totals.input, 35);
        assert_eq!(totals.output, 350);
    }

    #[test]
    fn test_cascade_delete_with_session() {
        let (storage, _dir) = test_storage();
        let session = make_test_session("s1");
        storage.save_session(&session).unwrap();
        storage.insert_cost_event(&ev("s1", 1, 1, 1)).unwrap();
        storage.delete_session("s1").unwrap();
        let totals = storage.cost_totals_for_session("s1").unwrap();
        assert_eq!(totals, CostTotals::default());
    }

    #[test]
    fn test_delete_cost_events_for_session() {
        let (storage, _dir) = test_storage();
        let session = make_test_session("s1");
        storage.save_session(&session).unwrap();
        storage.insert_cost_event(&ev("s1", 1, 1, 1)).unwrap();
        storage.delete_cost_events_for_session("s1").unwrap();
        let totals = storage.cost_totals_for_session("s1").unwrap();
        assert_eq!(totals, CostTotals::default());
    }

    #[test]
    fn test_microdollars_persist_and_aggregate() {
        let (storage, _dir) = test_storage();
        let session = make_test_session("s1");
        storage.save_session(&session).unwrap();
        let mut e1 = ev("s1", 0, 0, 1);
        e1.cost_microdollars = 1_000_000;
        let mut e2 = ev("s1", 0, 0, 2);
        e2.cost_microdollars = 2_500_000;
        storage.insert_cost_event(&e1).unwrap();
        storage.insert_cost_event(&e2).unwrap();
        let totals = storage.cost_totals_for_session("s1").unwrap();
        assert_eq!(totals.microdollars, 3_500_000);
    }

    #[test]
    fn test_unknown_session_returns_zero_totals() {
        let (storage, _dir) = test_storage();
        let totals = storage.cost_totals_for_session("nope").unwrap();
        assert_eq!(totals, CostTotals::default());
    }
}
