use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::NewSessionForm;
use crate::ui::theme::Theme;

/// Render the new session creation form as a centered overlay.
pub fn render_new_session(frame: &mut Frame, area: Rect, form: &NewSessionForm, theme: &Theme) {
    let has_completions =
        (form.focused_field == 1 || form.focused_field == 2) && form.completions.len() > 1;
    let max_completion_rows: usize = 6;
    let overlay_width = 64u16.min(area.width.saturating_sub(4));

    let (num_columns, completion_rows) = if has_completions {
        let inner_w = overlay_width.saturating_sub(2) as usize;
        let max_candidate_len = form.completions.iter().map(|c| c.len()).max().unwrap_or(0);
        let col_width = max_candidate_len + 3;
        let cols = (inner_w / col_width).max(1);
        let rows = form.completions.len().div_ceil(cols);
        (cols, rows.min(max_completion_rows))
    } else {
        (1, 0)
    };

    // Inner rows: title-label, title-in, blank, path-label, path-in, blank,
    // branch-label, branch-in, blank, base-label, base-in (= 11 fixed).
    // Optional: error row (+1). Optional completion label + grid (+1 + N).
    // Always: help hint row (+1).
    let mut inner_rows: u16 = 11;
    if form.error.is_some() {
        inner_rows += 1;
    }
    let extra = if has_completions {
        1 + completion_rows as u16
    } else {
        0
    };
    // +1 for the help hint line
    let overlay_height = (inner_rows + 2 + extra + 1).min(area.height.saturating_sub(4));

    let x = (area.width.saturating_sub(overlay_width)) / 2;
    let y = (area.height.saturating_sub(overlay_height)) / 2;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .title(" New Session ")
        .title_style(Style::default().fg(theme.primary).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_active));

    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    let mut constraints = vec![
        Constraint::Length(1), // title label
        Constraint::Length(1), // title input
        Constraint::Length(1), // spacer
        Constraint::Length(1), // path label
        Constraint::Length(1), // path input
        Constraint::Length(1), // spacer
        Constraint::Length(1), // branch label
        Constraint::Length(1), // branch input
        Constraint::Length(1), // spacer
        Constraint::Length(1), // base label
        Constraint::Length(1), // base input
    ];
    if form.error.is_some() {
        constraints.push(Constraint::Length(1));
    }
    if has_completions {
        constraints.push(Constraint::Length(1));
        constraints.push(Constraint::Length(completion_rows as u16));
    }
    constraints.push(Constraint::Length(1)); // help hint

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    let label_style = |focused: bool| {
        if focused {
            Style::default().fg(theme.primary)
        } else {
            Style::default().fg(theme.text_muted)
        }
    };

    // Title
    frame.render_widget(
        Paragraph::new("Title (leave empty for random):")
            .style(label_style(form.focused_field == 0)),
        chunks[0],
    );
    let title_display = if form.title.is_empty() && form.focused_field == 0 {
        "\u{2588}".to_string()
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

    // Path
    frame.render_widget(
        Paragraph::new("Project Path:").style(label_style(form.focused_field == 1)),
        chunks[3],
    );
    let path_display = if form.focused_field == 1 {
        format!("{}\u{2588}", form.project_path)
    } else {
        form.project_path.clone()
    };
    frame.render_widget(
        Paragraph::new(path_display).style(Style::default().fg(theme.text)),
        chunks[4],
    );

    // Worktree branch
    let mode_hint = if form.worktree_new_branch {
        "[new branch \u{2014} ^t to attach]"
    } else {
        "[attach existing \u{2014} ^t to create]"
    };
    let branch_label = format!("Worktree Branch (empty to skip): {}", mode_hint);
    frame.render_widget(
        Paragraph::new(branch_label).style(label_style(form.focused_field == 2)),
        chunks[6],
    );
    let branch_display = if form.focused_field == 2 {
        format!("{}\u{2588}", form.worktree_branch)
    } else if form.worktree_branch.is_empty() {
        "(no worktree)".to_string()
    } else {
        form.worktree_branch.clone()
    };
    frame.render_widget(
        Paragraph::new(branch_display).style(Style::default().fg(theme.text)),
        chunks[7],
    );

    // Base ref
    frame.render_widget(
        Paragraph::new("Base Ref (empty = HEAD):").style(label_style(form.focused_field == 3)),
        chunks[9],
    );
    let base_display = if form.focused_field == 3 {
        format!("{}\u{2588}", form.worktree_base)
    } else if form.worktree_base.is_empty() {
        "HEAD".to_string()
    } else {
        form.worktree_base.clone()
    };
    frame.render_widget(
        Paragraph::new(base_display).style(Style::default().fg(theme.text)),
        chunks[10],
    );

    let mut next_chunk = 11usize;
    if let Some(err) = &form.error {
        frame.render_widget(
            Paragraph::new(format!("\u{26a0} {}", err)).style(Style::default().fg(theme.warning)),
            chunks[next_chunk],
        );
        next_chunk += 1;
    }

    if has_completions {
        let total_rows = form.completions.len().div_ceil(num_columns);
        let more = if total_rows > max_completion_rows {
            format!(" ({} matches, Tab to cycle)", form.completions.len())
        } else {
            " (Tab to cycle)".to_string()
        };
        frame.render_widget(
            Paragraph::new(more).style(Style::default().fg(theme.text_muted)),
            chunks[next_chunk],
        );
        next_chunk += 1;

        let selected = form.completion_index.unwrap_or(0);
        let selected_row = selected / num_columns;
        let scroll_offset = if selected_row >= max_completion_rows {
            selected_row - max_completion_rows + 1
        } else {
            0
        };

        let grid_area = chunks[next_chunk];
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
        next_chunk += 1;
    }

    // Help hint line — always rendered last
    frame.render_widget(
        Paragraph::new(
            "^S save · Esc cancel · Tab/\u{2193} next · \u{21e7}Tab/\u{2191} back · ^T toggle",
        )
        .style(Style::default().fg(theme.text_muted)),
        chunks[next_chunk],
    );
}
