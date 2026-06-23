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

/// Word-wrap a styled line to `width` display columns, preserving span styles.
///
/// Captured tmux panes are sized to the agent's terminal, which is often wider
/// than the preview pane — without wrapping, over-width lines are clipped on the
/// right. Wrapping happens at word boundaries (a word longer than the pane is
/// hard-broken as a fallback) so prose stays readable, and the produced row
/// count is exact, which lets the caller align the visible window to the tail
/// without guessing wrapped heights.
///
/// Decorative rule lines (box-drawing borders, runs of dashes/equals — e.g. the
/// Claude/Codex input box) carry the same meaning at any length, so they are
/// left as a single row (clipped to the pane) rather than stacked into copies.
pub(super) fn wrap_line_to_width<'a>(line: Line<'a>, width: usize) -> Vec<Line<'a>> {
    if width == 0 || line.width() <= width {
        return vec![line];
    }
    if is_separator_line(&line) {
        return vec![line];
    }

    // Flatten to styled chars so wrapping can move words across span boundaries,
    // then coalesce each produced row back into styled spans.
    let chars: Vec<(char, Style)> = line
        .spans
        .iter()
        .flat_map(|span| {
            let style = span.style;
            span.content.chars().map(move |ch| (ch, style))
        })
        .collect();

    word_wrap_chars(&chars, width)
        .into_iter()
        .map(chars_to_line)
        .collect()
}

/// Greedy word-wrap over styled chars. Breaks at whitespace; a single token
/// wider than `width` is hard-broken at column boundaries.
fn word_wrap_chars(chars: &[(char, Style)], width: usize) -> Vec<Vec<(char, Style)>> {
    use unicode_width::UnicodeWidthChar;

    let char_w = |c: char| UnicodeWidthChar::width(c).unwrap_or(0);

    let mut rows: Vec<Vec<(char, Style)>> = Vec::new();
    let mut cur: Vec<(char, Style)> = Vec::new();
    let mut cur_w = 0usize;

    // Split into tokens of all-whitespace or all-non-whitespace runs.
    let mut i = 0;
    while i < chars.len() {
        let space = chars[i].0.is_whitespace();
        let start = i;
        while i < chars.len() && chars[i].0.is_whitespace() == space {
            i += 1;
        }
        let token = &chars[start..i];
        let token_w: usize = token.iter().map(|&(c, _)| char_w(c)).sum();

        if cur_w + token_w <= width {
            cur.extend_from_slice(token);
            cur_w += token_w;
        } else if space {
            // Whitespace at a wrap point is dropped instead of leading the next row.
            rows.push(std::mem::take(&mut cur));
            cur_w = 0;
        } else if token_w <= width {
            // Word fits on a fresh row.
            if !cur.is_empty() {
                rows.push(std::mem::take(&mut cur));
            }
            cur.extend_from_slice(token);
            cur_w = token_w;
        } else {
            // Word longer than the pane: hard-break across rows.
            for &(c, st) in token {
                let cw = char_w(c);
                if cur_w > 0 && cur_w + cw > width {
                    rows.push(std::mem::take(&mut cur));
                    cur_w = 0;
                }
                cur.push((c, st));
                cur_w += cw;
            }
        }
    }
    rows.push(cur);
    rows
}

/// Coalesce consecutive same-style chars back into styled spans.
fn chars_to_line<'a>(chars: Vec<(char, Style)>) -> Line<'a> {
    let mut spans: Vec<Span<'a>> = Vec::new();
    let mut buf = String::new();
    let mut cur_style: Option<Style> = None;

    for (ch, style) in chars {
        match cur_style {
            Some(s) if s == style => buf.push(ch),
            _ => {
                if let Some(s) = cur_style {
                    spans.push(Span::styled(std::mem::take(&mut buf), s));
                }
                buf.push(ch);
                cur_style = Some(style);
            }
        }
    }
    if let Some(s) = cur_style {
        spans.push(Span::styled(buf, s));
    }
    Line::from(spans)
}

/// True if the line is purely decorative (whitespace plus box-drawing or rule
/// characters), so wrapping it into multiple rows would only add visual noise.
fn is_separator_line(line: &Line) -> bool {
    let mut saw_decoration = false;
    for span in &line.spans {
        for ch in span.content.chars() {
            if ch.is_whitespace() {
                continue;
            }
            if !is_rule_char(ch) {
                return false;
            }
            saw_decoration = true;
        }
    }
    saw_decoration
}

fn is_rule_char(ch: char) -> bool {
    matches!(ch, '-' | '=' | '_' | '~' | '*' | '·' | '…' | '—' | '–')
        // Box Drawing block covers ─ ━ │ ┃ ┄ ╌ ═ ╭ ╮ ╰ ╯ etc.
        || ('\u{2500}'..='\u{257F}').contains(&ch)
        // Block Elements block covers ▀ ▁ ▔ █ ▏ etc.
        || ('\u{2580}'..='\u{259F}').contains(&ch)
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

    fn words_in(rows: &[Line]) -> Vec<String> {
        let mut out = Vec::new();
        for row in rows {
            let text: String = row.spans.iter().map(|s| s.content.as_ref()).collect();
            for w in text.split_whitespace() {
                out.push(w.to_string());
            }
        }
        out
    }

    #[test]
    fn wrap_breaks_on_word_boundaries_not_mid_word() {
        let line = Line::from("alpha bravo charlie delta");
        let rows = wrap_line_to_width(line, 8);
        for row in &rows {
            assert!(row.width() <= 8, "row exceeds width: {row:?}");
        }
        // Every original word survives intact and in order.
        assert_eq!(words_in(&rows), vec!["alpha", "bravo", "charlie", "delta"]);
    }

    #[test]
    fn wrap_hard_breaks_words_longer_than_width() {
        let line = Line::from("hi supercalifragilistic ok");
        let rows = wrap_line_to_width(line, 6);
        for row in &rows {
            assert!(row.width() <= 6, "row exceeds width: {row:?}");
        }
        // The long word is split, but the short words stay whole.
        let words = words_in(&rows);
        assert!(words.contains(&"hi".to_string()));
        assert!(words.contains(&"ok".to_string()));
    }

    #[test]
    fn wrap_collapses_separator_lines_to_one_row() {
        for rule in ["─".repeat(200), "-".repeat(200), "═".repeat(200)] {
            let rows = wrap_line_to_width(Line::from(rule.clone()), 40);
            assert_eq!(rows.len(), 1, "separator should stay one row: {rule:?}");
        }
    }

    #[test]
    fn wrap_does_not_treat_text_with_letters_as_separator() {
        let line = Line::from("------ Section header that is quite long ------");
        let rows = wrap_line_to_width(line, 20);
        assert!(rows.len() > 1, "text line should still wrap");
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
