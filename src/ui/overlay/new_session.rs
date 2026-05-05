use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::NewSessionForm;
use crate::ui::theme::Theme;

/// Render the new session creation form as a centered overlay.
pub fn render_new_session(frame: &mut Frame, area: Rect, form: &NewSessionForm, theme: &Theme) {
    let has_path_completions = form.focused_field == 1 && form.completions.len() > 1;
    let has_branch_completions = form.focused_field == 2 && form.completions.len() > 1;
    let has_completions = has_path_completions || has_branch_completions;
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

    // Build constraints dynamically so the completion grid appears inline,
    // directly under the field whose completion is active.
    //
    // Layout sections (each 1 row unless noted):
    //   title label, title input, spacer,
    //   path label, path input,
    //   [if path completions: completion hint, completion grid (N rows)],
    //   spacer,
    //   branch label, branch input,
    //   [if branch completions: completion hint, completion grid (N rows)],
    //   spacer,
    //   base label, base input,
    //   [if error: error row],
    //   help hint
    // title label + input + spacer; path label + input — always present
    let mut constraints: Vec<Constraint> = vec![
        Constraint::Length(1), // title label
        Constraint::Length(1), // title input
        Constraint::Length(1), // spacer
        Constraint::Length(1), // path label
        Constraint::Length(1), // path input
    ];
    // path completions (inline)
    let path_completion_start = if has_path_completions {
        let idx = constraints.len();
        constraints.push(Constraint::Length(1)); // hint
        constraints.push(Constraint::Length(completion_rows as u16)); // grid
        Some(idx)
    } else {
        None
    };
    // spacer before branch
    constraints.push(Constraint::Length(1));
    // branch label + input
    constraints.push(Constraint::Length(1));
    constraints.push(Constraint::Length(1));
    // branch completions (inline)
    let branch_completion_start = if has_branch_completions {
        let idx = constraints.len();
        constraints.push(Constraint::Length(1)); // hint
        constraints.push(Constraint::Length(completion_rows as u16)); // grid
        Some(idx)
    } else {
        None
    };
    // spacer before base
    constraints.push(Constraint::Length(1));
    // base label + input
    constraints.push(Constraint::Length(1));
    constraints.push(Constraint::Length(1));
    // error row
    if form.error.is_some() {
        constraints.push(Constraint::Length(1));
    }
    // help hint
    constraints.push(Constraint::Length(1));

    let overlay_height = (constraints.len() as u16 + 2).min(area.height.saturating_sub(4));

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

    let mut i: usize = 0;

    // Title
    frame.render_widget(
        Paragraph::new("Title (leave empty for random):")
            .style(label_style(form.focused_field == 0)),
        chunks[i],
    );
    i += 1;
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
        chunks[i],
    );
    i += 1;
    i += 1; // spacer

    // Path
    frame.render_widget(
        Paragraph::new("Project Path:").style(label_style(form.focused_field == 1)),
        chunks[i],
    );
    i += 1;
    let path_display = if form.focused_field == 1 {
        format!("{}\u{2588}", form.project_path)
    } else {
        form.project_path.clone()
    };
    frame.render_widget(
        Paragraph::new(path_display).style(Style::default().fg(theme.text)),
        chunks[i],
    );
    i += 1;

    // Path completions (inline, directly after path input)
    if let Some(start) = path_completion_start {
        debug_assert_eq!(i, start);
        render_completion_hint(
            frame,
            chunks[i],
            form,
            theme,
            num_columns,
            max_completion_rows,
        );
        i += 1;
        render_completion_grid(
            frame,
            chunks[i],
            form,
            theme,
            num_columns,
            completion_rows,
            max_completion_rows,
        );
        i += 1;
    }

    i += 1; // spacer before branch

    // Worktree branch
    let mode_hint = if form.worktree_new_branch {
        "[new branch \u{2014} ^t to attach]"
    } else {
        "[attach existing \u{2014} ^t to create]"
    };
    let branch_label = format!("Worktree Branch (empty to skip): {}", mode_hint);
    frame.render_widget(
        Paragraph::new(branch_label).style(label_style(form.focused_field == 2)),
        chunks[i],
    );
    i += 1;
    let branch_display = if form.focused_field == 2 {
        format!("{}\u{2588}", form.worktree_branch)
    } else if form.worktree_branch.is_empty() {
        "(no worktree)".to_string()
    } else {
        form.worktree_branch.clone()
    };
    frame.render_widget(
        Paragraph::new(branch_display).style(Style::default().fg(theme.text)),
        chunks[i],
    );
    i += 1;

    // Branch completions (inline, directly after branch input)
    if let Some(start) = branch_completion_start {
        debug_assert_eq!(i, start);
        render_completion_hint(
            frame,
            chunks[i],
            form,
            theme,
            num_columns,
            max_completion_rows,
        );
        i += 1;
        render_completion_grid(
            frame,
            chunks[i],
            form,
            theme,
            num_columns,
            completion_rows,
            max_completion_rows,
        );
        i += 1;
    }

    i += 1; // spacer before base

    // Base ref — Issue 3: clearer label, dimmed in attach mode
    let base_label = if form.worktree_new_branch {
        "Branch from (default: current HEAD):".to_string()
    } else {
        "Branch from (unused \u{2014} attach mode):".to_string()
    };
    let base_label_style = if !form.worktree_new_branch {
        // Attach mode: always dim regardless of focus
        Style::default().fg(theme.text_muted)
    } else {
        label_style(form.focused_field == 3)
    };
    frame.render_widget(
        Paragraph::new(base_label).style(base_label_style),
        chunks[i],
    );
    i += 1;
    let base_display = if form.focused_field == 3 {
        format!("{}\u{2588}", form.worktree_base)
    } else if form.worktree_base.is_empty() {
        if form.worktree_new_branch {
            "current HEAD".to_string()
        } else {
            "\u{2014}".to_string()
        }
    } else {
        form.worktree_base.clone()
    };
    let base_value_style = if !form.worktree_new_branch {
        Style::default().fg(theme.text_muted)
    } else {
        Style::default().fg(theme.text)
    };
    frame.render_widget(
        Paragraph::new(base_display).style(base_value_style),
        chunks[i],
    );
    i += 1;

    if let Some(err) = &form.error {
        frame.render_widget(
            Paragraph::new(format!("\u{26a0} {}", err)).style(Style::default().fg(theme.warning)),
            chunks[i],
        );
        i += 1;
    }

    // Help hint line — always last
    frame.render_widget(
        Paragraph::new(
            "^S save · Esc cancel · Tab/\u{2193} next · \u{21e7}Tab/\u{2191} back · ^T toggle",
        )
        .style(Style::default().fg(theme.text_muted)),
        chunks[i],
    );
}

fn render_completion_hint(
    frame: &mut Frame,
    area: Rect,
    form: &NewSessionForm,
    theme: &Theme,
    num_columns: usize,
    max_completion_rows: usize,
) {
    let total_rows = form.completions.len().div_ceil(num_columns);
    let more = if total_rows > max_completion_rows {
        format!(" ({} matches, Tab to cycle)", form.completions.len())
    } else {
        " (Tab to cycle)".to_string()
    };
    frame.render_widget(
        Paragraph::new(more).style(Style::default().fg(theme.text_muted)),
        area,
    );
}

fn render_completion_grid(
    frame: &mut Frame,
    area: Rect,
    form: &NewSessionForm,
    theme: &Theme,
    num_columns: usize,
    completion_rows: usize,
    max_completion_rows: usize,
) {
    let selected = form.completion_index.unwrap_or(0);
    let selected_row = selected / num_columns;
    let scroll_offset = if selected_row >= max_completion_rows {
        selected_row - max_completion_rows + 1
    } else {
        0
    };

    let col_width = area.width as usize / num_columns;
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
    frame.render_widget(Paragraph::new(lines), area);
}
