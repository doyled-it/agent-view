//! Overlay rendering for new session form and confirm dialogs

use crate::app::{CommandPalette, ConfirmDialog, GroupForm, MoveForm, NewSessionForm, RenameForm};
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
    let overlay_width = 60u16.min(area.width.saturating_sub(4));
    let overlay_height = 9u16.min(area.height.saturating_sub(4));

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
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Title label
            Constraint::Length(1), // Title input
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // Path label
            Constraint::Length(1), // Path input
        ])
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
    frame.render_widget(
        Paragraph::new("Project Path:").style(path_style),
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
        Paragraph::new(format!("{}\u{2588}", form.input))
            .style(Style::default().fg(theme.text)),
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
                Style::default().bg(theme.primary).fg(theme.selected_item_text)
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
                Style::default().bg(theme.primary).fg(theme.selected_item_text)
            } else {
                Style::default().fg(theme.text)
            };
            let line = Line::from(vec![
                Span::styled(format!("  {} ", item.label), style),
                Span::styled(
                    format!("  {}", item.key_hint),
                    if i == palette.selected {
                        Style::default().bg(theme.primary).fg(theme.selected_item_text)
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
    let width = area.width.min(60);
    let height = area.height.min(22);
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

    let left_bindings = vec![
        ("j / k", "Navigate up/down"),
        ("Enter", "Attach to session"),
        ("n", "New session"),
        ("s", "Stop session"),
        ("r", "Restart session"),
        ("d", "Delete session"),
        ("R", "Rename session/group"),
        ("m", "Move to group"),
        ("g", "Create group"),
        ("J / K", "Move group down/up"),
    ];

    let right_bindings = vec![
        ("Space", "Select session"),
        ("Ctrl+A", "Select all"),
        ("S", "Cycle sort mode"),
        ("p", "Pin/unpin session"),
        ("i", "Toggle follow-up"),
        ("!", "Toggle notifications"),
        ("e", "Export log"),
        ("a", "Toggle activity feed"),
        ("/", "Search"),
        ("Ctrl+K", "Command palette"),
    ];

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);

    let left_lines: Vec<Line> = left_bindings
        .iter()
        .map(|(key, desc)| {
            Line::from(vec![
                Span::styled(
                    format!(" {:>9} ", key),
                    Style::default().fg(theme.secondary).bold(),
                ),
                Span::styled(*desc, Style::default().fg(theme.text)),
            ])
        })
        .collect();

    let right_lines: Vec<Line> = right_bindings
        .iter()
        .map(|(key, desc)| {
            Line::from(vec![
                Span::styled(
                    format!(" {:>9} ", key),
                    Style::default().fg(theme.secondary).bold(),
                ),
                Span::styled(*desc, Style::default().fg(theme.text)),
            ])
        })
        .collect();

    frame.render_widget(Paragraph::new(left_lines), cols[0]);
    frame.render_widget(Paragraph::new(right_lines), cols[1]);
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
