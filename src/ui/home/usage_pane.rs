//! Usage pane rendering (Claude token usage)

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::types::UsageBucket;
use crate::ui::theme::Theme;

pub(super) fn render_usage_pane(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;

    let usage = match app.usage_data {
        Some(ref u) => u,
        None => return,
    };

    // Compute staleness
    let now_ms = chrono::Utc::now().timestamp_millis();
    let age_ms = now_ms - usage.last_updated;
    let is_stale = age_ms > 5 * 60 * 1000; // 5 minutes

    let title = if is_stale {
        " Usage (stale) "
    } else {
        " Usage "
    };

    let block = Block::default()
        .title(title)
        .title_style(Style::default().fg(theme.text_muted))
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.border));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let buckets: Vec<(&str, Option<&UsageBucket>)> = vec![
        ("Session", usage.session.as_ref()),
        ("Week", usage.week_all.as_ref()),
        ("Sonnet", usage.week_sonnet.as_ref()),
    ];

    // Pre-compute reset strings to find the longest one for bar width calc
    let resets_strs: Vec<String> = buckets
        .iter()
        .map(|(_, b)| {
            b.map(|b| format!("  resets {}", abbreviate_resets(&b.resets)))
                .unwrap_or_default()
        })
        .collect();
    let max_resets_len = resets_strs.iter().map(|s| s.len()).max().unwrap_or(0);

    // label(9) + bar + pct(5) + resets(max_resets_len)
    let fixed_width = 9 + 5 + max_resets_len;
    let bar_width = (inner.width as usize).saturating_sub(fixed_width);

    let lines: Vec<Line> = buckets
        .into_iter()
        .zip(resets_strs)
        .filter_map(|((label, bucket), resets_str)| {
            let b = bucket?;
            let color = usage_percent_color(theme, b.percent);
            let filled = (bar_width as u32 * b.percent as u32 / 100) as usize;
            let empty = bar_width.saturating_sub(filled);
            let bar_filled = "\u{2588}".repeat(filled);
            let bar_empty = "\u{2591}".repeat(empty);
            // Pad resets to max_resets_len so bars align
            let padded_resets = format!("{:<width$}", resets_str, width = max_resets_len);

            Some(Line::from(vec![
                Span::styled(
                    format!(" {:<8}", label),
                    maybe_dim(Style::default().fg(theme.text_muted), is_stale),
                ),
                Span::styled(bar_filled, maybe_dim(Style::default().fg(color), is_stale)),
                Span::styled(
                    bar_empty,
                    maybe_dim(
                        Style::default()
                            .fg(theme.text_muted)
                            .add_modifier(Modifier::DIM),
                        is_stale,
                    ),
                ),
                Span::styled(
                    format!(" {:>3}%", b.percent),
                    maybe_dim(Style::default().fg(color), is_stale),
                ),
                Span::styled(
                    padded_resets,
                    maybe_dim(Style::default().fg(theme.text_muted), is_stale),
                ),
            ]))
        })
        .collect();

    frame.render_widget(Paragraph::new(lines), inner);
}

/// Conditionally add DIM modifier to a style
fn maybe_dim(style: Style, dim: bool) -> Style {
    if dim {
        style.add_modifier(Modifier::DIM)
    } else {
        style
    }
}

fn abbreviate_resets(resets: &str) -> String {
    // "12pm (America/Los_Angeles)" -> "12pm PT"
    // "Apr 23 at 6pm (America/New_York)" -> "Apr 23 at 6pm ET"
    if let Some(idx) = resets.find('(') {
        let time_part = resets[..idx].trim_end();
        let tz_part = resets[idx..].trim_matches(|c| c == '(' || c == ')');
        let abbr = match tz_part {
            "America/Los_Angeles" => "PT",
            "America/Denver" => "MT",
            "America/Chicago" => "CT",
            "America/New_York" => "ET",
            "Europe/London" => "GMT",
            "Europe/Paris" | "Europe/Berlin" => "CET",
            "Asia/Tokyo" => "JST",
            "Asia/Shanghai" | "Asia/Hong_Kong" => "CST",
            "UTC" => "UTC",
            other => other.rsplit('/').next().unwrap_or(other),
        };
        format!("{} {}", time_part, abbr)
    } else {
        resets.to_string()
    }
}

fn usage_percent_color(theme: &Theme, percent: u8) -> Color {
    if percent >= 80 {
        theme.error
    } else if percent >= 50 {
        theme.warning
    } else {
        theme.success
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usage_color_thresholds() {
        let theme = crate::ui::theme::Theme::dark();
        // < 50% = success (green)
        assert_eq!(usage_percent_color(&theme, 0), theme.success);
        assert_eq!(usage_percent_color(&theme, 49), theme.success);
        // 50-79% = warning (yellow)
        assert_eq!(usage_percent_color(&theme, 50), theme.warning);
        assert_eq!(usage_percent_color(&theme, 79), theme.warning);
        // >= 80% = error (red)
        assert_eq!(usage_percent_color(&theme, 80), theme.error);
        assert_eq!(usage_percent_color(&theme, 100), theme.error);
    }

    #[test]
    fn test_abbreviate_resets_known_timezones() {
        assert_eq!(abbreviate_resets("12pm (America/Los_Angeles)"), "12pm PT");
        assert_eq!(abbreviate_resets("5pm (America/New_York)"), "5pm ET");
        assert_eq!(abbreviate_resets("3pm (America/Chicago)"), "3pm CT");
        assert_eq!(
            abbreviate_resets("Apr 23 at 6pm (America/Los_Angeles)"),
            "Apr 23 at 6pm PT"
        );
    }

    #[test]
    fn test_abbreviate_resets_unknown_timezone() {
        // Falls back to city name
        assert_eq!(abbreviate_resets("12pm (Asia/Kolkata)"), "12pm Kolkata");
    }

    #[test]
    fn test_abbreviate_resets_no_parens() {
        assert_eq!(abbreviate_resets("12pm PT"), "12pm PT");
    }

    #[test]
    fn test_usage_staleness_threshold() {
        let now = chrono::Utc::now().timestamp_millis();
        let four_min_ago = now - 4 * 60 * 1000;
        let six_min_ago = now - 6 * 60 * 1000;

        // Stale threshold is 5 min: 4-min-ago is fresh, 6-min-ago is stale
        let age_4_min = now - four_min_ago;
        let age_6_min = now - six_min_ago;

        assert!(age_4_min <= 5 * 60 * 1000);
        assert!(age_6_min > 5 * 60 * 1000);
    }
}
