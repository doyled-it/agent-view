//! Formatting helpers for timestamps, durations, and text truncation

pub(super) fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}

pub(super) fn format_timestamp(ms: i64) -> String {
    use chrono::{Local, TimeZone, Utc};
    let dt = Utc.timestamp_millis_opt(ms).single();
    match dt {
        Some(utc) => {
            let local = utc.with_timezone(&Local);
            local.format("%Y-%m-%d %H:%M").to_string()
        }
        None => "unknown".to_string(),
    }
}

pub(super) fn format_session_duration(created_at_ms: i64) -> String {
    let now = chrono::Utc::now().timestamp_millis();
    let diff_ms = now - created_at_ms;
    if diff_ms < 0 {
        return "just started".to_string();
    }

    let seconds = diff_ms / 1000;
    let minutes = seconds / 60;
    let hours = minutes / 60;
    let days = hours / 24;

    if days > 0 {
        format!("{}d {}h", days, hours % 24)
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes % 60)
    } else if minutes > 0 {
        format!("{}m", minutes)
    } else {
        "< 1m".to_string()
    }
}

pub(super) fn format_note_age(timestamp_ms: i64) -> String {
    let now = chrono::Utc::now().timestamp_millis();
    let diff_ms = now - timestamp_ms;
    if diff_ms < 0 {
        return "now".to_string();
    }

    let minutes = diff_ms / 60_000;
    let hours = minutes / 60;
    let days = hours / 24;

    if days > 0 {
        format!("{}d ago", days)
    } else if hours > 0 {
        format!("{}h ago", hours)
    } else if minutes > 0 {
        format!("{}m ago", minutes)
    } else {
        "now".to_string()
    }
}
