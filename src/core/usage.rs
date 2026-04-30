//! Parser for Claude Code /usage terminal output

use crate::types::{UsageBucket, UsageData};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const META_SESSION_NAME: &str = "__agentview_meta_usage";
pub const META_SESSION_PREFIX: &str = "__agentview_meta_";
const POLL_INTERVAL: Duration = Duration::from_secs(30);
const INIT_POLL_INTERVAL: Duration = Duration::from_millis(500);
const INIT_TIMEOUT: Duration = Duration::from_secs(30);

/// How many polls in a row with no bars (or identical data) before we consider the session stuck.
const STUCK_THRESHOLD: u8 = 3;
/// Max recoveries in any rolling 10-minute window before we give up recovering.
const MAX_RECOVERIES_PER_WINDOW: usize = 3;
const RECOVERY_WINDOW: Duration = Duration::from_secs(600); // 10 minutes

/// Shared usage data between the monitor thread and the main UI thread.
pub type SharedUsageData = Arc<Mutex<Option<UsageData>>>;

/// Spawn the usage monitor background thread.
/// Returns the shared data handle and the thread join handle.
pub fn spawn_monitor() -> (SharedUsageData, std::thread::JoinHandle<()>) {
    let shared: SharedUsageData = Arc::new(Mutex::new(None));
    let shared_clone = Arc::clone(&shared);

    let handle = std::thread::spawn(move || {
        monitor_loop(shared_clone);
    });

    (shared, handle)
}

/// Initialize (or re-initialize) the meta session: create it, wait for idle prompt,
/// accept trust prompt, send /usage, and wait for the quota bars to render.
/// Returns Ok(capture) with the rendered output on success, or Err with a reason.
fn init_meta_session() -> Result<String, &'static str> {
    if crate::core::tmux::session_exists(META_SESSION_NAME) {
        let _ = crate::core::tmux::kill_session(META_SESSION_NAME);
    }

    if crate::core::tmux::create_session(META_SESSION_NAME, Some("claude"), Some("/tmp"), None)
        .is_err()
    {
        return Err("claude not available");
    }

    // Pin the pane width so /usage output doesn't wrap. Without this the
    // detached session inherits tmux's default-size (~80 cols) and bar/Resets
    // lines wrap at "% used", breaking suffix-based percent extraction.
    let _ = crate::core::tmux::resize_window(META_SESSION_NAME, 200, 50);

    // Wait for Claude to reach idle prompt
    let start = Instant::now();
    let mut trust_accepted = false;
    loop {
        std::thread::sleep(INIT_POLL_INTERVAL);
        if start.elapsed() > INIT_TIMEOUT {
            let _ = crate::core::tmux::kill_session(META_SESSION_NAME);
            return Err("timed out waiting for idle prompt");
        }
        if let Ok(output) = crate::core::tmux::capture_pane_joined(META_SESSION_NAME, Some(-20)) {
            // Accept the workspace trust prompt if it appears
            if !trust_accepted && output.contains("Yes, I trust this folder") {
                let _ = crate::core::tmux::send_keys_raw(META_SESSION_NAME, "Enter");
                trust_accepted = true;
                continue;
            }
            let status = crate::core::status::parse_tool_status(&output, Some("claude"));
            if status.has_idle_prompt {
                break;
            }
        }
    }

    // Brief settle before sending command — Claude may not be fully ready
    std::thread::sleep(Duration::from_secs(1));

    // Send /usage — the command shows a persistent usage view
    if crate::core::tmux::send_keys(META_SESSION_NAME, "/usage").is_err() {
        let _ = crate::core::tmux::kill_session(META_SESSION_NAME);
        return Err("failed to send /usage");
    }

    // Wait for the quota bars to render
    let capture =
        wait_for_usage_render(META_SESSION_NAME).ok_or("render wait returned no capture")?;

    Ok(capture)
}

/// Wait for the /usage view to render the quota bars. Returns the final captured
/// output once all three buckets have parsed (header + Resets line each), or
/// the last capture after timeout.
fn wait_for_usage_render(session_name: &str) -> Option<String> {
    let mut last_capture: Option<String> = None;
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(250));
        if let Ok(output) = crate::core::tmux::capture_pane_joined(session_name, Some(-30)) {
            let data = parse_usage_output(&output);
            if data.session.is_some() && data.week_all.is_some() && data.week_sonnet.is_some() {
                return Some(output);
            }
            last_capture = Some(output);
        }
    }
    last_capture
}

