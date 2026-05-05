use crate::types::{UsageBucket, UsageData};

pub(super) fn parse_usage_output(output: &str) -> UsageData {
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
}
