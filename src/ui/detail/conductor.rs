//! Conductor session detail rendering.

use ratatui::layout::Rect;
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::App;
use crate::types::{Session, SessionRole};
use crate::ui::theme::{status_color, Theme};

pub(super) fn render_sub_session_details(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    session: &Session,
    theme: &Theme,
) {
    let block = Block::default()
        .title(" Sub-session Details ")
        .title_style(Style::default().fg(theme.primary).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let paragraph =
        Paragraph::new(build_sub_session_lines(app, session, theme)).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

fn build_sub_session_lines(app: &App, session: &Session, theme: &Theme) -> Vec<Line<'static>> {
    if session.role == SessionRole::Conductor {
        return conductor_lines(app, session, theme);
    }

    child_lines(app, session, theme)
}

fn conductor_lines(app: &App, conductor: &Session, theme: &Theme) -> Vec<Line<'static>> {
    let children: Vec<&Session> = app
        .sessions
        .iter()
        .filter(|child| child.parent_session_id == conductor.id)
        .collect();

    if children.is_empty() {
        return vec![Line::from(vec![Span::styled(
            "No child sessions yet.",
            Style::default().fg(theme.text_muted),
        )])];
    }

    let mut lines = vec![Line::from(vec![
        Span::styled("Children: ", Style::default().fg(theme.text_muted)),
        Span::styled(children.len().to_string(), Style::default().fg(theme.text)),
    ])];

    for child in children {
        lines.push(session_line(child, theme, false));
    }

    lines
}

fn child_lines(app: &App, child: &Session, theme: &Theme) -> Vec<Line<'static>> {
    let parent = app
        .sessions
        .iter()
        .find(|session| session.id == child.parent_session_id);
    let siblings: Vec<&Session> = app
        .sessions
        .iter()
        .filter(|session| session.parent_session_id == child.parent_session_id)
        .collect();

    let mut lines = Vec::new();
    lines.push(Line::from(vec![Span::styled(
        "Parent Conductor",
        Style::default().fg(theme.text_muted),
    )]));

    if let Some(parent) = parent {
        lines.push(session_line(parent, theme, false));
    } else {
        lines.push(Line::from(vec![Span::styled(
            "Parent session not found.",
            Style::default().fg(theme.warning),
        )]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Sub-sessions: ", Style::default().fg(theme.text_muted)),
        Span::styled(siblings.len().to_string(), Style::default().fg(theme.text)),
    ]));

    for sibling in siblings {
        lines.push(session_line(sibling, theme, sibling.id == child.id));
    }

    lines
}

fn session_line(session: &Session, theme: &Theme, selected: bool) -> Line<'static> {
    let marker = if selected { "> " } else { "  " };
    Line::from(vec![
        Span::styled(marker, Style::default().fg(theme.accent)),
        Span::styled(
            format!("{} ", session.status.icon()),
            Style::default().fg(status_color(theme, session.status)),
        ),
        Span::styled(session.title.clone(), Style::default().fg(theme.text)),
        Span::styled("  ", Style::default().fg(theme.text_muted)),
        Span::styled(
            session.status.as_str().to_string(),
            Style::default().fg(status_color(theme, session.status)),
        ),
    ])
}