/// Merge a freshly-parsed `UsageData` into the shared state, preserving any
/// previously-known bucket whose corresponding new field is `None`. Without
/// this the first transient poll where one bucket fails to parse (e.g. its
/// "Resets" line hadn't rendered yet) wipes the bar from the UI.
fn merge_into(shared: &SharedUsageData, fresh: &UsageData) {
    if fresh.session.is_none() && fresh.week_all.is_none() && fresh.week_sonnet.is_none() {
        return;
    }
    if let Ok(mut guard) = shared.lock() {
        let merged = match guard.take() {
            Some(prev) => UsageData {
                session: fresh.session.clone().or(prev.session),
                week_all: fresh.week_all.clone().or(prev.week_all),
                week_sonnet: fresh.week_sonnet.clone().or(prev.week_sonnet),
                last_updated: fresh.last_updated,
            },
            None => fresh.clone(),
        };
        *guard = Some(merged);
    }
}

/// Hash the percent values and reset strings of the parsed buckets to detect unchanged data.
fn hash_usage_data(data: &UsageData) -> u64 {
    let mut h = DefaultHasher::new();
    if let Some(b) = &data.session {
        b.percent.hash(&mut h);
        b.resets.hash(&mut h);
    }
    if let Some(b) = &data.week_all {
        b.percent.hash(&mut h);
        b.resets.hash(&mut h);
    }
    if let Some(b) = &data.week_sonnet {
        b.percent.hash(&mut h);
        b.resets.hash(&mut h);
    }
    h.finish()
}

/// Decide whether a poll result looks stuck.
///
/// Returns true (stuck) when:
/// - The capture contains "Loading usage data" but NOT "Current session", OR
/// - The new data hash equals the previous hash (data did not change).
fn is_stuck(capture: &str, new_hash: u64, prev_hash: Option<u64>) -> bool {
    let has_bars = capture.contains("Current session");
    let loading = capture.contains("Loading usage data");
    if loading && !has_bars {
        return true;
    }
    if let Some(ph) = prev_hash {
        if new_hash == ph {
            return true;
        }
    }
    false
}

fn monitor_loop(shared: SharedUsageData) {
    // Initial session setup
    let initial_capture = match init_meta_session() {
        Ok(c) => c,
        Err(_) => return, // claude not available — silently disable usage tracking
    };

    // Initial parse
    {
        let data = parse_usage_output(&initial_capture);
        merge_into(&shared, &data);
    }

    let mut prev_hash: Option<u64> = None;
    let mut unchanged_count: u8 = 0;
    let mut recovery_timestamps: Vec<Instant> = Vec::new();

    // Poll loop — close and reopen /usage to refresh data
    loop {
        std::thread::sleep(POLL_INTERVAL);

        if !crate::core::tmux::session_exists(META_SESSION_NAME) {
            if let Ok(mut guard) = shared.lock() {
                *guard = None;
            }
            return;
        }

        // Escape closes the /usage view, returning to idle prompt
        let _ = crate::core::tmux::send_keys_raw(META_SESSION_NAME, "Escape");
        std::thread::sleep(Duration::from_secs(1));
        // Drop accumulated scrollback so parse_bucket's rposition only sees
        // the upcoming render — bounds the cost of repeated polls.
        let _ = crate::core::tmux::clear_history(META_SESSION_NAME);
        // Re-send /usage to get fresh data
        let _ = crate::core::tmux::send_keys(META_SESSION_NAME, "/usage");

        // Wait for bars to render
        let capture = match wait_for_usage_render(META_SESSION_NAME) {
            Some(c) => c,
            None => continue,
        };

        let data = parse_usage_output(&capture);
        let new_hash = hash_usage_data(&data);

        let stuck = is_stuck(&capture, new_hash, prev_hash);

        if stuck {
            unchanged_count = unchanged_count.saturating_add(1);
        } else {
            unchanged_count = 0;
            prev_hash = Some(new_hash);
            merge_into(&shared, &data);
        }

        // Auto-recover if stuck for too many consecutive polls
        if unchanged_count >= STUCK_THRESHOLD {
            // Prune recovery timestamps older than the window
            let now = Instant::now();
            recovery_timestamps.retain(|t| now.duration_since(*t) < RECOVERY_WINDOW);

            if recovery_timestamps.len() < MAX_RECOVERIES_PER_WINDOW {
                recovery_timestamps.push(now);

                match init_meta_session() {
                    Ok(fresh_capture) => {
                        unchanged_count = 0;
                        let fresh_data = parse_usage_output(&fresh_capture);
                        let fresh_hash = hash_usage_data(&fresh_data);
                        prev_hash = Some(fresh_hash);
                        merge_into(&shared, &fresh_data);
                    }
                    Err(reason) => {
                        crate::core::logger::log_diagnostic(&format!(
                            "usage monitor: recovery failed — {reason}; continuing to poll"
                        ));
                        // Don't return; keep the loop alive
                    }
                }
            } else {
                crate::core::logger::log_diagnostic(&format!(
                    "usage monitor: recovery cap reached ({MAX_RECOVERIES_PER_WINDOW} in 10 min); skipping auto-recover, continuing to poll"
                ));
                // Reset unchanged_count so we don't spam on every subsequent poll
                unchanged_count = 0;
            }
        }
    }
}

