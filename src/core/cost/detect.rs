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

/// Resolve a Claude `Plan` for the current user, reading
/// `~/.claude.json` `oauthAccount.userRateLimitTier`. Returns `None`
/// when the file is missing, unreadable, or names an unknown tier.
///
/// The userRateLimitTier reflects per-user billing tier even on Team
/// accounts (where the org pays but each seat has individual limits),
/// so it's the right signal for credit-pacing math.
pub fn detect_claude_plan() -> Option<Plan> {
    detect_claude_plan_at(&dirs::home_dir()?.join(".claude.json"))
}

/// Testable variant — takes the path explicitly so fixtures don't need
/// to write to the real `~/.claude.json`.
pub fn detect_claude_plan_at(path: &Path) -> Option<Plan> {
    let bytes = std::fs::read(path).ok()?;
    let json: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let tier = json
        .get("oauthAccount")?
        .get("userRateLimitTier")?
        .as_str()?;
    tier_to_plan(tier)
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
        assert_eq!(detect_claude_plan_at(f.path()), Some(Plan::Max5x));
    }

    #[test]
    fn detects_pro_tier() {
        let f = write_fixture(r#"{"oauthAccount":{"userRateLimitTier":"default_claude_pro"}}"#);
        assert_eq!(detect_claude_plan_at(f.path()), Some(Plan::Pro));
    }

    #[test]
    fn detects_max_20x_tier() {
        let f = write_fixture(r#"{"oauthAccount":{"userRateLimitTier":"default_claude_max_20x"}}"#);
        assert_eq!(detect_claude_plan_at(f.path()), Some(Plan::Max20x));
    }

    #[test]
    fn unknown_tier_returns_none() {
        let f = write_fixture(r#"{"oauthAccount":{"userRateLimitTier":"experimental_foo"}}"#);
        assert_eq!(detect_claude_plan_at(f.path()), None);
    }

    #[test]
    fn missing_file_returns_none() {
        assert_eq!(
            detect_claude_plan_at(Path::new("/nonexistent/path/.claude.json")),
            None
        );
    }

    #[test]
    fn missing_oauth_account_returns_none() {
        let f = write_fixture(r#"{"otherKey":"value"}"#);
        assert_eq!(detect_claude_plan_at(f.path()), None);
    }

    #[test]
    fn missing_tier_field_returns_none() {
        let f = write_fixture(r#"{"oauthAccount":{"organizationName":"MITRE"}}"#);
        assert_eq!(detect_claude_plan_at(f.path()), None);
    }
}
