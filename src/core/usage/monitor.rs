use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use crate::core::{logger, status, tmux};
use crate::types::UsageData;

use super::parser::parse_usage_output;
use super::SharedUsageData;
use super::META_SESSION_NAME;

const POLL_INTERVAL: Duration = Duration::from_secs(30);
const INIT_POLL_INTERVAL: Duration = Duration::from_millis(500);
const INIT_TIMEOUT: Duration = Duration::from_secs(30);

/// How many polls in a row with no bars (or identical data) before we consider the session stuck.
const STUCK_THRESHOLD: u8 = 3;
/// Max recoveries in any rolling 10-minute window before we give up recovering.
const MAX_RECOVERIES_PER_WINDOW: usize = 3;
const RECOVERY_WINDOW: Duration = Duration::from_secs(600); // 10 minutes

/// Initialize (or re-initialize) the meta session: create it, wait for idle prompt,
/// accept trust prompt, send /usage, and wait for the quota bars to render.
/// Returns Ok(capture) with the rendered output on success, or Err with a reason.
fn init_meta_session() -> Result<String, &'static str> {
    if tmux::session_exists(META_SESSION_NAME) {
        let _ = tmux::kill_session(META_SESSION_NAME);
    }

    if tmux::create_session(META_SESSION_NAME, Some("claude"), Some("/tmp"), None).is_err() {
        return Err("claude not available");
    }

    // Pin the pane width so /usage output doesn't wrap. Without this the
    // detached session inherits tmux's default-size (~80 cols) and bar/Resets
    // lines wrap at "% used", breaking suffix-based percent extraction.
    let _ = tmux::resize_window(META_SESSION_NAME, 200, 50);

    // Wait for Claude to reach idle prompt
    let start = Instant::now();
    let mut trust_accepted = false;
    loop {
        std::thread::sleep(INIT_POLL_INTERVAL);
        if start.elapsed() > INIT_TIMEOUT {
            let _ = tmux::kill_session(META_SESSION_NAME);
            return Err("timed out waiting for idle prompt");
        }
        if let Ok(output) = tmux::capture_pane_joined(META_SESSION_NAME, Some(-20)) {
            // Accept the workspace trust prompt if it appears
            if !trust_accepted && output.contains("Yes, I trust this folder") {
                let _ = tmux::send_keys_raw(META_SESSION_NAME, "Enter");
                trust_accepted = true;
                continue;
            }
            let s = status::parse_tool_status(&output, Some("claude"));
            if s.has_idle_prompt {
                break;
            }
        }
    }

    // Brief settle before sending command — Claude may not be fully ready
    std::thread::sleep(Duration::from_secs(1));

    // Send /usage — the command shows a persistent usage view
    if tmux::send_keys(META_SESSION_NAME, "/usage").is_err() {
        let _ = tmux::kill_session(META_SESSION_NAME);
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
        if let Ok(output) = tmux::capture_pane_joined(session_name, Some(-30)) {
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

pub(super) fn monitor_loop(shared: SharedUsageData) {
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

        if !tmux::session_exists(META_SESSION_NAME) {
            if let Ok(mut guard) = shared.lock() {
                *guard = None;
            }
            return;
        }

        // Escape closes the /usage view, returning to idle prompt
        let _ = tmux::send_keys_raw(META_SESSION_NAME, "Escape");
        std::thread::sleep(Duration::from_secs(1));
        // Drop accumulated scrollback so parse_bucket's rposition only sees
        // the upcoming render — bounds the cost of repeated polls.
        let _ = tmux::clear_history(META_SESSION_NAME);
        // Re-send /usage to get fresh data
        let _ = tmux::send_keys(META_SESSION_NAME, "/usage");

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
                        logger::log_diagnostic(&format!(
                            "usage monitor: recovery failed — {reason}; continuing to poll"
                        ));
                        // Don't return; keep the loop alive
                    }
                }
            } else {
                logger::log_diagnostic(&format!(
                    "usage monitor: recovery cap reached ({MAX_RECOVERIES_PER_WINDOW} in 10 min); skipping auto-recover, continuing to poll"
                ));
                // Reset unchanged_count so we don't spam on every subsequent poll
                unchanged_count = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use crate::types::{UsageBucket, UsageData};

    use super::super::parser::parse_usage_output;
    use super::super::SharedUsageData;
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
