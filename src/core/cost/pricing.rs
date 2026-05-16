//! Pricer: per-model rate table + microdollar cost computation.
//!
//! Defaults cover the Anthropic models we ship runners for today (Opus 4.7,
//! Sonnet 4.6, Haiku 4.5). User overrides layer on top via the `costs.pricing`
//! section of `~/.agent-view/config.json`.
//!
//! Model strings on disk include dated suffixes (e.g. `claude-opus-4-7-20251015`).
//! Lookup matches the longest registered prefix, so registering the bare name
//! catches every dated revision automatically.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// USD-per-Mtok rates for one model. Field names mirror the `cost_events`
/// storage columns (`input_tokens`, `output_tokens`, `cache_read_tokens`,
/// `cache_creation_tokens`) so the data path uses one vocabulary end to end.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ModelRate {
    #[serde(default)]
    pub input_per_mtok: f64,
    #[serde(default)]
    pub output_per_mtok: f64,
    #[serde(default)]
    pub cache_read_per_mtok: f64,
    #[serde(default)]
    pub cache_creation_per_mtok: f64,
}

impl ModelRate {
    /// Cost in microdollars for the supplied token counts at this rate.
    /// Negative inputs are clamped to zero (defensive — token columns are
    /// declared NOT NULL DEFAULT 0 so they should never be negative).
    ///
    /// Identity: `tokens * (USD per Mtok)` is already microdollars
    /// (1 USD/Mtok = 1 micro$/token), so we sum and round once without a
    /// dollar intermediate.
    pub fn microdollars(
        &self,
        input: i64,
        output: i64,
        cache_read: i64,
        cache_creation: i64,
    ) -> i64 {
        let i = input.max(0) as f64;
        let o = output.max(0) as f64;
        let cr = cache_read.max(0) as f64;
        let cc = cache_creation.max(0) as f64;
        (i * self.input_per_mtok
            + o * self.output_per_mtok
            + cr * self.cache_read_per_mtok
            + cc * self.cache_creation_per_mtok)
            .round() as i64
    }
}

/// Per-model rate table. Construct via [`Pricer::with_defaults`] and optionally
/// layer overrides via [`Pricer::with_overrides`].
#[derive(Debug, Clone, Default)]
pub struct Pricer {
    rates: HashMap<String, ModelRate>,
}

impl Pricer {
    /// New Pricer seeded with the built-in Anthropic rate table.
    pub fn with_defaults() -> Self {
        let mut rates = HashMap::new();
        rates.insert("claude-opus-4-7".to_string(), opus_4_7());
        rates.insert("claude-sonnet-4-6".to_string(), sonnet_4_6());
        rates.insert("claude-haiku-4-5".to_string(), haiku_4_5());
        Self { rates }
    }

    /// Layer per-model overrides on top of the current table. An override
    /// matching an existing key replaces it; a new key adds a model.
    pub fn with_overrides(mut self, overrides: HashMap<String, ModelRate>) -> Self {
        for (model, rate) in overrides {
            self.rates.insert(model, rate);
        }
        self
    }

    /// Look up the rate for a model name. Matches the longest registered
    /// prefix so dated suffixes (`claude-opus-4-7-20251015`) resolve to the
    /// bare entry (`claude-opus-4-7`).
    pub fn rate_for(&self, model: &str) -> Option<ModelRate> {
        let mut best: Option<(&str, ModelRate)> = None;
        for (key, rate) in &self.rates {
            if model.starts_with(key) {
                match best {
                    Some((existing, _)) if existing.len() >= key.len() => {}
                    _ => best = Some((key.as_str(), *rate)),
                }
            }
        }
        best.map(|(_, r)| r)
    }

    /// Microdollar cost for the supplied tokens. Returns 0 for unknown models
    /// (caller can detect via [`Pricer::rate_for`] when distinguishing free
    /// from unpriced is important).
    pub fn compute_microdollars(
        &self,
        model: &str,
        input: i64,
        output: i64,
        cache_read: i64,
        cache_creation: i64,
    ) -> i64 {
        match self.rate_for(model) {
            Some(rate) => rate.microdollars(input, output, cache_read, cache_creation),
            None => 0,
        }
    }
}

// --- Default rate table (Anthropic public list prices, USD/Mtok) ---
//
// These are list-price snapshots, NOT a live feed. Refresh when Anthropic
// changes pricing or when adding a new Claude model.
//
//   Snapshot date: 2026-05-15
//   Source: https://www.anthropic.com/pricing
//
// Users with active overrides via `costs.pricing` are insulated from
// staleness; default users get whatever was current at snapshot time.

fn opus_4_7() -> ModelRate {
    ModelRate {
        input_per_mtok: 15.0,
        output_per_mtok: 75.0,
        cache_read_per_mtok: 1.50,
        cache_creation_per_mtok: 18.75,
    }
}

fn sonnet_4_6() -> ModelRate {
    ModelRate {
        input_per_mtok: 3.0,
        output_per_mtok: 15.0,
        cache_read_per_mtok: 0.30,
        cache_creation_per_mtok: 3.75,
    }
}