/// Kill the usage monitor tmux session (call on app shutdown).
pub fn kill_monitor() {
    if crate::core::tmux::session_exists(META_SESSION_NAME) {
        let _ = crate::core::tmux::kill_session(META_SESSION_NAME);
    }
}

pub fn parse_usage_output(output: &str) -> UsageData {
    let lines: Vec<&str> = output.lines().collect();

    UsageData {
        session: parse_bucket(&lines, "Current session"),
        week_all: parse_bucket(&lines, "Current week (all models)"),
        week_sonnet: parse_bucket(&lines, "Current week (Sonnet only)"),
        last_updated: chrono::Utc::now().timestamp_millis(),
    }
}

fn parse_bucket(lines: &[&str], label: &str) -> Option<UsageBucket> {
    // /usage output accumulates in scrollback (each poll re-renders inline),
    // so multiple matching headers may be present. Use the most recent one —
    // older renders may have been pushed apart by intervening output and no
    // longer have their Resets line within the scan window.
    let label_idx = lines.iter().rposition(|l| l.trim().starts_with(label))?;

    // Scan forward for "Resets ..." and a "X% used" value. Formats seen:
    //   Old: bar line "████ 33% used" followed by "Resets ..."
    //   New: "Resets ... N% used" on one line, or "Resets ..." alone (percent omitted for near-zero)
    // Stop when we hit the next bucket header so values don't bleed across buckets.
    let mut percent: Option<u8> = None;
    let mut resets: Option<String> = None;

    // Scan up to 8 lines or until the next bucket header — claude occasionally
    // renders a transient "Loading usage data…" or extra blank between the
    // header and the Resets line, so a tighter window misses them.
    for line in lines.iter().skip(label_idx + 1).take(8) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("Current ") {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("Resets ") {
            let (resets_part, inline_pct) = split_trailing_percent(rest);
            if resets.is_none() {
                resets = Some(resets_part.trim_end().to_string());
            }
            if percent.is_none() {
                percent = inline_pct;
            }
            continue;
        }
        // Legacy bar line: "████ 33% used"
        if percent.is_none() {
            if let Some(cap) = trimmed.strip_suffix("% used") {
                if let Some(num_str) = cap.split_whitespace().last() {
                    percent = num_str.parse().ok();
                }
            }
        }
    }

    Some(UsageBucket {
        label: label.to_string(),
        percent: percent.unwrap_or(0),
        resets: resets?,
    })
}

