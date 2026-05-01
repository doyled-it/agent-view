//! Run detail panel: metadata view for a routine run

use ratatui::layout::Rect;
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::types::{RoutineRun, RunStatus};
use crate::ui::theme::Theme;

pub(super) fn render_run_metadata(
    frame: &mut Frame,
    area: Rect,
    run: &RoutineRun,
    routine_name: &str,
    theme: &Theme,
) {
    let block = Block::default()
        .title(" Run Details ")
        .title_style(Style::default().fg(theme.primary).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    use chrono::{Local, TimeZone};
    let started = Local
        .timestamp_millis_opt(run.started_at)
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "???".to_string());
    let finished = run
        .finished_at
        .map(|t| {
            Local
                .timestamp_millis_opt(t)
                .single()
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| "???".to_string())
        })
        .unwrap_or_else(|| "running...".to_string());
    let duration = run
        .finished_at
        .map(|f| {
            let ms = f - run.started_at;
            let secs = ms / 1000;
            if secs > 3600 {
                format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
            } else if secs > 60 {
                format!("{}m{}s", secs / 60, secs % 60)
            } else {
                format!("{}s", secs)
            }
        })
        .unwrap_or_else(|| "...".to_string());

    let status_color = match run.status {
        RunStatus::Completed => theme.success,
        RunStatus::Running => theme.info,
        RunStatus::Failed => theme.error,
        RunStatus::TimedOut => theme.warning,
        RunStatus::Crashed => theme.error,
    };

    let mut all_lines = vec![
        Line::from(vec![
            Span::styled(" Routine: ", Style::default().fg(theme.text_muted)),
            Span::styled(routine_name, Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled(" Status: ", Style::default().fg(theme.text_muted)),
            Span::styled(
                format!("{} {}", run.status.icon(), run.status),
                Style::default().fg(status_color),
            ),
        ]),
        Line::from(vec![
            Span::styled(" Started: ", Style::default().fg(theme.text_muted)),
            Span::styled(started, Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled(" Finished: ", Style::default().fg(theme.text_muted)),
            Span::styled(finished, Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled(" Duration: ", Style::default().fg(theme.text_muted)),
            Span::styled(duration, Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled(" Steps: ", Style::default().fg(theme.text_muted)),
            Span::styled(
                format!("{}/{}", run.steps_completed, run.steps_total),
                Style::default().fg(theme.text),
            ),
        ]),
    ];

    if let Some(ref promoted_id) = run.promoted_session_id {
        all_lines.push(Line::from(vec![
            Span::styled(" Promoted: ", Style::default().fg(theme.text_muted)),
            Span::styled(promoted_id, Style::default().fg(theme.accent)),
        ]));
    }

    frame.render_widget(Paragraph::new(all_lines), inner);
}
