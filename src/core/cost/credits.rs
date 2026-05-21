//! Anthropic plan-credit formula. Converts (input, output) token counts to
//! Anthropic plan credits — see https://she-llac.com/claude-limits.
//!
//! Cache-reads are credit-free on Anthropic subscription plans (a major
//! reason why agentic workflows benefit from a plan). `cache_creation` is
//! undocumented in the blog post; v1 treats it as zero credits. If
//! evidence emerges that it bills, update the table and re-derive.
//!
//! Lookup is longest-prefix on the model string, mirroring `Pricer` so the
//! dated suffixes Anthropic ships (`claude-opus-4-7-20251015`) all hit the
//! bare family entry.

/// Credits per token for input vs. output on one Anthropic family.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClaudeRate {
    pub credits_per_input_token: f64,
    pub credits_per_output_token: f64,
}

/// Built-in rate table. Keys are longest-prefix matched.
fn rate_table() -> &'static [(&'static str, ClaudeRate)] {
    &[
        (
            "claude-opus",
            ClaudeRate {
                credits_per_input_token: 0.667,
                credits_per_output_token: 3.333,
            },
        ),
        (
            "claude-sonnet",
            ClaudeRate {
                credits_per_input_token: 0.4,
                credits_per_output_token: 2.0,
            },
        ),
        (
            "claude-haiku",
            ClaudeRate {
                credits_per_input_token: 0.133,
                credits_per_output_token: 0.667,
            },
        ),
    ]
}

/// Resolve a rate for `model`. Returns `None` for non-Anthropic models.
pub fn rate_for(model: &str) -> Option<ClaudeRate> {
    let mut best: Option<(&str, ClaudeRate)> = None;
    for (prefix, rate) in rate_table() {
        if model.starts_with(prefix) {
            match best {
                Some((p, _)) if p.len() >= prefix.len() => {}
                _ => best = Some((prefix, *rate)),
            }
        }
    }
    best.map(|(_, r)| r)
}

/// Credits consumed by one turn at API-rate inputs/outputs. Cache-reads
/// and cache-creation are credit-free. Returns `None` for non-Claude models.
pub fn compute_credits(model: &str, input_tokens: i64, output_tokens: i64) -> Option<i64> {
    let rate = rate_for(model)?;
    let i = input_tokens.max(0) as f64;
    let o = output_tokens.max(0) as f64;
    let raw = i * rate.credits_per_input_token + o * rate.credits_per_output_token;
    Some(raw.ceil() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opus_rate_matches_blog_post() {
        let r = rate_for("claude-opus-4-7").unwrap();
        assert!((r.credits_per_input_token - 0.667).abs() < 1e-9);
        assert!((r.credits_per_output_token - 3.333).abs() < 1e-9);
    }

    #[test]
    fn sonnet_rate_matches_blog_post() {
        let r = rate_for("claude-sonnet-4-6").unwrap();
        assert_eq!(r.credits_per_input_token, 0.4);
        assert_eq!(r.credits_per_output_token, 2.0);
    }

    #[test]
    fn dated_suffix_resolves_via_prefix() {
        // Anthropic ships `claude-opus-4-7-20251015`; our prefix table
        // contains `claude-opus`. Lookup must hit Opus.
        let r = rate_for("claude-opus-4-7-20251015").unwrap();
        assert!((r.credits_per_input_token - 0.667).abs() < 1e-9);
    }

    #[test]
    fn non_anthropic_returns_none() {
        assert!(rate_for("gpt-5.5").is_none());
        assert!(rate_for("gemini-2.5-pro").is_none());
    }

    #[test]
    fn opus_example_credits() {
        // 100 in, 50 out → ceil(100*0.667 + 50*3.333) = ceil(66.7 + 166.65)
        //                = ceil(233.35) = 234.
        assert_eq!(compute_credits("claude-opus-4-7", 100, 50), Some(234));
    }

    #[test]
    fn sonnet_example_credits() {
        // 1000 in, 250 out → 1000*0.4 + 250*2.0 = 400 + 500 = 900.
        assert_eq!(compute_credits("claude-sonnet-4-6", 1000, 250), Some(900));
    }

    #[test]
    fn negative_tokens_clamp_to_zero() {
        assert_eq!(compute_credits("claude-opus-4-7", -5, 10), Some(34)); // ceil(33.33)
    }

    #[test]
    fn compute_credits_none_for_non_anthropic() {
        assert_eq!(compute_credits("gpt-5.5", 100, 50), None);
    }
}
