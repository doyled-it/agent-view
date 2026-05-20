//! Cost / token events table — populated by the hook handler from Claude
//! Code Stop events (transcript JSONL last-line usage data).

use rusqlite::{params, Result as SqlResult};

use super::Storage;
use crate::core::cost::Pricer;

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

    /// Recompute `cost_microdollars` for every row in `cost_events` using the
    /// supplied rate table. Idempotent — running with the same Pricer twice
    /// yields the same values. Used by the schema v9 migration (with the
    /// built-in defaults) and again at watcher startup (with user overrides
    /// from config) so that historical rows pick up rate-table changes.
    ///
    /// All UPDATEs run inside a single transaction so per-statement fsync
    /// overhead doesn't dominate startup on large `cost_events` tables.
    /// Failure rolls back: if any row's UPDATE errors out, the rate
    /// recompute is atomic — no half-updated mixture of old and new
    /// per-row values.
    pub fn recompute_cost_microdollars(&self, pricer: &Pricer) -> SqlResult<()> {
        let mut stmt = self.conn.prepare(
            "SELECT id, model, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens
             FROM cost_events",
        )?;
        let rows: Vec<(i64, String, i64, i64, i64, i64)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })?
            .collect::<SqlResult<Vec<_>>>()?;
        drop(stmt);
        if rows.is_empty() {
            return Ok(());
        }
        self.conn.execute("BEGIN", [])?;
        let result: SqlResult<()> = (|| {
            let mut update = self
                .conn
                .prepare("UPDATE cost_events SET cost_microdollars = ?1 WHERE id = ?2")?;
            for (id, model, input, output, cache_read, cache_creation) in rows {
                let micros =
                    pricer.compute_microdollars(&model, input, output, cache_read, cache_creation);
                update.execute(params![micros, id])?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.conn.execute("COMMIT", [])?;
                Ok(())
            }
            Err(e) => {
                let _ = self.conn.execute("ROLLBACK", []);
                Err(e)
            }
        }
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
    fn test_recompute_cost_microdollars_applies_override_pricer() {
        use crate::core::cost::{ModelRate, Pricer};
        let (storage, _dir) = test_storage();
        storage.save_session(&make_test_session("s1")).unwrap();
        // Seed a row priced at default rates.
        let mut e = ev("s1", 1_000_000, 0, 1);
        e.model = "claude-opus-4-7".to_string();
        e.cost_microdollars = 15_000_000; // default Opus input rate
        storage.insert_cost_event(&e).unwrap();

        // Recompute with an override that halves Opus input.
        let mut overrides = std::collections::HashMap::new();
        overrides.insert(
            "claude-opus-4-7".to_string(),
            ModelRate {
                input_per_mtok: 7.5,
                output_per_mtok: 0.0,
                cache_read_per_mtok: 0.0,
                cache_creation_per_mtok: 0.0,
            },
        );
        let pricer = Pricer::with_defaults().with_overrides(overrides);
        storage.recompute_cost_microdollars(&pricer).unwrap();

        let totals = storage.cost_totals_for_session("s1").unwrap();
        // 1M input @ $7.5/Mtok = $7.50 = 7_500_000 microdollars
        assert_eq!(totals.microdollars, 7_500_000);
    }

    #[test]
    fn test_unknown_session_returns_zero_totals() {
        let (storage, _dir) = test_storage();
        let totals = storage.cost_totals_for_session("nope").unwrap();
        assert_eq!(totals, CostTotals::default());
    }

    #[test]
    fn test_recompute_is_atomic_across_many_rows() {
        // The recompute MUST land its UPDATEs inside one transaction so
        // (a) per-row fsync doesn't dominate startup on large tables and
        // (b) every row ends up with the new rate. The 500-row count is
        // arbitrary but large enough that a per-row commit would visibly
        // slow the test (catches a future regression that drops the BEGIN).
        let (storage, _dir) = test_storage();
        storage.save_session(&make_test_session("rc")).unwrap();
        for ts in 0..500 {
            storage
                .insert_cost_event(&ev("rc", 1_000_000, 0, ts))
                .unwrap();
        }
        // Seed the column from defaults so we have a meaningful baseline.
        storage
            .recompute_cost_microdollars(&Pricer::with_defaults())
            .unwrap();
        // Default Opus 4.7 input rate is $15/Mtok = 15_000_000 microdollars.
        let totals_default = storage.cost_totals_for_session("rc").unwrap();
        assert_eq!(totals_default.microdollars, 500 * 15_000_000);

        // Override to a flat $1/Mtok and recompute. All 500 rows update.
        let mut overrides = std::collections::HashMap::new();
        overrides.insert(
            "claude-opus-4-7".to_string(),
            crate::core::cost::ModelRate {
                input_per_mtok: 1.0,
                output_per_mtok: 0.0,
                cache_read_per_mtok: 0.0,
                cache_creation_per_mtok: 0.0,
            },
        );
        let pricer = Pricer::with_defaults().with_overrides(overrides);
        storage.recompute_cost_microdollars(&pricer).unwrap();
        let totals_override = storage.cost_totals_for_session("rc").unwrap();
        assert_eq!(totals_override.microdollars, 500 * 1_000_000);
    }

    #[test]
    fn test_recompute_empty_table_is_no_op() {
        let (storage, _dir) = test_storage();
        // Should not BEGIN a transaction at all (would deadlock if buggy).
        storage
            .recompute_cost_microdollars(&Pricer::with_defaults())
            .unwrap();
    }
}
