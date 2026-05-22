//! Pure data types for cost aggregation. Storage methods that produce
//! these live in `core::storage::cost_aggregation` — keeping the types
//! here means UI code can name them without depending on Storage's
//! rusqlite types.

use crate::types::Tool;

/// Time window the user has selected in the Costs tab. Boundaries are
/// resolved against the current local clock at query time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostPeriod {
    Today,
    Week,
    Month,
    AllTime,
}

impl CostPeriod {
    pub const ALL: [CostPeriod; 4] = [
        CostPeriod::Today,
        CostPeriod::Week,
        CostPeriod::Month,
        CostPeriod::AllTime,
    ];

    /// Approximate length in days. Used for plan pro-rating in
    /// `Plan::saved_vs_api`. AllTime returns 0 because we suppress the
    /// Saved row for that period (no honest denominator).
    pub fn days(self) -> f64 {
        match self {
            CostPeriod::Today => 1.0,
            CostPeriod::Week => 7.0,
            CostPeriod::Month => 30.4,
            CostPeriod::AllTime => 0.0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CostPeriod::Today => "Today",
            CostPeriod::Week => "This week",
            CostPeriod::Month => "This month",
            CostPeriod::AllTime => "All time",
        }
    }

    /// Cycle next (◀ button reversed: ◀ goes backward).
    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|p| *p == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|p| *p == self).unwrap_or(0);
        Self::ALL[(idx + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// Header summary across all sessions in the period.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CostSummary {
    pub total_microdollars: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
}

/// One row of the Per-runner pane.
#[derive(Debug, Clone, PartialEq)]
pub struct RunnerCost {
    pub tool: Tool,
    pub microdollars: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    /// Sum of credits across all rows where credits could be computed
    /// (Anthropic models only). `None` when no Claude usage in the period.
    pub credits: Option<i64>,
}

/// One row of the Per-model pane.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelCost {
    pub model: String,
    pub microdollars: i64,
    pub credits: Option<i64>,
}

/// One row of the Top sessions pane.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionCost {
    pub session_id: String,
    pub session_label: String,
    pub tool: Tool,
    pub microdollars: i64,
    pub last_event_ts_unix: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn period_cycles_forward() {
        assert_eq!(CostPeriod::Today.next(), CostPeriod::Week);
        assert_eq!(CostPeriod::Week.next(), CostPeriod::Month);
        assert_eq!(CostPeriod::Month.next(), CostPeriod::AllTime);
        assert_eq!(CostPeriod::AllTime.next(), CostPeriod::Today);
    }

    #[test]
    fn period_cycles_backward() {
        assert_eq!(CostPeriod::Today.prev(), CostPeriod::AllTime);
        assert_eq!(CostPeriod::Week.prev(), CostPeriod::Today);
    }

    #[test]
    fn alltime_days_zero_suppresses_savings() {
        assert_eq!(CostPeriod::AllTime.days(), 0.0);
    }
}
