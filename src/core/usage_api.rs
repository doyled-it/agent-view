//! Parses the response from https://api.anthropic.com/api/oauth/usage.
//!
//! The endpoint is undocumented; the shape is inferred. All fields are
//! optional so we degrade gracefully if Anthropic changes the format.

use crate::types::{UsageBucket, UsageData};
use chrono::TimeZone;

pub fn parse_response(json: &serde_json::Value) -> UsageData {
    UsageData {
        session: parse_bucket(json.get("five_hour"), "Current session"),
        week_all: parse_bucket(json.get("seven_day"), "Current week (all models)"),
        week_sonnet: parse_bucket(json.get("seven_day_opus"), "Current week (Sonnet only)"),
        last_updated: chrono::Utc::now().timestamp_millis(),
    }
}

fn parse_bucket(value: Option<&serde_json::Value>, label: &str) -> Option<UsageBucket> {
    let v = value?;
    let utilization = v.get("utilization").and_then(|x| x.as_f64()).unwrap_or(0.0);
    let percent = (utilization * 100.0).round().clamp(0.0, 100.0) as u8;

    let resets_at = v
        .get("resets_at")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    let resets = format_resets(resets_at);

    Some(UsageBucket {
        label: label.to_string(),
        percent,
        resets,
    })
}

/// Format an ISO-8601 timestamp into the legacy display string the UI expects:
/// "Apr 26 at 10am (America/Los_Angeles)" or "12pm (America/Los_Angeles)" if same day.
fn format_resets(iso: &str) -> String {
    let Ok(dt_utc) = chrono::DateTime::parse_from_rfc3339(iso) else {
        return iso.to_string();
    };
    let local = chrono::Local.from_utc_datetime(&dt_utc.naive_utc());
    let now = chrono::Local::now();

    let tz_name = iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".to_string());

    let same_day = local.date_naive() == now.date_naive();
    let time_part = local.format("%-l%P").to_string(); // e.g. "12pm"
    if same_day {
        format!("{} ({})", time_part, tz_name)
    } else {
        format!(
            "{} at {} ({})",
            local.format("%b %-d"),
            time_part,
            tz_name
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_full_response() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{
                "five_hour": { "utilization": 0.33, "resets_at": "2026-04-29T19:00:00Z" },
                "seven_day": { "utilization": 0.40, "resets_at": "2026-05-02T19:00:00Z" },
                "seven_day_opus": { "utilization": 0.07, "resets_at": "2026-05-02T01:00:00Z" }
            }"#,
        )
        .unwrap();
        let data = parse_response(&json);
        assert_eq!(data.session.as_ref().unwrap().percent, 33);
        assert_eq!(data.week_all.as_ref().unwrap().percent, 40);
        assert_eq!(data.week_sonnet.as_ref().unwrap().percent, 7);
    }

    #[test]
    fn test_parse_missing_buckets() {
        let json: serde_json::Value = serde_json::from_str(r#"{}"#).unwrap();
        let data = parse_response(&json);
        assert!(data.session.is_none());
        assert!(data.week_all.is_none());
        assert!(data.week_sonnet.is_none());
    }

    #[test]
    fn test_parse_zero_utilization() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{ "five_hour": { "utilization": 0.0, "resets_at": "2026-04-29T19:00:00Z" } }"#,
        )
        .unwrap();
        let data = parse_response(&json);
        assert_eq!(data.session.as_ref().unwrap().percent, 0);
    }

    #[test]
    fn test_parse_clamps_high_utilization() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{ "five_hour": { "utilization": 1.5, "resets_at": "2026-04-29T19:00:00Z" } }"#,
        )
        .unwrap();
        let data = parse_response(&json);
        assert_eq!(data.session.as_ref().unwrap().percent, 100);
    }

    #[test]
    fn test_parse_garbage_resets_passes_through() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{ "five_hour": { "utilization": 0.5, "resets_at": "not-a-date" } }"#,
        )
        .unwrap();
        let data = parse_response(&json);
        assert_eq!(data.session.as_ref().unwrap().resets, "not-a-date");
    }
}
