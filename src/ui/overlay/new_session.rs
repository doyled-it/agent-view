use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::NewSessionForm;
use crate::ui::theme::Theme;

const MCP_FIELD: usize = 5;
const DEFAULT_OVERLAY_WIDTH: u16 = 64;
const WIDE_OVERLAY_WIDTH: u16 = 72;

/// Render the new session creation form as a centered overlay.
pub fn render_new_session(frame: &mut Frame, area: Rect, form: &NewSessionForm, theme: &Theme) {
    let has_path_completions = form.focused_field == 2 && form.completions.len() > 1;
    let has_branch_completions = form.focused_field == 3 && form.completions.len() > 1;
    let has_base_completions = form.focused_field == 4 && form.completions.len() > 1;
    let has_completions = has_path_completions || has_branch_completions || has_base_completions;
    let max_completion_rows: usize = 6;
    let mcp_line_specs = build_new_session_mcp_line_specs(form);
    let mcp_needs_wide_overlay = mcp_line_specs
        .iter()
        .any(|line| line.text.chars().count() > DEFAULT_OVERLAY_WIDTH.saturating_sub(2) as usize);
    let target_overlay_width = if mcp_needs_wide_overlay {
        WIDE_OVERLAY_WIDTH
    } else {
        DEFAULT_OVERLAY_WIDTH
    };
    let overlay_width = target_overlay_width.min(area.width.saturating_sub(4));

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
    //   runner label, runner cycle row, spacer (after runner),
    //   title label, title input, spacer,
    //   path label, path input,
    //   [if path completions: completion hint, completion grid (N rows)],
    //   spacer,
    //   branch label, branch input,
    //   [if branch completions: completion hint, completion grid (N rows)],
    //   spacer,
    //   base label, base input,
    //   [if base completions: completion hint, completion grid (N rows)],
    //   MCP summary,
    //   [if MCP expanded: one row per server],
    //   [if error: error row],
    //   help hint
    // runner label + cycle row + spacer; title label + input + spacer; path label + input — always present
    let mut constraints: Vec<Constraint> = vec![
        Constraint::Length(1), // runner label
        Constraint::Length(1), // runner cycle row
        Constraint::Length(1), // spacer (after runner, before title)
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
    // base completions (inline)
    if has_base_completions {
        constraints.push(Constraint::Length(1)); // hint
        constraints.push(Constraint::Length(completion_rows as u16)); // grid
    }
    // MCP summary/selection rows
    for _ in &mcp_line_specs {
        constraints.push(Constraint::Length(1));
    }
    // error row
    if form.error.is_some() {
        constraints.push(Constraint::Length(1));
    }
    // help hint
    constraints.push(Constraint::Length(1));

    // Sum actual constraint lengths — the grid constraint covers multiple rows.
    let inner_height: u16 = constraints
        .iter()
        .map(|c| match c {
            Constraint::Length(n) => *n,
            _ => 0,
        })
        .sum();
    let overlay_height = (inner_height + 2).min(area.height.saturating_sub(4));

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

    // Runner row: label, cycle value, then a one-row spacer before the title.
    frame.render_widget(
        Paragraph::new("Runner:").style(label_style(form.focused_field == 0)),
        chunks[i],
    );
    let runner_line = Line::from(vec![
        Span::styled("  \u{2039} ", Style::default().fg(theme.text_muted)),
        Span::styled(
            display_runner_label(form.runner.as_str()),
            Style::default().fg(theme.text),
        ),
        Span::styled(" \u{203a}", Style::default().fg(theme.text_muted)),
    ]);
    frame.render_widget(Paragraph::new(runner_line), chunks[i + 1]);
    i += 3;

    // Title
    frame.render_widget(
        Paragraph::new("Title (leave empty for random):")
            .style(label_style(form.focused_field == 1)),
        chunks[i],
    );
    i += 1;
    let title_display = if form.title.is_empty() && form.focused_field == 1 {
        "\u{2588}".to_string()
    } else if form.focused_field == 1 {
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
        Paragraph::new("Project Path:").style(label_style(form.focused_field == 2)),
        chunks[i],
    );
    i += 1;
    let path_display = if form.focused_field == 2 {
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
        "new (^T attach)"
    } else {
        "attach (^T new)"
    };
    let branch_label = format!("Worktree Branch \u{00b7} {}:", mode_hint);
    frame.render_widget(
        Paragraph::new(branch_label).style(label_style(form.focused_field == 3)),
        chunks[i],
    );
    i += 1;
    let branch_display = if form.focused_field == 3 {
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

    // Base ref — clearer label, dimmed in attach mode
    let base_label = if form.worktree_new_branch {
        "Branch off from (optional):".to_string()
    } else {
        "Branch off from (n/a in attach mode):".to_string()
    };
    let base_label_style = if !form.worktree_new_branch {
        // Attach mode: always dim regardless of focus
        Style::default().fg(theme.text_muted)
    } else {
        label_style(form.focused_field == 4)
    };
    frame.render_widget(
        Paragraph::new(base_label).style(base_label_style),
        chunks[i],
    );
    i += 1;
    let base_display = if form.focused_field == 4 {
        format!("{}\u{2588}", form.worktree_base)
    } else if form.worktree_base.is_empty() {
        if form.worktree_new_branch {
            "(currently checked out branch)".to_string()
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

    // Base-ref completions (inline, directly after base input)
    if has_base_completions {
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

    for line in build_new_session_mcp_lines(form, theme) {
        frame.render_widget(Paragraph::new(line), chunks[i]);
        i += 1;
    }

    if let Some(err) = &form.error {
        frame.render_widget(
            Paragraph::new(format!("\u{26a0} {}", err)).style(Style::default().fg(theme.warning)),
            chunks[i],
        );
        i += 1;
    }

    // Help hint line — always last
    let base_hint =
        "^S save · Esc cancel · Tab/\u{2193} next · \u{21e7}Tab/\u{2191} back · ^T toggle";
    let hint = match form.focused_field {
        0 => format!("\u{2190}/\u{2192} cycle runner   {}", base_hint),
        MCP_FIELD if form.mcp_expanded => format!("Space toggle · Enter MCP   {}", base_hint),
        MCP_FIELD => format!("Enter MCP   {}", base_hint),
        _ => base_hint.to_string(),
    };
    frame.render_widget(
        Paragraph::new(hint).style(Style::default().fg(theme.text_muted)),
        chunks[i],
    );
}

/// ASCII-uppercase the first character of a runner's `name()` for display.
/// E.g., "claude" -> "Claude", "shell" -> "Shell".
fn display_runner_label(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) => c.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

#[derive(Debug)]
struct McpLineSpec {
    text: String,
    server_row: Option<usize>,
}

fn build_new_session_mcp_line_specs(form: &NewSessionForm) -> Vec<McpLineSpec> {
    let mut lines = vec![McpLineSpec {
        text: format!("MCP: {}", form.mcp_summary()),
        server_row: None,
    }];

    if !form.mcp_expanded {
        return lines;
    }

    for (idx, server) in form.mcp_servers.iter().enumerate() {
        let marker = if mcp_server_enabled(form, &server.id) {
            "[x]"
        } else {
            "[ ]"
        };
        let mut text = format!("  {} {}", marker, server.display_name);
        if !server.tool_filter_enforceable {
            text.push_str("  server-level only");
        }
        lines.push(McpLineSpec {
            text,
            server_row: Some(idx),
        });
    }

    lines
}

fn build_new_session_mcp_lines(form: &NewSessionForm, theme: &Theme) -> Vec<Line<'static>> {
    let specs = build_new_session_mcp_line_specs(form);
    build_new_session_mcp_lines_from_specs(&specs, form, theme)
}

fn build_new_session_mcp_lines_from_specs(
    specs: &[McpLineSpec],
    form: &NewSessionForm,
    theme: &Theme,
) -> Vec<Line<'static>> {
    specs
        .iter()
        .map(|spec| {
            let is_selected = form.focused_field == MCP_FIELD
                && form.mcp_expanded
                && spec.server_row == Some(form.mcp_selected_row);
            let style = if is_selected {
                Style::default()
                    .bg(theme.primary)
                    .fg(theme.selected_item_text)
                    .bold()
            } else if form.focused_field == MCP_FIELD && spec.server_row.is_none() {
                Style::default().fg(theme.primary)
            } else if spec.server_row.is_none() {
                Style::default().fg(theme.text_muted)
            } else {
                Style::default().fg(theme.text)
            };
            Line::from(Span::styled(spec.text.clone(), style))
        })
        .collect()
}

fn mcp_server_enabled(form: &NewSessionForm, id: &str) -> bool {
    if form.mcp_selection.is_all_servers() {
        return true;
    }

    form.mcp_selection
        .servers
        .iter()
        .find(|server| server.id == id)
        .map(|server| server.enabled)
        .unwrap_or(true)
}

#[cfg(test)]
fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

#[cfg(test)]
pub(crate) fn render_new_session_lines_for_test(form: &NewSessionForm) -> Vec<String> {
    let theme = Theme::dark();
    build_new_session_mcp_lines(form, &theme)
        .iter()
        .map(line_text)
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::mcp::{McpSelection, McpServerSelection};
    use crate::types::Tool;

    #[test]
    fn new_session_overlay_renders_mcp_summary_collapsed() {
        let form = NewSessionForm::new();

        let lines = render_new_session_lines_for_test(&form);

        assert!(
            lines.iter().any(|line| line == "MCP: All MCP servers"),
            "rendered lines: {lines:#?}"
        );
    }

    #[test]
    fn new_session_overlay_renders_enabled_mcp_rows_with_server_level_note() {
        let mut form = NewSessionForm::new();
        form.runner = Tool::Codex;
        form.mcp_expanded = true;
        form.set_mcp_servers_for_test(vec!["GitLabMITRE".into()]);

        let lines = render_new_session_lines_for_test(&form);

        assert!(
            lines.iter().any(|line| line.contains("[x] GitLabMITRE")),
            "rendered lines: {lines:#?}"
        );
        assert!(
            lines.iter().any(|line| line.contains("server-level only")),
            "rendered lines: {lines:#?}"
        );
    }

    #[test]
    fn new_session_overlay_renders_disabled_selected_mcp_server() {
        let mut form = NewSessionForm::new();
        form.runner = Tool::Codex;
        form.mcp_expanded = true;
        form.mcp_selected_row = 1;
        form.set_mcp_servers_for_test(vec!["GitLabMITRE".into(), "browser".into()]);
        form.mcp_selection = McpSelection {
            profile_id: None,
            servers: vec![
                McpServerSelection {
                    id: "GitLabMITRE".into(),
                    enabled: true,
                    selected_tools: None,
                },
                McpServerSelection {
                    id: "browser".into(),
                    enabled: false,
                    selected_tools: None,
                },
            ],
        };

        let lines = render_new_session_lines_for_test(&form);

        assert!(
            lines.iter().any(|line| line.contains("[ ] browser")),
            "rendered lines: {lines:#?}"
        );
    }

    #[test]
    fn new_session_overlay_renders_omitted_known_mcp_server_as_enabled() {
        let mut form = NewSessionForm::new();
        form.runner = Tool::Codex;
        form.mcp_expanded = true;
        form.set_mcp_servers_for_test(vec!["GitLabMITRE".into(), "browser".into()]);
        form.mcp_selection = McpSelection {
            profile_id: None,
            servers: vec![McpServerSelection {
                id: "GitLabMITRE".into(),
                enabled: false,
                selected_tools: None,
            }],
        };

        let lines = render_new_session_lines_for_test(&form);

        assert!(
            lines.iter().any(|line| line.contains("[ ] GitLabMITRE")),
            "rendered lines: {lines:#?}"
        );
        assert!(
            lines.iter().any(|line| line.contains("[x] browser")),
            "rendered lines: {lines:#?}"
        );
    }
}