fn haiku_4_5() -> ModelRate {
    ModelRate {
        input_per_mtok: 1.0,
        output_per_mtok: 5.0,
        cache_read_per_mtok: 0.10,
        cache_creation_per_mtok: 1.25,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn microdollars_simple_input_output() {
        let rate = ModelRate {
            input_per_mtok: 3.0,
            output_per_mtok: 15.0,
            cache_read_per_mtok: 0.0,
            cache_creation_per_mtok: 0.0,
        };
        // 1M input @ $3/Mtok = $3 = 3_000_000 microdollars
        // 100k output @ $15/Mtok = $1.50 = 1_500_000 microdollars
        assert_eq!(rate.microdollars(1_000_000, 100_000, 0, 0), 4_500_000);
    }

    #[test]
    fn microdollars_includes_cache_components() {
        let rate = ModelRate {
            input_per_mtok: 0.0,
            output_per_mtok: 0.0,
            cache_read_per_mtok: 0.30,
            cache_creation_per_mtok: 3.75,
        };
        // 1M cache_read @ $0.30 = $0.30 = 300_000 microdollars
        // 1M cache_creation @ $3.75 = $3.75 = 3_750_000 microdollars
        assert_eq!(rate.microdollars(0, 0, 1_000_000, 1_000_000), 4_050_000);
    }

    #[test]
    fn microdollars_clamps_negatives() {
        let rate = ModelRate {
            input_per_mtok: 10.0,
            output_per_mtok: 10.0,
            cache_read_per_mtok: 10.0,
            cache_creation_per_mtok: 10.0,
        };
        assert_eq!(rate.microdollars(-5, -5, -5, -5), 0);
    }

    #[test]
    fn microdollars_large_token_counts_stay_precise() {
        // 100M input tokens @ $15/Mtok = $1500 = 1_500_000_000 microdollars.
        // Guards against the previous formulation's divide-then-multiply
        // float roundtrip introducing rounding drift at large magnitudes.
        let rate = ModelRate {
            input_per_mtok: 15.0,
            output_per_mtok: 0.0,
            cache_read_per_mtok: 0.0,
            cache_creation_per_mtok: 0.0,
        };
        assert_eq!(rate.microdollars(100_000_000, 0, 0, 0), 1_500_000_000);
    }

    #[test]
    fn defaults_have_three_anthropic_models() {
        let p = Pricer::with_defaults();
        assert!(p.rate_for("claude-opus-4-7").is_some());
        assert!(p.rate_for("claude-sonnet-4-6").is_some());
        assert!(p.rate_for("claude-haiku-4-5").is_some());
    }

    #[test]
    fn rate_for_matches_dated_suffix() {
        let p = Pricer::with_defaults();
        let dated = p.rate_for("claude-opus-4-7-20251015").unwrap();
        let bare = p.rate_for("claude-opus-4-7").unwrap();
        assert_eq!(dated, bare);
    }

    #[test]
    fn rate_for_unknown_model_is_none() {
        let p = Pricer::with_defaults();
        assert!(p.rate_for("gpt-4o").is_none());
        assert!(p.rate_for("claude-not-a-thing").is_none());
    }

    #[test]
    fn rate_for_prefers_longest_prefix() {
        // Defensive: if both a bare and a more specific variant are
        // registered, the longer match wins. Guards against rate-table
        // expansion accidentally regressing dated lookups.
        let mut overrides = HashMap::new();
        overrides.insert(
            "claude-opus-4-7-special".to_string(),
            ModelRate {
                input_per_mtok: 999.0,
                output_per_mtok: 999.0,
                cache_read_per_mtok: 0.0,
                cache_creation_per_mtok: 0.0,
            },
        );
        let p = Pricer::with_defaults().with_overrides(overrides);
        let r = p.rate_for("claude-opus-4-7-special-20251015").unwrap();
        assert_eq!(r.input_per_mtok, 999.0);
        let r2 = p.rate_for("claude-opus-4-7-20251015").unwrap();
        assert_eq!(r2.input_per_mtok, 15.0);
    }

    #[test]
    fn compute_microdollars_unknown_model_returns_zero() {
        let p = Pricer::with_defaults();
        assert_eq!(
            p.compute_microdollars("unknown", 1_000_000, 1_000_000, 0, 0),
            0
        );
    }

    #[test]
    fn compute_microdollars_opus_47() {
        let p = Pricer::with_defaults();
        // 1M in @ $15, 1M out @ $75, 1M cache_read @ $1.50, 1M cache_creation @ $18.75
        // = $15 + $75 + $1.50 + $18.75 = $110.25 = 110_250_000 microdollars
        let got = p.compute_microdollars(
            "claude-opus-4-7",
            1_000_000,
            1_000_000,
            1_000_000,
            1_000_000,
        );
        assert_eq!(got, 110_250_000);
    }

    #[test]
    fn overrides_replace_existing_model() {
        let mut overrides = HashMap::new();
        overrides.insert(
            "claude-opus-4-7".to_string(),
            ModelRate {
                input_per_mtok: 1.0,
                output_per_mtok: 1.0,
                cache_read_per_mtok: 0.0,
                cache_creation_per_mtok: 0.0,
            },
        );
        let p = Pricer::with_defaults().with_overrides(overrides);
        // 1M in + 1M out @ $1 each = $2 = 2_000_000 microdollars
        assert_eq!(
            p.compute_microdollars("claude-opus-4-7", 1_000_000, 1_000_000, 0, 0),
            2_000_000
        );
    }

    #[test]
    fn overrides_can_add_new_model() {
        let mut overrides = HashMap::new();
        overrides.insert(
            "gpt-4o".to_string(),
            ModelRate {
                input_per_mtok: 2.50,
                output_per_mtok: 10.0,
                cache_read_per_mtok: 0.0,
                cache_creation_per_mtok: 0.0,
            },
        );
        let p = Pricer::with_defaults().with_overrides(overrides);
        // 1M in @ $2.50 = 2_500_000 microdollars
        assert_eq!(
            p.compute_microdollars("gpt-4o", 1_000_000, 0, 0, 0),
            2_500_000
        );
    }
}
