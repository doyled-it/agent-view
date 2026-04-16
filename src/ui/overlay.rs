//! Overlay rendering for new session form and confirm dialogs

use crate::app::{
    CommandPalette, ConfirmDialog, GroupForm, MoveForm, NewSessionForm, NoteForm, RenameForm,
};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::prelude::*;
use ratatui::widgets::*;

/// Render the new session creation form as a centered overlay
pub fn render_new_session(
    frame: &mut Frame,
    area: Rect,
    form: &NewSessionForm,
    theme: &crate::ui::theme::Theme,
) {
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

/// Render the rename overlay for sessions and groups
pub fn render_rename(
    frame: &mut Frame,
    area: Rect,
    form: &RenameForm,
    theme: &crate::ui::theme::Theme,
) {
    let overlay_width = 50u16.min(area.width.saturating_sub(4));
    let overlay_height = 5u16.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(overlay_width)) / 2;
    let y = (area.height.saturating_sub(overlay_height)) / 2;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    frame.render_widget(Clear, overlay_area);

    let title = match form.target_type {
        crate::app::RenameTarget::Session => " Rename Session ",
        crate::app::RenameTarget::Group => " Rename Group ",
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
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    frame.render_widget(
        Paragraph::new("New name:").style(Style::default().fg(theme.text_muted)),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(format!("{}\u{2588}", form.input)).style(Style::default().fg(theme.text)),
        chunks[1],
    );
}

/// Render a confirmation dialog as a centered overlay
pub fn render_confirm(
    frame: &mut Frame,
    area: Rect,
    dialog: &ConfirmDialog,
    theme: &crate::ui::theme::Theme,
) {
    let overlay_width = 50u16.min(area.width.saturating_sub(4));
    let overlay_height = 5u16.min(area.height.saturating_sub(4));

    let x = (area.width.saturating_sub(overlay_width)) / 2;
    let y = (area.height.saturating_sub(overlay_height)) / 2;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .title(" Confirm ")
        .title_style(Style::default().fg(theme.warning).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.warning));

    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    frame.render_widget(
        Paragraph::new(dialog.message.as_str()).style(Style::default().fg(theme.text)),
        chunks[0],
    );

    frame.render_widget(
        Paragraph::new("y/Enter = yes, n/Esc = no").style(Style::default().fg(theme.text_muted)),
        chunks[1],
    );
}

/// Render the move session overlay — list of groups to choose from
pub fn render_move(
    frame: &mut Frame,
    area: Rect,
    form: &MoveForm,
    theme: &crate::ui::theme::Theme,
) {
    let overlay_height = (form.groups.len() as u16 + 4).min(area.height.saturating_sub(4));
    let overlay_width = 50u16.min(area.width.saturating_sub(4));
    let x = (area.width.saturating_sub(overlay_width)) / 2;
    let y = (area.height.saturating_sub(overlay_height)) / 2;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    frame.render_widget(Clear, overlay_area);

    let title = format!(" Move \"{}\" ", form.session_title);
    let block = Block::default()
        .title(title)
        .title_style(Style::default().fg(theme.primary).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_active));

    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    let items: Vec<ListItem> = form
        .groups
        .iter()
        .enumerate()
        .map(|(i, (_, name))| {
            let style = if i == form.selected {
                Style::default()
                    .bg(theme.primary)
                    .fg(theme.selected_item_text)
            } else {
                Style::default().fg(theme.text)
            };
            ListItem::new(format!("  {}", name)).style(style)
        })
        .collect();

    frame.render_widget(List::new(items), inner);
}

/// Render the command palette overlay — centered searchable list of actions
pub fn render_command_palette(
    frame: &mut Frame,
    area: Rect,
    palette: &CommandPalette,
    theme: &crate::ui::theme::Theme,
) {
    let max_items = 10;
    let visible = palette.filtered.len().min(max_items);
    let overlay_height = (visible as u16 + 4).min(area.height.saturating_sub(4));
    let overlay_width = 50u16.min(area.width.saturating_sub(4));
    let x = (area.width.saturating_sub(overlay_width)) / 2;
    let y = area.height / 6; // Near the top
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .title(" Commands ")
        .title_style(Style::default().fg(theme.primary).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_active));

    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);

    // Search input
    let input_line = Line::from(vec![
        Span::styled("> ", Style::default().fg(theme.primary)),
        Span::styled(palette.query.as_str(), Style::default().fg(theme.text)),
        Span::styled("\u{2588}", Style::default().fg(theme.primary)),
    ]);
    frame.render_widget(Paragraph::new(input_line), chunks[0]);

    // Filtered items
    let items: Vec<ListItem> = palette
        .filtered
        .iter()
        .enumerate()
        .take(max_items)
        .map(|(i, &idx)| {
            let item = &palette.items[idx];
            let style = if i == palette.selected {
                Style::default()
                    .bg(theme.primary)
                    .fg(theme.selected_item_text)
            } else {
                Style::default().fg(theme.text)
            };
            let line = Line::from(vec![
                Span::styled(format!("  {} ", item.label), style),
                Span::styled(
                    format!("  {}", item.key_hint),
                    if i == palette.selected {
                        Style::default()
                            .bg(theme.primary)
                            .fg(theme.selected_item_text)
                    } else {
                        Style::default().fg(theme.text_muted)
                    },
                ),
            ]);
            ListItem::new(line)
        })
        .collect();

    frame.render_widget(List::new(items), chunks[1]);
}

