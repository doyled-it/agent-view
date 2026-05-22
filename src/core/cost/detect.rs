//! Best-effort plan-tier auto-detection from runner config files.
//!
//! Used by the Costs tab as a fallback when the user hasn't pinned a
//! value in `costs.plan` — beats the misleading "(API)" label and lets
//! the Summary pane do honest plan-cost / Saved math when we have a
//! reliable signal.
//!
//! Detection is conservative: unknown tier strings return `None` rather
//! than guessing. The renderer falls back to "API" in that case.

use crate::core::cost::Plan;
use std::path::Path;

/// Detected account state from `~/.claude.json`. Both fields are
/// independently optional so a partial detection (e.g. unknown tier but
/// known org type) still lets the UI add some context.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClaudeAccount {
    /// Effective per-user plan tier, mapped from `userRateLimitTier`.
    pub plan: Option<Plan>,
    /// Organization kind from `oauthAccount.organizationType`
    /// (e.g. `"claude_team"`, `"individual"`). Surfaced verbatim so
    /// new types Anthropic ships don't silently disappear.
    pub org_type: Option<String>,
}

impl ClaudeAccount {
    /// True when this account is a Team org (rather than an individual
    /// subscription). Used by the renderer to add a "Team" suffix.
    pub fn is_team(&self) -> bool {
        self.org_type.as_deref() == Some("claude_team")
    }
}

/// Resolve Claude account state for the current user, reading
/// `~/.claude.json`. Returns an empty `ClaudeAccount` when the file is
/// missing or unreadable.
pub fn detect_claude_account() -> ClaudeAccount {
    dirs::home_dir()
        .map(|h| detect_claude_account_at(&h.join(".claude.json")))
        .unwrap_or_default()
}

/// Testable variant — takes the path explicitly so fixtures don't need
/// to write to the real `~/.claude.json`.
pub fn detect_claude_account_at(path: &Path) -> ClaudeAccount {
    let Ok(bytes) = std::fs::read(path) else {
        return ClaudeAccount::default();
    };
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return ClaudeAccount::default();
    };
    let oauth = json.get("oauthAccount");
    let plan = oauth
        .and_then(|o| o.get("userRateLimitTier"))
        .and_then(|v| v.as_str())
        .and_then(tier_to_plan);
    let org_type = oauth
        .and_then(|o| o.get("organizationType"))
        .and_then(|v| v.as_str())
        .map(String::from);
    ClaudeAccount { plan, org_type }
}

/// Plan-only convenience wrapper. Kept so existing call sites that only
/// care about the credit-math tier don't have to thread a struct.
pub fn detect_claude_plan() -> Option<Plan> {
    detect_claude_account().plan
}

/// Map a `userRateLimitTier` string from Claude Code's oauth state to
/// our `Plan` enum. Returns `None` for unknown patterns.
fn tier_to_plan(tier: &str) -> Option<Plan> {
    match tier {
        "default_claude_pro" => Some(Plan::Pro),
        "default_claude_max_5x" => Some(Plan::Max5x),
        "default_claude_max_20x" => Some(Plan::Max20x),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_fixture(json: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(json.as_bytes()).unwrap();
        f
    }

    #[test]
    fn detects_max_5x_on_team_account() {
        // Real shape observed in the wild: Team-tier user with
        // userRateLimitTier set to default_claude_max_5x.
        let f = write_fixture(
            r#"{
                "oauthAccount": {
                    "organizationType": "claude_team",
                    "userRateLimitTier": "default_claude_max_5x",
                    "organizationName": "MITRE"
                }
            }"#,
        );
        let acct = detect_claude_account_at(f.path());
        assert_eq!(acct.plan, Some(Plan::Max5x));
        assert_eq!(acct.org_type.as_deref(), Some("claude_team"));
        assert!(acct.is_team());
    }

    #[test]
    fn detects_pro_tier_individual_account() {
        let f = write_fixture(
            r#"{"oauthAccount":{"organizationType":"individual","userRateLimitTier":"default_claude_pro"}}"#,
        );
        let acct = detect_claude_account_at(f.path());
        assert_eq!(acct.plan, Some(Plan::Pro));
        assert!(!acct.is_team());
    }

    #[test]
    fn detects_max_20x_tier() {
        let f = write_fixture(r#"{"oauthAccount":{"userRateLimitTier":"default_claude_max_20x"}}"#);
        assert_eq!(detect_claude_account_at(f.path()).plan, Some(Plan::Max20x));
    }

    #[test]
    fn unknown_tier_returns_none_plan_but_keeps_org() {
        let f = write_fixture(
            r#"{"oauthAccount":{"organizationType":"claude_team","userRateLimitTier":"experimental_foo"}}"#,
        );
        let acct = detect_claude_account_at(f.path());
        assert_eq!(acct.plan, None);
        assert!(acct.is_team());
    }

    #[test]
    fn missing_file_returns_default() {
        let acct = detect_claude_account_at(Path::new("/nonexistent/path/.claude.json"));
        assert_eq!(acct, ClaudeAccount::default());
    }

    #[test]
    fn missing_oauth_account_returns_default() {
        let f = write_fixture(r#"{"otherKey":"value"}"#);
        assert_eq!(detect_claude_account_at(f.path()), ClaudeAccount::default());
    }

    #[test]
    fn missing_tier_field_keeps_org_type() {
        let f = write_fixture(
            r#"{"oauthAccount":{"organizationType":"claude_team","organizationName":"MITRE"}}"#,
        );
        let acct = detect_claude_account_at(f.path());
        assert_eq!(acct.plan, None);
        assert!(acct.is_team());
    }
}