/// Split a string like "Apr 26 at 10am (America/Los_Angeles)        1% used"
/// into ("Apr 26 at 10am (America/Los_Angeles)", Some(1)). Returns the full
/// input and None if no trailing "N% used" is present.
fn split_trailing_percent(s: &str) -> (&str, Option<u8>) {
    let Some(pct_idx) = s.rfind("% used") else {
        return (s, None);
    };
    let before = s[..pct_idx].trim_end();
    let Some(num_start) = before.rfind(|c: char| c.is_whitespace()) else {
        return (s, None);
    };
    let num_str = before[num_start..].trim();
    match num_str.parse::<u8>() {
        Ok(p) => (before[..num_start].trim_end(), Some(p)),
        Err(_) => (s, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_OUTPUT: &str = r#"
   Status   Config   Usage   Stats

  Current session
  ████████████████▌                                  33% used
  Resets 12pm (America/Los_Angeles)

  Current week (all models)
  ████████████████████                               40% used
  Resets Apr 23 at 12pm (America/Los_Angeles)

  Current week (Sonnet only)
  ███▌                                               7% used
  Resets Apr 23 at 6pm (America/Los_Angeles)

  Esc to cancel
"#;

    #[test]
    fn test_parse_session_bucket() {
        let data = parse_usage_output(SAMPLE_OUTPUT);
        let session = data.session.unwrap();
        assert_eq!(session.label, "Current session");
        assert_eq!(session.percent, 33);
        assert_eq!(session.resets, "12pm (America/Los_Angeles)");
    }

    #[test]
    fn test_parse_week_all_bucket() {
        let data = parse_usage_output(SAMPLE_OUTPUT);
        let week = data.week_all.unwrap();
        assert_eq!(week.label, "Current week (all models)");
        assert_eq!(week.percent, 40);
        assert_eq!(week.resets, "Apr 23 at 12pm (America/Los_Angeles)");
    }

    #[test]
    fn test_parse_week_sonnet_bucket() {
        let data = parse_usage_output(SAMPLE_OUTPUT);
        let sonnet = data.week_sonnet.unwrap();
        assert_eq!(sonnet.label, "Current week (Sonnet only)");
        assert_eq!(sonnet.percent, 7);
        assert_eq!(sonnet.resets, "Apr 23 at 6pm (America/Los_Angeles)");
    }

    #[test]
    fn test_parse_empty_output() {
        let data = parse_usage_output("");
        assert!(data.session.is_none());
        assert!(data.week_all.is_none());
        assert!(data.week_sonnet.is_none());
    }

    #[test]
    fn test_parse_garbage_output() {
        let data = parse_usage_output("some random text\nno usage data here");
        assert!(data.session.is_none());
        assert!(data.week_all.is_none());
        assert!(data.week_sonnet.is_none());
    }

    // The /usage command format introduced in Claude Code 2.1.x: no bar line,
    // and percent (when present) is appended to the "Resets ..." line.
    const NEW_FORMAT_OUTPUT: &str = r#"
   Status   Config   Usage   Stats

  Current session
  Resets 3pm (America/Los_Angeles)

  Current week (all models)
  Resets Apr 26 at 10am (America/Los_Angeles)

  Current week (Sonnet only)
  Resets Apr 26 at 10am (America/Los_Angeles)        1% used

  Esc to cancel
"#;

    #[test]
    fn test_parse_new_format_session_no_percent() {
        let data = parse_usage_output(NEW_FORMAT_OUTPUT);
        let session = data.session.expect("session bucket should be present");
        assert_eq!(session.percent, 0);
        assert_eq!(session.resets, "3pm (America/Los_Angeles)");
    }

    #[test]
    fn test_parse_new_format_week_no_bleed() {
        // Week (all) has no trailing percent; it must not pick up the "1% used"
        // from the Sonnet line below.
        let data = parse_usage_output(NEW_FORMAT_OUTPUT);
        let week = data.week_all.expect("week_all bucket should be present");
        assert_eq!(week.percent, 0);
        assert_eq!(week.resets, "Apr 26 at 10am (America/Los_Angeles)");
    }

    #[test]
    fn test_parse_new_format_sonnet_inline_percent() {
        let data = parse_usage_output(NEW_FORMAT_OUTPUT);
        let sonnet = data
            .week_sonnet
            .expect("week_sonnet bucket should be present");
        assert_eq!(sonnet.percent, 1);
        // Resets string must not contain the trailing "1% used"
        assert_eq!(sonnet.resets, "Apr 26 at 10am (America/Los_Angeles)");
    }

    #[test]
    fn test_split_trailing_percent() {
        let (r, p) = split_trailing_percent("Apr 26 at 10am (America/Los_Angeles)        1% used");
        assert_eq!(r, "Apr 26 at 10am (America/Los_Angeles)");
        assert_eq!(p, Some(1));

        let (r, p) = split_trailing_percent("3pm (America/Los_Angeles)");
        assert_eq!(r, "3pm (America/Los_Angeles)");
        assert_eq!(p, None);
    }

    #[test]
    fn test_parse_partial_output() {
        let partial = r#"
  Current session
  ████████████████▌                                  33% used
  Resets 12pm (America/Los_Angeles)
"#;
        let data = parse_usage_output(partial);
        assert!(data.session.is_some());
        assert!(data.week_all.is_none());
        assert!(data.week_sonnet.is_none());
    }

    // --- Stuck-state detection unit tests ---

    #[test]
    fn test_is_stuck_loading_without_bars() {
        let capture = "Loading usage data…\n  Esc to cancel";
        let data = parse_usage_output(capture);
        let h = hash_usage_data(&data);
        assert!(is_stuck(capture, h, None));
    }

    #[test]
    fn test_is_stuck_same_hash() {
        // No "Loading" but same hash as previous → stuck
        let capture = "some output without loading";
        let data = parse_usage_output(capture);
        let h = hash_usage_data(&data);
        assert!(is_stuck(capture, h, Some(h)));
    }

    #[test]
    fn test_not_stuck_with_bars_and_changed_data() {
        let capture = SAMPLE_OUTPUT;
        let data = parse_usage_output(capture);
        let h = hash_usage_data(&data);
        // Different previous hash
        assert!(!is_stuck(capture, h, Some(h.wrapping_add(1))));
    }

    #[test]
    fn test_not_stuck_first_poll() {
        // No previous hash → even identical data should not be stuck on first poll
        let capture = SAMPLE_OUTPUT;
        let data = parse_usage_output(capture);
        let h = hash_usage_data(&data);
        assert!(!is_stuck(capture, h, None));
    }

    // Two stacked /usage renders in one capture: an older one that was dismissed
    // mid-render (header but no Resets), then a fresh full render. The parser
    // must use the newest render, not the first match.
    #[test]
    fn test_parse_uses_most_recent_render() {
        let capture = r#"
  Current session
  Loading usage data…

  ❯ /usage

  Current session
  ████████████████▌                                  33% used
  Resets 12pm (America/Los_Angeles)

  Current week (all models)
  ████████████████████                               40% used
  Resets Apr 23 at 12pm (America/Los_Angeles)

  Current week (Sonnet only)
  ███▌                                               7% used
  Resets Apr 23 at 6pm (America/Los_Angeles)
"#;
        let data = parse_usage_output(capture);
        let s = data.session.expect("should parse newest session render");
        assert_eq!(s.percent, 33);
        assert_eq!(s.resets, "12pm (America/Los_Angeles)");
    }

    // A transient "Loading…" line between header and Resets used to push Resets
    // outside the 4-line scan window. The widened window must catch it.
    #[test]
    fn test_parse_tolerates_loading_line_above_resets() {
        let capture = r#"
  Current session
  Loading usage data…

  ████████████████▌                                  33% used
  Resets 12pm (America/Los_Angeles)
"#;
        let data = parse_usage_output(capture);
        let s = data
            .session
            .expect("session should parse with loading line");
        assert_eq!(s.percent, 33);
        assert_eq!(s.resets, "12pm (America/Los_Angeles)");
    }

    #[test]
    fn test_merge_into_preserves_missing_buckets() {
        let shared: SharedUsageData = Arc::new(Mutex::new(None));
        let full = parse_usage_output(SAMPLE_OUTPUT);
        merge_into(&shared, &full);

        // Now a "partial" poll where only week_all parsed.
        let partial = UsageData {
            session: None,
            week_all: Some(UsageBucket {
                label: "Current week (all models)".into(),
                percent: 99,
                resets: "tomorrow".into(),
            }),
            week_sonnet: None,
            last_updated: 12345,
        };
        merge_into(&shared, &partial);

        let guard = shared.lock().unwrap();
        let merged = guard.as_ref().expect("merged data present");
        // Session and Sonnet preserved from prior full poll
        assert_eq!(merged.session.as_ref().unwrap().percent, 33);
        assert_eq!(merged.week_sonnet.as_ref().unwrap().percent, 7);
        // Week updated with the fresh value
        assert_eq!(merged.week_all.as_ref().unwrap().percent, 99);
        // last_updated reflects the freshest poll
        assert_eq!(merged.last_updated, 12345);
    }

    #[test]
    fn test_merge_into_ignores_fully_empty_poll() {
        let shared: SharedUsageData = Arc::new(Mutex::new(None));
        let full = parse_usage_output(SAMPLE_OUTPUT);
        merge_into(&shared, &full);

        // Fully empty poll: must not clobber prior good data.
        merge_into(&shared, &UsageData::default());

        let guard = shared.lock().unwrap();
        let merged = guard.as_ref().expect("prior data must be preserved");
        assert_eq!(merged.session.as_ref().unwrap().percent, 33);
        assert_eq!(merged.week_all.as_ref().unwrap().percent, 40);
        assert_eq!(merged.week_sonnet.as_ref().unwrap().percent, 7);
    }
}
