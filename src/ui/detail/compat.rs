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

/// Hard-wrap a styled line to `width` display columns, preserving span styles.
///
/// Captured tmux panes are sized to the agent's terminal, which is often wider
/// than the preview pane — without wrapping, over-width lines are clipped on the
/// right. We wrap at column boundaries (like a terminal) rather than word
/// boundaries so the resulting row count is exact, which lets the caller align
/// the visible window to the tail without guessing wrapped heights.
pub(super) fn wrap_line_to_width<'a>(line: Line<'a>, width: usize) -> Vec<Line<'a>> {
    use unicode_width::UnicodeWidthChar;

    if width == 0 || line.width() <= width {
        return vec![line];
    }

    let mut rows: Vec<Line<'a>> = Vec::new();
    let mut cur_spans: Vec<Span<'a>> = Vec::new();
    let mut cur_width = 0usize;

    for span in line.spans {
        let style = span.style;
        let mut buf = String::new();
        for ch in span.content.chars() {
            let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
            // Break before a char that would overflow, but never on an empty
            // row (guards against a single char wider than the whole pane).
            if cur_width > 0 && cur_width + cw > width {
                if !buf.is_empty() {
                    cur_spans.push(Span::styled(std::mem::take(&mut buf), style));
                }
                rows.push(Line::from(std::mem::take(&mut cur_spans)));
                cur_width = 0;
            }
            buf.push(ch);
            cur_width += cw;
        }
        if !buf.is_empty() {
            cur_spans.push(Span::styled(buf, style));
        }
    }
    rows.push(Line::from(cur_spans));
    rows
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
    fn wrap_leaves_short_lines_untouched() {
        let line = Line::from("short");
        let rows = wrap_line_to_width(line, 10);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].width(), 5);
    }

    #[test]
    fn wrap_splits_wide_line_at_column_boundary() {
        let line = Line::from("A".repeat(25));
        let rows = wrap_line_to_width(line, 10);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].width(), 10);
        assert_eq!(rows[1].width(), 10);
        assert_eq!(rows[2].width(), 5);
        let total: usize = rows.iter().map(|r| r.width()).sum();
        assert_eq!(total, 25);
    }

    #[test]
    fn wrap_preserves_span_styles_across_breaks() {
        let line = Line::from(vec![
            Span::styled("aaaaaa", Style::default().fg(Color::Red)),
            Span::styled("bbbbbb", Style::default().fg(Color::Blue)),
        ]);
        let rows = wrap_line_to_width(line, 4);
        // 12 columns / width 4 => 3 rows.
        assert_eq!(rows.len(), 3);
        // Every produced span keeps either the red or blue foreground.
        for row in &rows {
            for span in &row.spans {
                assert!(
                    span.style.fg == Some(Color::Red) || span.style.fg == Some(Color::Blue),
                    "span lost its style: {span:?}"
                );
            }
        }
    }

    #[test]
    fn wrap_handles_wide_unicode_chars() {
        // Each CJK char is 2 columns wide; 4 chars = 8 columns, width 4 => 2 rows.
        let line = Line::from("漢字漢字");
        let rows = wrap_line_to_width(line, 4);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].width(), 4);
        assert_eq!(rows[1].width(), 4);
    }

    #[test]
    fn wrap_with_zero_width_is_noop() {
        let line = Line::from("anything");
        let rows = wrap_line_to_width(line, 0);
        assert_eq!(rows.len(), 1);
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
