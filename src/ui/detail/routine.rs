//! Routine detail panel: preview pane and metadata view

use ratatui::layout::Rect;
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use chrono::{Local, TimeZone};

use ansi_to_tui::IntoText;

use crate::core::schedule::human_readable;
use crate::types::{Routine, RoutineStep};
use crate::ui::theme::Theme;

use super::compat::convert_core_line;
use super::format::truncate;

pub(super) fn render_routine_preview(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    preview_content: &str,
) {
    let block = Block::default()
        .title(" Run Log ")
        .title_style(Style::default().fg(theme.primary).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if preview_content.is_empty() {
        let msg = Paragraph::new("No log available").style(Style::default().fg(theme.text_muted));
        frame.render_widget(msg, inner);
        return;
    }

    let height = inner.height as usize;

    match preview_content.into_text() {
        Ok(core_text) => {
            // Routine logs are typically scroll-tail like a shell, but a
            // routine that runs a TUI (e.g. wraps a `codex` invocation) leaves
            // trailing blank padding in the capture. Trim so visible window
            // doesn't blank out.
            let mut lines = core_text.lines;
            while lines
                .last()
                .is_some_and(|l| l.spans.iter().all(|s| s.content.trim().is_empty()))
            {
                lines.pop();
            }
            let line_count = lines.len();
            let skip = line_count.saturating_sub(height);
            let visible_lines: Vec<Line> = lines
                .into_iter()
                .skip(skip)
                .map(convert_core_line)
                .collect();
            frame.render_widget(Paragraph::new(visible_lines), inner);
        }
        Err(_) => {
            // Fallback to plain text
            let mut lines: Vec<&str> = preview_content.lines().collect();
            while lines.last().is_some_and(|l| l.trim().is_empty()) {
                lines.pop();
            }
            let skip = lines.len().saturating_sub(height);
            let visible: Vec<Line> = lines.into_iter().skip(skip).map(Line::raw).collect();
            frame.render_widget(Paragraph::new(visible), inner);
        }
    }
}

pub(super) fn render_routine_metadata(
    frame: &mut Frame,
    area: Rect,
    routine: &Routine,
    theme: &Theme,
) {
    let block = Block::default()
        .title(" Routine Details ")
        .title_style(Style::default().fg(theme.primary).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let schedule_str = human_readable(&routine.schedule);
    let enabled_str = if routine.enabled { "Yes" } else { "No" };
    let next_str = routine
        .next_run_at
        .map(|t| {
            Local
                .timestamp_millis_opt(t)
                .single()
                .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_else(|| "???".to_string())
        })
        .unwrap_or_else(|| "N/A".to_string());

    let steps_str = routine
        .steps
        .iter()
        .enumerate()
        .map(|(i, step)| match step {
            RoutineStep::Claude { prompt } => {
                format!("  {}. [claude] {}", i + 1, truncate(prompt, 30))
            }
            RoutineStep::Shell { command } => {
                format!("  {}. [shell] {}", i + 1, truncate(command, 30))
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let mut all_lines = vec![
        Line::from(vec![
            Span::styled(" Name: ", Style::default().fg(theme.text_muted)),
            Span::styled(&routine.name, Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled(" Schedule: ", Style::default().fg(theme.text_muted)),
            Span::styled(schedule_str, Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled(" Enabled: ", Style::default().fg(theme.text_muted)),
            Span::styled(enabled_str, Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled(" Next run: ", Style::default().fg(theme.text_muted)),
            Span::styled(next_str, Style::default().fg(theme.info)),
        ]),
        Line::from(vec![
            Span::styled(" Working dir: ", Style::default().fg(theme.text_muted)),
            Span::styled(&routine.working_dir, Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled(" Run count: ", Style::default().fg(theme.text_muted)),
            Span::styled(
                routine.run_count.to_string(),
                Style::default().fg(theme.text),
            ),
        ]),
        Line::from(Span::styled(
            " Steps:",
            Style::default().fg(theme.text_muted),
        )),
    ];

    for line_str in steps_str.lines() {
        all_lines.push(Line::from(Span::styled(
            line_str.to_string(),
            Style::default().fg(theme.text),
        )));
    }

    frame.render_widget(Paragraph::new(all_lines), inner);
}
