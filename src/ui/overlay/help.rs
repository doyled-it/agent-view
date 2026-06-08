use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{ActiveTab, App};

/// Render the keybinding help overlay
pub fn render_help(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
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
        binding("Tab", "Switch tab", key_style, desc_style),
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
        binding("d", "Delete group", key_style, desc_style),
        binding("R", "Rename group", key_style, desc_style),
    ];

    let right_lines: Vec<Line> = if app.active_tab == ActiveTab::Routines {
        vec![
            section_header("Routines", section_style),
            binding("n", "New routine", key_style, desc_style),
            binding("e", "Edit routine", key_style, desc_style),
            binding("Space", "Enable/disable", key_style, desc_style),
            binding("Enter", "Expand runs", key_style, desc_style),
            binding("d", "Delete", key_style, desc_style),
            binding("p", "Pin/unpin", key_style, desc_style),
            binding("R", "Rename", key_style, desc_style),
            Line::from(""),
            section_header("Runs", section_style),
            binding("r", "Inspect/resume", key_style, desc_style),
            binding("P", "Promote to session", key_style, desc_style),
            binding("d", "Delete run", key_style, desc_style),
            Line::from(""),
            section_header("General", section_style),
            binding("Tab", "Switch tab", key_style, desc_style),
            binding("Ctrl+K", "Command palette", key_style, desc_style),
            binding("M", "MCP profiles", key_style, desc_style),
        ]
    } else {
        vec![
            section_header("Sessions", section_style),
            binding("n", "New session", key_style, desc_style),
            binding("s", "Stop session", key_style, desc_style),
            binding("r", "Restart", key_style, desc_style),
            binding("d", "Delete", key_style, desc_style),
            binding("R", "Rename", key_style, desc_style),
            binding("m", "Move to group", key_style, desc_style),
            Line::from(""),
            section_header("New Session MCP", section_style),
            binding("Enter", "Expand", key_style, desc_style),
            binding("Space", "Toggle", key_style, desc_style),
            binding("D/Del", "Delete profile", key_style, desc_style),
            binding("Ctrl+P", "Save profile", key_style, desc_style),
            binding("Ctrl+U", "Update profile", key_style, desc_style),
            Line::from(""),
            section_header("Actions", section_style),
            binding("Space", "Select session", key_style, desc_style),
            binding("Ctrl+A", "Select all", key_style, desc_style),
            binding("e", "Export log", key_style, desc_style),
            binding("!", "Notifications", key_style, desc_style),
            binding("i", "Follow-up flag", key_style, desc_style),
            binding("w", "Waiting marker", key_style, desc_style),
            binding("p", "Pin/unpin", key_style, desc_style),
            binding("S", "Cycle sort", key_style, desc_style),
            binding("M", "MCP profiles", key_style, desc_style),
            binding("Ctrl+K", "Command palette", key_style, desc_style),
            binding("Palette", "Sync MCP servers", key_style, desc_style),
        ]
    };

    frame.render_widget(Paragraph::new(left_lines), cols[0]);
    frame.render_widget(Paragraph::new(right_lines), cols[1]);
}