/// Render the keybinding help overlay
pub fn render_help(frame: &mut Frame, area: Rect, theme: &crate::ui::theme::Theme) {
    let width = area.width.min(72);
    let height = area.height.min(24);
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let popup = Rect::new(area.x + x, area.y + y, width, height);

    frame.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Keybindings ")
        .title_style(Style::default().fg(theme.primary).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);

    let section_style = Style::default().fg(theme.accent).bold();
    let key_style = Style::default().fg(theme.secondary).bold();
    let desc_style = Style::default().fg(theme.text);

    fn section_header<'a>(title: &'a str, style: Style) -> Line<'a> {
        Line::from(Span::styled(format!(" {}", title), style))
    }

    fn binding<'a>(key: &'a str, desc: &'a str, ks: Style, ds: Style) -> Line<'a> {
        Line::from(vec![
            Span::styled(format!(" {:>9} ", key), ks),
            Span::styled(desc, ds),
        ])
    }

    let left_lines: Vec<Line> = vec![
        section_header("Navigation", section_style),
        binding("j / k", "Navigate", key_style, desc_style),
        binding("Enter", "Attach session", key_style, desc_style),
        binding("Home/End", "First/Last", key_style, desc_style),
        binding("PgUp/Dn", "Page scroll", key_style, desc_style),
        binding("1-9", "Jump to group", key_style, desc_style),
        binding("/", "Search", key_style, desc_style),
        Line::from(""),
        section_header("View", section_style),
        binding("v", "Cycle panel", key_style, desc_style),
        binding("a", "Activity feed", key_style, desc_style),
        binding("t", "Select theme", key_style, desc_style),
        binding("?", "This help", key_style, desc_style),
        Line::from(""),
        section_header("Groups", section_style),
        binding("g", "Create group", key_style, desc_style),
        binding("J / K", "Move group", key_style, desc_style),
    ];

    let right_lines: Vec<Line> = vec![
        section_header("Sessions", section_style),
        binding("n", "New session", key_style, desc_style),
        binding("s", "Stop session", key_style, desc_style),
        binding("r", "Restart", key_style, desc_style),
        binding("d", "Delete", key_style, desc_style),
        binding("R", "Rename", key_style, desc_style),
        binding("m", "Move to group", key_style, desc_style),
        Line::from(""),
        section_header("Actions", section_style),
        binding("Space", "Select session", key_style, desc_style),
        binding("Ctrl+A", "Select all", key_style, desc_style),
        binding("e", "Export log", key_style, desc_style),
        binding("!", "Notifications", key_style, desc_style),
        binding("i", "Follow-up flag", key_style, desc_style),
        binding("p", "Pin/unpin", key_style, desc_style),
        binding("S", "Cycle sort", key_style, desc_style),
        binding("Ctrl+K", "Command palette", key_style, desc_style),
    ];

    frame.render_widget(Paragraph::new(left_lines), cols[0]);
    frame.render_widget(Paragraph::new(right_lines), cols[1]);
}

/// Render the theme selection overlay with live preview
pub fn render_theme_select(
    frame: &mut Frame,
    area: Rect,
    form: &crate::app::ThemeSelectForm,
    theme: &crate::ui::theme::Theme,
) {
    let width = area.width.min(30);
    let height = (form.options.len() as u16 + 2).min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let popup = Rect::new(area.x + x, area.y + y, width, height);

    frame.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Theme ")
        .title_style(Style::default().fg(theme.primary).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let items: Vec<ListItem> = form
        .options
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let is_selected = i == form.selected;
            let style = if is_selected {
                Style::default()
                    .fg(theme.selected_item_text)
                    .bg(theme.primary)
                    .bold()
            } else {
                Style::default().fg(theme.text)
            };
            ListItem::new(format!("  {}  ", name)).style(style)
        })
        .collect();

    frame.render_widget(List::new(items), inner);
}

/// Render the add note overlay
pub fn render_add_note(
    frame: &mut Frame,
    area: Rect,
    form: &NoteForm,
    theme: &crate::ui::theme::Theme,
) {
    let overlay_width = 60u16.min(area.width.saturating_sub(4));
    let overlay_height = 12u16.min(area.height.saturating_sub(4));

    let x = (area.width.saturating_sub(overlay_width)) / 2;
    let y = (area.height.saturating_sub(overlay_height)) / 2;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .title(" Add Note ")
        .title_style(Style::default().fg(theme.primary).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_active));

    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    // Text area with cursor
    let display_text = format!("{}\u{2588}", form.text);
    let text_widget = Paragraph::new(display_text)
        .style(Style::default().fg(theme.text))
        .wrap(Wrap { trim: false });
    frame.render_widget(text_widget, inner);
}

/// Render the group creation overlay
pub fn render_group_manage(
    frame: &mut Frame,
    area: Rect,
    form: &GroupForm,
    theme: &crate::ui::theme::Theme,
) {
    let overlay_width = 50u16.min(area.width.saturating_sub(4));
    let overlay_height = 5u16.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(overlay_width)) / 2;
    let y = (area.height.saturating_sub(overlay_height)) / 2;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .title(" New Group ")
        .title_style(Style::default().fg(theme.primary).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_active));

    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    frame.render_widget(
        Paragraph::new("Group name:").style(Style::default().fg(theme.text_muted)),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(format!("{}\u{2588}", form.name)).style(Style::default().fg(theme.text)),
        chunks[1],
    );
}
