//! Sparkline rendering helpers for the session list

use crate::types::StatusHistoryEntry;

/// Activity level for a status — higher = more active
fn status_activity_level(status: &str) -> u8 {
    match status {
        "running" => 5,
        "waiting" => 4,
        "error" => 3,
        "draft" | "paused" | "compacting" => 2,
        "idle" | "stopped" => 1,
        _ => 0,
    }
}

fn activity_level_to_char(level: u8) -> char {
    match level {
        5 => '\u{2586}', // ▆  running
        4 => '\u{2584}', // ▄  waiting
        3 => '\u{2585}', // ▅  error
        2 => '\u{2582}', // ▂  paused
        1 => '\u{2581}', // ▁  idle
        _ => ' ',        //    no data
    }
}

/// Render a time-bucketed sparkline over the last 24 hours.
/// Each character represents ~90 minutes. The dominant (most active)
/// status in each bucket determines the bar height.
pub(super) fn render_sparkline_str(history: &[StatusHistoryEntry], buckets: usize) -> String {
    render_sparkline_at(history, buckets, chrono::Utc::now().timestamp_millis())
}

/// Testable version that accepts a custom "now" timestamp.
fn render_sparkline_at(history: &[StatusHistoryEntry], buckets: usize, now_ms: i64) -> String {
    if history.is_empty() {
        return String::new();
    }

    let window_ms: i64 = 24 * 60 * 60 * 1000; // 24 hours
    let start_ms = now_ms - window_ms;
    let bucket_ms = window_ms / buckets as i64;

    // Check if any history falls within the window
    let has_recent = history.iter().any(|e| e.timestamp >= start_ms);
    if !has_recent {
        // All activity is older than 24h — show nothing
        return String::new();
    }

    // For each bucket, find the highest activity level.
    // A status entry represents the state AT that timestamp and persists
    // until the next entry. So we need to figure out what status was active
    // during each bucket.
    let mut result = String::with_capacity(buckets);

    for b in 0..buckets {
        let bucket_start = start_ms + b as i64 * bucket_ms;
        let bucket_end = bucket_start + bucket_ms;

        // Find the highest activity status that overlaps this bucket.
        // An entry at timestamp T is active from T until the next entry's timestamp.
        let mut max_level: u8 = 0;

        for (idx, entry) in history.iter().enumerate() {
            let entry_start = entry.timestamp;
            let entry_end = if idx + 1 < history.len() {
                history[idx + 1].timestamp
            } else {
                now_ms // last entry extends to now
            };

            // Does this entry overlap the bucket?
            if entry_start < bucket_end && entry_end > bucket_start {
                let level = status_activity_level(&entry.status);
                if level > max_level {
                    max_level = level;
                }
            }
        }

        result.push(activity_level_to_char(max_level));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sparkline_empty_history() {
        let spark = render_sparkline_str(&[], 4);
        assert_eq!(spark, "");
    }

    #[test]
    fn test_sparkline_all_old_returns_empty() {
        // History older than 24h — should return empty
        let now = chrono::Utc::now().timestamp_millis();
        let two_days_ago = now - 2 * 24 * 60 * 60 * 1000;
        let history = vec![StatusHistoryEntry {
            status: "running".to_string(),
            timestamp: two_days_ago,
        }];
        let spark = render_sparkline_at(&history, 16, now);
        assert_eq!(spark, "");
    }

    #[test]
    fn test_sparkline_bucket_count_matches() {
        let now = chrono::Utc::now().timestamp_millis();
        let history = vec![StatusHistoryEntry {
            status: "running".to_string(),
            timestamp: now - 60 * 60 * 1000, // 1 hour ago
        }];
        let spark = render_sparkline_at(&history, 16, now);
        assert_eq!(spark.chars().count(), 16);
    }

    #[test]
    fn test_sparkline_recent_running_shows_tall_bars() {
        let now = chrono::Utc::now().timestamp_millis();
        // Running for the entire last 24h
        let history = vec![StatusHistoryEntry {
            status: "running".to_string(),
            timestamp: now - 24 * 60 * 60 * 1000,
        }];
        let spark = render_sparkline_at(&history, 4, now);
        // All 4 buckets should be running = ▆
        assert_eq!(spark, "\u{2586}\u{2586}\u{2586}\u{2586}");
    }

    #[test]
    fn test_sparkline_mixed_statuses() {
        let now = chrono::Utc::now().timestamp_millis();
        let h24 = 24 * 60 * 60 * 1000;
        // First half idle, second half running (4 buckets)
        let history = vec![
            StatusHistoryEntry {
                status: "idle".to_string(),
                timestamp: now - h24,
            },
            StatusHistoryEntry {
                status: "running".to_string(),
                timestamp: now - h24 / 2,
            },
        ];
        let spark = render_sparkline_at(&history, 4, now);
        assert_eq!(spark.chars().count(), 4);
        // First 2 buckets idle (▁), last 2 running (▆)
        assert_eq!(spark, "\u{2581}\u{2581}\u{2586}\u{2586}");
    }
}
