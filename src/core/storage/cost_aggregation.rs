//! Time-windowed cost aggregation queries. All return DTOs from
//! `core::cost::aggregation` and never leak rusqlite types.

use crate::core::cost::{
    compute_credits, CostPeriod, CostSummary, ModelCost, RunnerCost, SessionCost,
};
use crate::core::storage::Storage;
use crate::types::Tool;
use chrono::{Datelike, Local, TimeZone};
use rusqlite::Result as SqlResult;

/// Lower bound (unix seconds) for `period`. `None` for `AllTime`.
fn period_floor_unix(period: CostPeriod) -> Option<i64> {
    let now = Local::now();
    match period {
        CostPeriod::Today => {
            let start = Local
                .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
                .single()?;
            Some(start.timestamp())
        }
        CostPeriod::Week => {
            // ISO Monday 00:00 local.
            let weekday = now.weekday().num_days_from_monday() as i64;
            let today_start = Local
                .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
                .single()?;
            Some(today_start.timestamp() - weekday * 86_400)
        }
        CostPeriod::Month => {
            let start = Local
                .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
                .single()?;
            Some(start.timestamp())
        }
        CostPeriod::AllTime => None,
    }
}

/// Convert `ts` (unix nanoseconds, as stored in `cost_events`) to seconds.
fn ns_floor_for(period: CostPeriod) -> i64 {
    period_floor_unix(period)
        .map(|s| s * 1_000_000_000)
        .unwrap_or(0)
}

impl Storage {
    pub fn cost_summary(&self, period: CostPeriod) -> SqlResult<CostSummary> {
        let floor_ns = ns_floor_for(period);
        let mut stmt = self.conn.prepare(
            "SELECT \
               COALESCE(SUM(cost_microdollars), 0), \
               COALESCE(SUM(input_tokens), 0), \
               COALESCE(SUM(output_tokens), 0), \
               COALESCE(SUM(cache_read_tokens), 0), \
               COALESCE(SUM(cache_creation_tokens), 0) \
             FROM cost_events WHERE ts >= ?1",
        )?;
        let row = stmt.query_row([floor_ns], |r| {
            Ok(CostSummary {
                total_microdollars: r.get(0)?,
                input_tokens: r.get(1)?,
                output_tokens: r.get(2)?,
                cache_read_tokens: r.get(3)?,
                cache_creation_tokens: r.get(4)?,
            })
        })?;
        Ok(row)
    }

