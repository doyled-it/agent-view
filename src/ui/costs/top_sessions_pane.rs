//! Top-N sessions by cost.

use chrono::Local;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::core::cost::{render_usd, SessionCost};
use crate::ui::theme::Theme;

const ROW_LIMIT: usize = 10;

pub fn build_top_session_lines<'a>(
    rows: &[SessionCost],
    now_unix: i64,
    theme: &'a Theme,
) -> Vec<Line<'a>> {
    if rows.is_empty() {
        return vec![Line::from(Span::styled(
            "no cost events",
            Style::default().fg(theme.text_muted),
        ))];
    }
    rows.iter()
        .map(|r| {
            Line::from(vec![
                Span::raw(format!("{:<22} ", truncate(&r.session_label, 22))),
                Span::raw(render_usd(r.microdollars)),
                Span::raw("  "),
                Span::styled(
                    format!("({})", relative_time(r.last_event_ts_unix, now_unix)),
                    Style::default().fg(theme.text_muted),
                ),
            ])
        })
        .collect()
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n.saturating_sub(1)).collect::<String>() + "…"
    }
}

fn relative_time(then_unix: i64, now_unix: i64) -> String {
    let delta = (now_unix - then_unix).max(0);
    if delta < 60 {
        format!("{}s ago", delta)
    } else if delta < 3600 {
        format!("{}m ago", delta / 60)
    } else if delta < 86_400 {
        format!("{}h ago", delta / 3600)
    } else {
        format!("{}d ago", delta / 86_400)
    }
}

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let rows = match &app.storage {
        Some(s) => s
            .lock()
            .ok()
            .and_then(|guard| guard.top_sessions(app.cost_period, ROW_LIMIT).ok())
            .unwrap_or_default(),
        None => Vec::new(),
    };
    let now = Local::now().timestamp();
    let lines = build_top_session_lines(&rows, now, &app.theme);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Top sessions ");
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Tool;
    use crate::ui::theme::Theme;

    #[test]
    fn relative_time_buckets() {
        assert_eq!(relative_time(900, 1000), "1m ago");
        assert_eq!(relative_time(0, 7200), "2h ago");
        assert_eq!(relative_time(0, 200_000), "2d ago");
    }

    #[test]
    fn empty_rows_shows_no_events() {
        let t = Theme::dark();
        let lines = build_top_session_lines(&[], 0, &t);
        let txt: String = lines[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<Vec<&str>>()
            .join("");
        assert!(txt.contains("no cost events"));
    }

    #[test]
    fn row_renders_label_cost_and_relative_time() {
        let t = Theme::dark();
        let rows = vec![SessionCost {
            session_id: "id-1".into(),
            session_label: "agent-view/main".into(),
            tool: Tool::Claude,
            microdollars: 18_400_000,
            last_event_ts_unix: 1000,
        }];
        let lines = build_top_session_lines(&rows, 11_800, &t);
        let s: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(s.contains("agent-view/main"));
        assert!(s.contains("$18.40"));
        assert!(s.contains("3h ago"));
    }
}
