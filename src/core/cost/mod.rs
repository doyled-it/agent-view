//! Cost computation: pricing tables, USD/microdollar arithmetic.

pub mod pricing;
pub mod plan;
pub mod credits;

pub use pricing::{ModelRate, Pricer};
pub use plan::{Plan, PlanLimits};
pub use credits::{compute_credits, rate_for, ClaudeRate};