    pub fn cost_by_runner(&self, period: CostPeriod) -> SqlResult<Vec<RunnerCost>> {
        let floor_ns = ns_floor_for(period);
        let mut stmt = self.conn.prepare(
            "SELECT \
               COALESCE(s.tool, \
                 CASE \
                   WHEN ce.model LIKE 'claude-%' THEN 'claude' \
                   WHEN ce.model LIKE 'gpt-%' THEN 'codex' \
                   WHEN ce.model LIKE 'gemini-%' THEN 'gemini' \
                   ELSE 'custom' \
                 END) AS tool, \
               ce.model, \
               SUM(ce.cost_microdollars) AS microdollars, \
               SUM(ce.input_tokens) AS input_tokens, \
               SUM(ce.output_tokens) AS output_tokens \
             FROM cost_events ce \
             LEFT JOIN sessions s ON s.id = ce.session_id \
             WHERE ce.ts >= ?1 \
             GROUP BY tool, ce.model",
        )?;
        let rows = stmt.query_map([floor_ns], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?;

        use std::collections::BTreeMap;
        let mut by_tool: BTreeMap<Tool, RunnerCost> = BTreeMap::new();
        for row in rows {
            let (tool_str, model, micro, input, output) = row?;
            let tool = Tool::from_str(&tool_str);
            let credits = compute_credits(&model, input, output);
            let entry = by_tool.entry(tool).or_insert_with(|| RunnerCost {
                tool,
                microdollars: 0,
                input_tokens: 0,
                output_tokens: 0,
                credits: None,
            });
            entry.microdollars += micro;
            entry.input_tokens += input;
            entry.output_tokens += output;
            if let Some(c) = credits {
                entry.credits = Some(entry.credits.unwrap_or(0) + c);
            }
        }

        let mut out: Vec<RunnerCost> = by_tool.into_values().collect();
        out.sort_by(|a, b| b.microdollars.cmp(&a.microdollars));
        Ok(out)
    }

    pub fn cost_by_model(&self, period: CostPeriod) -> SqlResult<Vec<ModelCost>> {
        let floor_ns = ns_floor_for(period);
        let mut stmt = self.conn.prepare(
            "SELECT model, \
                    SUM(cost_microdollars) AS micro, \
                    SUM(input_tokens) AS input, \
                    SUM(output_tokens) AS output \
             FROM cost_events WHERE ts >= ?1 \
             GROUP BY model ORDER BY micro DESC",
        )?;
        let rows = stmt
            .query_map([floor_ns], |r| {
                let model: String = r.get(0)?;
                let micro: i64 = r.get(1)?;
                let input: i64 = r.get(2)?;
                let output: i64 = r.get(3)?;
                Ok(ModelCost {
                    credits: compute_credits(&model, input, output),
                    model,
                    microdollars: micro,
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn top_sessions(&self, period: CostPeriod, limit: usize) -> SqlResult<Vec<SessionCost>> {
        let floor_ns = ns_floor_for(period);
        let mut stmt = self.conn.prepare(
            "SELECT ce.session_id, \
                    COALESCE(s.title, ce.session_id) AS label, \
                    COALESCE(s.tool, 'custom') AS tool, \
                    SUM(ce.cost_microdollars) AS micro, \
                    MAX(ce.ts) / 1000000000 AS last_ts_unix \
             FROM cost_events ce \
             LEFT JOIN sessions s ON s.id = ce.session_id \
             WHERE ce.ts >= ?1 \
             GROUP BY ce.session_id \
             ORDER BY micro DESC \
             LIMIT ?2",
        )?;
        let rows = stmt
            .query_map([floor_ns, limit as i64], |r| {
                Ok(SessionCost {
                    session_id: r.get(0)?,
                    session_label: r.get(1)?,
                    tool: Tool::from_str(&r.get::<_, String>(2)?),
                    microdollars: r.get(3)?,
                    last_event_ts_unix: r.get(4)?,
                })
            })?
            .collect::<SqlResult<Vec<_>>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::storage::{CostEvent, Storage};
    use tempfile::TempDir;

    fn fresh_storage() -> (TempDir, Storage) {
        let dir = TempDir::new().unwrap();
        let storage = Storage::open(dir.path().join("state.db")).unwrap();
        storage.migrate().unwrap();
        (dir, storage)
    }

    fn make_session(storage: &Storage, id: &str, tool: &str, name: &str) {
        storage
            .conn
            .execute(
                "INSERT INTO sessions (id, title, project_path, tool, status, created_at) \
                 VALUES (?1, ?2, '/tmp', ?3, 'idle', 0)",
                [id, name, tool],
            )
            .unwrap();
    }

    fn now_ns() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as i64
    }

    #[test]
    fn empty_db_returns_zero_summary() {
        let (_d, s) = fresh_storage();
        let sum = s.cost_summary(CostPeriod::AllTime).unwrap();
        assert_eq!(sum, CostSummary::default());
    }

    #[test]
    fn summary_aggregates_across_runners() {
        let (_d, s) = fresh_storage();
        make_session(&s, "claude-1", "claude", "claude work");
        make_session(&s, "codex-1", "codex", "codex work");
        let ts = now_ns();
        s.insert_cost_event(&CostEvent {
            session_id: "claude-1".into(),
            model: "claude-opus-4-7".into(),
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            ts,
            cost_microdollars: 30_000_000,
        })
        .unwrap();
        s.insert_cost_event(&CostEvent {
            session_id: "codex-1".into(),
            model: "gpt-5.5".into(),
            input_tokens: 200,
            output_tokens: 50,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            ts,
            cost_microdollars: 5_000_000,
        })
        .unwrap();

        let sum = s.cost_summary(CostPeriod::AllTime).unwrap();
        assert_eq!(sum.total_microdollars, 35_000_000);
        assert_eq!(sum.input_tokens, 1200);
        assert_eq!(sum.output_tokens, 550);
    }

    #[test]
    fn cost_by_runner_buckets_and_sums_credits_for_claude() {
        let (_d, s) = fresh_storage();
        make_session(&s, "c1", "claude", "claude work");
        make_session(&s, "x1", "codex", "codex work");
        let ts = now_ns();
        s.insert_cost_event(&CostEvent {
            session_id: "c1".into(),
            model: "claude-opus-4-7".into(),
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            ts,
            cost_microdollars: 1_000_000,
        })
        .unwrap();
        s.insert_cost_event(&CostEvent {
            session_id: "c1".into(),
            model: "claude-sonnet-4-6".into(),
            input_tokens: 1000,
            output_tokens: 250,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            ts,
            cost_microdollars: 2_000_000,
        })
        .unwrap();
        s.insert_cost_event(&CostEvent {
            session_id: "x1".into(),
            model: "gpt-5.5".into(),
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            ts,
            cost_microdollars: 500_000,
        })
        .unwrap();

        let by_runner = s.cost_by_runner(CostPeriod::AllTime).unwrap();
        let claude = by_runner.iter().find(|r| r.tool == Tool::Claude).unwrap();
        assert_eq!(claude.microdollars, 3_000_000);
        assert_eq!(claude.credits, Some(234 + 900));
        let codex = by_runner.iter().find(|r| r.tool == Tool::Codex).unwrap();
        assert_eq!(codex.microdollars, 500_000);
        assert_eq!(codex.credits, None);
    }

    #[test]
    fn top_sessions_sorts_by_cost_desc_with_limit() {
        let (_d, s) = fresh_storage();
        for (i, micro) in [
            ("a", 10_000_000),
            ("b", 50_000_000),
            ("c", 25_000_000),
        ] {
            make_session(&s, i, "claude", i);
            s.insert_cost_event(&CostEvent {
                session_id: i.into(),
                model: "claude-opus-4-7".into(),
                input_tokens: 100,
                output_tokens: 50,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
                ts: now_ns(),
                cost_microdollars: micro,
            })
            .unwrap();
        }
        let top = s.top_sessions(CostPeriod::AllTime, 2).unwrap();
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].session_id, "b");
        assert_eq!(top[1].session_id, "c");
    }

    #[test]
    fn period_floor_excludes_old_rows() {
        let (_d, s) = fresh_storage();
        make_session(&s, "old", "claude", "old work");
        let old_ts_s = chrono::Local::now().timestamp() - 30 * 86_400;
        s.insert_cost_event(&CostEvent {
            session_id: "old".into(),
            model: "claude-opus-4-7".into(),
            input_tokens: 1,
            output_tokens: 1,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            ts: old_ts_s * 1_000_000_000,
            cost_microdollars: 999_999,
        })
        .unwrap();
        let sum = s.cost_summary(CostPeriod::Today).unwrap();
        assert_eq!(sum.total_microdollars, 0);
        let sum_all = s.cost_summary(CostPeriod::AllTime).unwrap();
        assert_eq!(sum_all.total_microdollars, 999_999);
    }
}
