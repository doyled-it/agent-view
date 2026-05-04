//! Adapters: convert ratatui_core types to ratatui types for rendering

use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};

/// Convert a `ratatui_core::Color` to a `ratatui::Color`
pub(super) fn convert_core_color(c: ratatui_core::style::Color) -> Color {
    use ratatui_core::style::Color as CC;
    match c {
        CC::Reset => Color::Reset,
        CC::Black => Color::Black,
        CC::Red => Color::Red,
        CC::Green => Color::Green,
        CC::Yellow => Color::Yellow,
        CC::Blue => Color::Blue,
        CC::Magenta => Color::Magenta,
        CC::Cyan => Color::Cyan,
        CC::Gray => Color::Gray,
        CC::DarkGray => Color::DarkGray,
        CC::LightRed => Color::LightRed,
        CC::LightGreen => Color::LightGreen,
        CC::LightYellow => Color::LightYellow,
        CC::LightBlue => Color::LightBlue,
        CC::LightMagenta => Color::LightMagenta,
        CC::LightCyan => Color::LightCyan,
        CC::White => Color::White,
        CC::Rgb(r, g, b) => Color::Rgb(r, g, b),
        CC::Indexed(i) => Color::Indexed(i),
    }
}

/// Convert a `ratatui_core::Style` to a `ratatui::Style`
pub(super) fn convert_core_style(s: ratatui_core::style::Style) -> Style {
    let mut out = Style::default();
    if let Some(fg) = s.fg {
        out = out.fg(convert_core_color(fg));
    }
    if let Some(bg) = s.bg {
        out = out.bg(convert_core_color(bg));
    }
    if s.add_modifier.contains(ratatui_core::style::Modifier::BOLD) {
        out = out.bold();
    }
    if s.add_modifier
        .contains(ratatui_core::style::Modifier::ITALIC)
    {
        out = out.italic();
    }
    if s.add_modifier
        .contains(ratatui_core::style::Modifier::UNDERLINED)
    {
        out = out.underlined();
    }
    if s.add_modifier.contains(ratatui_core::style::Modifier::DIM) {
        out = out.dim();
    }
    if s.add_modifier
        .contains(ratatui_core::style::Modifier::CROSSED_OUT)
    {
        out = out.crossed_out();
    }
    if s.add_modifier
        .contains(ratatui_core::style::Modifier::REVERSED)
    {
        out = out.reversed();
    }
    out
}

/// Convert a `ratatui_core::Span` to a `ratatui::Span`
fn convert_core_span(s: ratatui_core::text::Span<'_>) -> Span<'_> {
    Span::styled(s.content, convert_core_style(s.style))
}

/// Convert a `ratatui_core::Line` to a `ratatui::Line`
pub(super) fn convert_core_line(l: ratatui_core::text::Line<'_>) -> Line<'_> {
    Line::from(
        l.spans
            .into_iter()
            .map(convert_core_span)
            .collect::<Vec<_>>(),
    )
}
