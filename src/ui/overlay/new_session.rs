use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::NewSessionForm;
use crate::ui::theme::Theme;

/// Render the new session creation form as a centered overlay
pub fn render_new_session(frame: &mut Frame, area: Rect, form: &NewSessionForm, theme: &Theme) {
    let has_completions = form.completions.len() > 1;
    let max_completion_rows: usize = 8;
    let overlay_width = 60u16.min(area.width.saturating_sub(4));

    // Calculate multi-column layout for completions
    let (num_columns, completion_rows) = if has_completions {
        // Inner width = overlay - 2 (borders), leave 2 char padding per column
        let inner_w = overlay_width.saturating_sub(2) as usize;
        let max_candidate_len = form.completions.iter().map(|c| c.len()).max().unwrap_or(0);
        let col_width = max_candidate_len + 3; // 2 leading spaces + 1 trailing
        let cols = (inner_w / col_width).max(1);
        let rows = form.completions.len().div_ceil(cols);
        let visible_rows = rows.min(max_completion_rows);
        (cols, visible_rows)
    } else {
        (1, 0)
    };

    // Base: 7 inner rows (title label + input + spacer + path label + input) + 2 border = 9
    // With completions: + 1 label row + completion_rows
    let overlay_height = if has_completions {
        (9 + 1 + completion_rows as u16).min(area.height.saturating_sub(4))
    } else {
        9u16.min(area.height.saturating_sub(4))
    };

    let x = (area.width.saturating_sub(overlay_width)) / 2;
    let y = (area.height.saturating_sub(overlay_height)) / 2;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    // Clear background
    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .title(" New Session ")
        .title_style(Style::default().fg(theme.primary).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_active));

    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    // Layout fields vertically
    let mut constraints = vec![
        Constraint::Length(1), // Title label
        Constraint::Length(1), // Title input
        Constraint::Length(1), // Spacer
        Constraint::Length(1), // Path label
        Constraint::Length(1), // Path input
    ];
    if has_completions {
        constraints.push(Constraint::Length(1)); // Completion label
        constraints.push(Constraint::Length(completion_rows as u16)); // Completion grid
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    // Title field
    let title_style = if form.focused_field == 0 {
        Style::default().fg(theme.primary)
    } else {
        Style::default().fg(theme.text_muted)
    };
    frame.render_widget(
        Paragraph::new("Title (leave empty for random):").style(title_style),
        chunks[0],
    );

    let title_display = if form.title.is_empty() && form.focused_field == 0 {
        "\u{2588}".to_string() // cursor block
    } else if form.focused_field == 0 {
        format!("{}\u{2588}", form.title)
    } else if form.title.is_empty() {
        "(auto-generated)".to_string()
    } else {
        form.title.clone()
    };
    frame.render_widget(
        Paragraph::new(title_display).style(Style::default().fg(theme.text)),
        chunks[1],
    );

    // Project path field
    let path_style = if form.focused_field == 1 {
        Style::default().fg(theme.primary)
    } else {
        Style::default().fg(theme.text_muted)
    };
    frame.render_widget(Paragraph::new("Project Path:").style(path_style), chunks[3]);

    let path_display = if form.focused_field == 1 {
        format!("{}\u{2588}", form.project_path)
    } else {
        form.project_path.clone()
    };
    frame.render_widget(
        Paragraph::new(path_display).style(Style::default().fg(theme.text)),
        chunks[4],
    );

    // Completion grid (multi-column)
    if has_completions {
        let total_rows = form.completions.len().div_ceil(num_columns);
        let more = if total_rows > max_completion_rows {
            format!(" ({} matches, Tab to cycle)", form.completions.len())
        } else {
            " (Tab to cycle)".to_string()
        };
        frame.render_widget(
            Paragraph::new(more).style(Style::default().fg(theme.text_muted)),
            chunks[5],
        );

        // Determine scroll offset to keep selected row visible
        let selected = form.completion_index.unwrap_or(0);
        let selected_row = selected / num_columns;
        let scroll_offset = if selected_row >= max_completion_rows {
            selected_row - max_completion_rows + 1
        } else {
            0
        };

        // Build lines row by row, column by column
        let grid_area = chunks[6];
        let col_width = grid_area.width as usize / num_columns;
        let mut lines: Vec<Line> = Vec::new();

        for row in scroll_offset..(scroll_offset + completion_rows) {
            let mut spans: Vec<Span> = Vec::new();
            for col in 0..num_columns {
                let idx = row * num_columns + col;
                if idx < form.completions.len() {
                    let candidate = &form.completions[idx];
                    let is_active = form.completion_index == Some(idx);
                    let display = format!("  {:width$}", candidate, width = col_width - 2);
                    // Truncate to col_width to prevent overflow
                    let display: String = display.chars().take(col_width).collect();
                    let style = if is_active {
                        Style::default()
                            .bg(theme.primary)
                            .fg(theme.selected_item_text)
                            .bold()
                    } else {
                        Style::default().fg(theme.text)
                    };
                    spans.push(Span::styled(display, style));
                }
            }
            lines.push(Line::from(spans));
        }

        frame.render_widget(Paragraph::new(lines), grid_area);
    }
}
