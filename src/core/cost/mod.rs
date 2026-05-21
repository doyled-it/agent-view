//! Cost computation: pricing tables, USD/microdollar arithmetic.

pub mod aggregation;
pub mod credits;
pub mod plan;
pub mod pricing;

pub use aggregation::{CostPeriod, CostSummary, ModelCost, RunnerCost, SessionCost};
#[allow(unused_imports)]
pub use credits::{compute_credits, rate_for, ClaudeRate};
#[allow(unused_imports)]
pub use plan::{Plan, PlanLimits};
pub use pricing::{ModelRate, Pricer};

/// Format microdollars as `$NN.NN` (USD, two decimals). Single source of
/// truth so the cost panes can't drift on formatting.
pub fn render_usd(microdollars: i64) -> String {
    let dollars = microdollars as f64 / 1_000_000.0;
    format!("${:.2}", dollars)
}

#[cfg(test)]
mod render_usd_tests {
    use super::render_usd;

    #[test]
    fn formats_zero() {
        assert_eq!(render_usd(0), "$0.00");
    }

    #[test]
    fn formats_dollar() {
        assert_eq!(render_usd(1_000_000), "$1.00");
    }

    #[test]
    fn formats_cents() {
        assert_eq!(render_usd(1_234_567), "$1.23");
    }

    #[test]
    fn formats_large() {
        assert_eq!(render_usd(47_230_000), "$47.23");
    }
}
