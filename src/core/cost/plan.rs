//! Claude subscription-plan tiers + savings math.
//!
//! Constants source: https://she-llac.com/claude-limits (2026-05).
//! Codex and Gemini plan tiers are out of scope for v1 — those runners
//! always resolve to `Plan::Api` regardless of config.

use serde::{Deserialize, Serialize};

/// Subscription tier for a single runner. `Api` means "no plan — bill at
/// API rates"; the dashboard's Saved row is suppressed for Api runners.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Plan {
    #[default]
    Api,
    Pro,
    #[serde(rename = "max-5x")]
    Max5x,
    #[serde(rename = "max-20x")]
    Max20x,
}

/// Flat-rate plan limits — `None` for `Plan::Api`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlanLimits {
    pub monthly_cost_usd: f64,
    pub monthly_credits: i64,
    pub weekly_credits: i64,
    /// 5-hour rolling session quota.
    pub session_credits: i64,
}

impl Plan {
    pub fn limits(self) -> Option<PlanLimits> {
        match self {
            Plan::Api => None,
            Plan::Pro => Some(PlanLimits {
                monthly_cost_usd: 20.0,
                monthly_credits: 21_700_000,
                weekly_credits: 5_000_000,
                session_credits: 550_000,
            }),
            Plan::Max5x => Some(PlanLimits {
                monthly_cost_usd: 100.0,
                monthly_credits: 180_600_000,
                weekly_credits: 41_670_000,
                session_credits: 3_300_000,
            }),
            Plan::Max20x => Some(PlanLimits {
                monthly_cost_usd: 200.0,
                monthly_credits: 361_100_000,
                weekly_credits: 83_330_000,
                session_credits: 11_000_000,
            }),
        }
    }

    /// Savings (microdollars) vs. paying at API rates over `period_days`.
    /// Returns `None` for `Plan::Api`. Returns 0 when API cost < plan cost
    /// (the plan isn't paying for itself yet this period).
    pub fn saved_vs_api(self, api_microdollars: i64, period_days: f64) -> Option<i64> {
        let limits = self.limits()?;
        let pro_rated_plan_micro =
            (limits.monthly_cost_usd * 1_000_000.0 * period_days / 30.4) as i64;
        Some((api_microdollars - pro_rated_plan_micro).max(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_has_no_limits() {
        assert!(Plan::Api.limits().is_none());
    }

    #[test]
    fn pro_limits_match_blog_post() {
        let l = Plan::Pro.limits().unwrap();
        assert_eq!(l.monthly_cost_usd, 20.0);
        assert_eq!(l.weekly_credits, 5_000_000);
        assert_eq!(l.session_credits, 550_000);
    }

    #[test]
    fn max20x_is_10x_pro_monthly() {
        let pro = Plan::Pro.limits().unwrap();
        let m20 = Plan::Max20x.limits().unwrap();
        assert_eq!(m20.monthly_cost_usd / pro.monthly_cost_usd, 10.0);
    }

    #[test]
    fn saved_returns_none_for_api() {
        assert_eq!(Plan::Api.saved_vs_api(100_000_000, 30.4), None);
    }

    #[test]
    fn saved_is_zero_when_api_cost_below_plan_cost() {
        // Pro monthly = $20 = 20_000_000 microdollars. $10 of API usage saves nothing.
        let saved = Plan::Pro.saved_vs_api(10_000_000, 30.4).unwrap();
        assert_eq!(saved, 0);
    }

    #[test]
    fn saved_pro_rates_period() {
        // 1 week of $80 API at Pro: plan-pro-rated = $20 * 7/30.4 ≈ $4.61.
        // saved ≈ $80 - $4.61 = $75.39 = 75_394_736 microdollars (±1 rounding).
        let saved = Plan::Pro.saved_vs_api(80_000_000, 7.0).unwrap();
        assert!(
            (75_300_000..=75_500_000).contains(&saved),
            "saved={}",
            saved
        );
    }

    #[test]
    fn plan_kebab_case_serde() {
        assert_eq!(
            serde_json::from_str::<Plan>(r#""max-5x""#).unwrap(),
            Plan::Max5x
        );
        assert_eq!(serde_json::from_str::<Plan>(r#""api""#).unwrap(), Plan::Api);
    }
}
