//! Adapters: convert ratatui_core types to ratatui types for rendering

use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};

/// Convert a `ratatui_core::Color` to a `ratatui::Color`
fn convert_core_color(c: ratatui_core::style::Color) -> Color {
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
fn convert_core_style(s: ratatui_core::style::Style) -> Style {
    let mut out = Style::default();
    if let Some(fg) = s.fg {
        out = out.fg(convert_core_color(fg));
    }
    if s.add_modifier.contains(ratatui_core::style::Modifier::BOLD) {
        out = out.bold();
    }
    if s.add_modifier
        .contains(ratatui_core::style::Modifier::UNDERLINED)
    {
        out = out.underlined();
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Modifier;
    use ratatui_core::style::Modifier as CoreModifier;
    use ratatui_core::style::{Color as CoreColor, Style as CoreStyle};
    use ratatui_core::text::{Line as CoreLine, Span as CoreSpan};

    fn converted_background(bg: CoreColor) -> Option<Color> {
        let source = CoreLine::from(vec![CoreSpan::styled(
            "output",
            CoreStyle::default().fg(CoreColor::White).bg(bg),
        )]);

        let converted = convert_core_line(source);
        let span = &converted.spans[0];
        assert_eq!(span.style.fg, Some(Color::White));
        span.style.bg
    }

    #[test]
    fn drops_default_black_backgrounds_from_preview_spans() {
        for bg in [
            CoreColor::Black,
            CoreColor::Indexed(0),
            CoreColor::Rgb(0, 0, 0),
        ] {
            assert_eq!(converted_background(bg), None);
        }
    }

    #[test]
    fn drops_colored_backgrounds_from_preview_spans() {
        assert_eq!(converted_background(CoreColor::Red), None);
    }

    #[test]
    fn drops_dim_modifier_from_preview_spans() {
        let source = CoreLine::from(vec![CoreSpan::styled(
            "output",
            CoreStyle::default().add_modifier(CoreModifier::DIM | CoreModifier::BOLD),
        )]);

        let converted = convert_core_line(source);
        let modifiers = converted.spans[0].style.add_modifier;

        assert!(!modifiers.contains(Modifier::DIM));
        assert!(modifiers.contains(Modifier::BOLD));
    }

    #[test]
    fn drops_italic_modifier_from_preview_spans() {
        let source = CoreLine::from(vec![CoreSpan::styled(
            "recap",
            CoreStyle::default().add_modifier(CoreModifier::ITALIC | CoreModifier::BOLD),
        )]);

        let converted = convert_core_line(source);
        let modifiers = converted.spans[0].style.add_modifier;

        assert!(!modifiers.contains(Modifier::ITALIC));
        assert!(modifiers.contains(Modifier::BOLD));
    }
}
