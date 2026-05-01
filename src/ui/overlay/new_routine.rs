use chrono::{Local, TimeZone};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::*;
use ratatui::Frame;

use crate::app::{NewRoutineForm, ScheduleFrequency};
use crate::core::schedule;
use crate::types::RoutineStep;
use crate::ui::theme::Theme;

/// Render the new routine creation form as a centered overlay
pub fn render_new_routine(frame: &mut Frame, area: Rect, form: &NewRoutineForm, theme: &Theme) {
    let overlay_width = 65u16.min(area.width.saturating_sub(4));
    let overlay_height = 22u16.min(area.height.saturating_sub(4));

    let x = (area.width.saturating_sub(overlay_width)) / 2;
    let y = (area.height.saturating_sub(overlay_height)) / 2;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    frame.render_widget(Clear, overlay_area);

    let title = if form.edit_routine_id.is_some() {
        " Edit Routine "
    } else {
        " New Routine "
    };
    let block = Block::default()
        .title(title)
        .title_style(Style::default().fg(theme.primary).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_active));

    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Name label
            Constraint::Length(1), // Name input
            Constraint::Length(1), // Tool label + toggle
            Constraint::Length(1), // Working dir label
            Constraint::Length(1), // Working dir input
            Constraint::Length(1), // Schedule label
            Constraint::Length(1), // Frequency selector
            Constraint::Length(1), // Time/params
            Constraint::Length(1), // Next run preview
            Constraint::Length(1), // Steps label
            Constraint::Min(2),    // Steps list
            Constraint::Length(1), // Notifications + timeout
            Constraint::Length(1), // Submit hint
        ])
        .split(inner);

    let field_label_style = |field: usize| {
        if form.focused_field == field {
            Style::default().fg(theme.primary)
        } else {
            Style::default().fg(theme.text_muted)
        }
    };

    // Field 0: Name
    frame.render_widget(
        Paragraph::new(" Name:").style(field_label_style(0)),
        chunks[0],
    );
    let name_display = format!(
        " {}{}",
        form.name,
        if form.focused_field == 0 {
            "\u{2588}"
        } else {
            ""
        }
    );
    frame.render_widget(
        Paragraph::new(name_display).style(Style::default().fg(theme.text)),
        chunks[1],
    );

    // Field 1: Tool
    let tool_display = format!(" Tool: [{}]  (Space to toggle)", form.default_tool);
    frame.render_widget(
        Paragraph::new(tool_display).style(field_label_style(1)),
        chunks[2],
    );

    // Field 2: Working dir
    frame.render_widget(
        Paragraph::new(" Working Directory:").style(field_label_style(2)),
        chunks[3],
    );
    let dir_display = format!(
        " {}{}",
        form.working_dir,
        if form.focused_field == 2 {
            "\u{2588}"
        } else {
            ""
        }
    );
    frame.render_widget(
        Paragraph::new(dir_display).style(Style::default().fg(theme.text)),
        chunks[4],
    );

    // Field 3: Schedule
    frame.render_widget(
        Paragraph::new(" Schedule:").style(field_label_style(3)),
        chunks[5],
    );
    let freq_display = format!(" <{}> (Left/Right to change)", form.frequency.label());
    frame.render_widget(
        Paragraph::new(freq_display).style(Style::default().fg(theme.text)),
        chunks[6],
    );

    // Time params based on frequency
    let time_display = match form.frequency {
        ScheduleFrequency::Hourly => {
            format!(" At minute :{:02} (Up/Down)", form.minute)
        }
        ScheduleFrequency::Daily => {
            format!(" At {:02}:{:02} (Up/Down = hour)", form.hour, form.minute)
        }
        ScheduleFrequency::Weekly => {
            let days: Vec<&str> = form
                .weekdays
                .iter()
                .enumerate()
                .filter(|(_, &sel)| sel)
                .map(|(i, _)| schedule::Weekday::all()[i].label())
                .collect();
            format!(
                " {} at {:02}:{:02} (Space=toggle day)",
                days.join(","),
                form.hour,
                form.minute
            )
        }
        ScheduleFrequency::Monthly => {
            format!(
                " On day {} at {:02}:{:02} (+/- day)",
                form.month_day, form.hour, form.minute
            )
        }
        ScheduleFrequency::Yearly => {
            format!(
                " Month {} day {} at {:02}:{:02} (+/- month, [/] day)",
                form.month, form.month_day, form.hour, form.minute
            )
        }
        ScheduleFrequency::Advanced => {
            format!(
                " Cron: {}{}",
                form.cron_raw,
                if form.focused_field == 3 {
                    "\u{2588}"
                } else {
                    ""
                }
            )
        }
    };
    frame.render_widget(
        Paragraph::new(time_display).style(Style::default().fg(theme.text_muted)),
        chunks[7],
    );

    // Next run preview
    let cron = form.cron_expression();
    let next_preview = schedule::next_run(&cron)
        .map(|ms| {
            Local
                .timestamp_millis_opt(ms)
                .single()
                .map(|dt| format!(" Next run: {}", dt.format("%a %b %d at %H:%M")))
                .unwrap_or_else(|| " Next run: ???".to_string())
        })
        .unwrap_or_else(|| " Next run: (invalid schedule)".to_string());
    frame.render_widget(
        Paragraph::new(next_preview).style(Style::default().fg(theme.info)),
        chunks[8],
    );

    // Field 4: Steps
    frame.render_widget(
        Paragraph::new(format!(" Steps ({}):  a:add  d:delete", form.steps.len()))
            .style(field_label_style(4)),
        chunks[9],
    );
    let step_lines: Vec<Line> = form
        .steps
        .iter()
        .enumerate()
        .map(|(i, step)| {
            let (type_str, content) = match step {
                RoutineStep::Claude { prompt } => ("claude", prompt.as_str()),
                RoutineStep::Shell { command } => ("shell", command.as_str()),
            };
            let truncated = if content.len() > 40 {
                &content[..40]
            } else {
                content
            };
            Line::from(format!("  {}. [{}] {}", i + 1, type_str, truncated))
        })
        .collect();
    let mut all_lines = step_lines;
    if let Some(ref text) = form.editing_step {
        all_lines.push(Line::from(format!("  > {}\u{2588}", text)));
    }
    frame.render_widget(
        Paragraph::new(all_lines)
            .style(Style::default().fg(theme.text))
            .wrap(Wrap { trim: false }),
        chunks[10],
    );

    // Fields 5+6: Notifications and timeout (highlighted independently)
    let notify_str = if form.notify { "ON" } else { "OFF" };
    let timeout_min = form.step_timeout_secs / 60;
    let notify_style = if form.focused_field == 5 {
        Style::default().fg(theme.primary)
    } else {
        Style::default().fg(theme.text_muted)
    };
    let timeout_style = if form.focused_field == 6 {
        Style::default().fg(theme.primary)
    } else {
        Style::default().fg(theme.text_muted)
    };
    let bottom_line = Line::from(vec![
        Span::styled(format!(" Notify: [{}]", notify_str), notify_style),
        Span::styled("    ", Style::default().fg(theme.text_muted)),
        Span::styled(
            format!("Timeout: {}min (Left/Right)", timeout_min),
            timeout_style,
        ),
    ]);
    frame.render_widget(Paragraph::new(bottom_line), chunks[11]);

    // Submit hint
    frame.render_widget(
        Paragraph::new(" Enter: save  Esc: cancel  Tab: next field")
            .style(Style::default().fg(theme.text_muted)),
        chunks[12],
    );
}
