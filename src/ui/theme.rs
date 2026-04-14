//! Dark/light theme definitions — Catppuccin Mocha (dark) and Latte (light)

use ratatui::style::Color;

#[derive(Debug, Clone)]
pub struct Theme {
    pub primary: Color,
    pub secondary: Color,
    pub accent: Color,
    pub error: Color,
    pub warning: Color,
    pub success: Color,
    pub info: Color,
    pub text: Color,
    pub text_muted: Color,
    pub selected_item_text: Color,
    pub background: Color,
    #[allow(dead_code)]
    pub background_panel: Color,
    pub background_element: Color,
    pub border: Color,
    pub border_active: Color,
    #[allow(dead_code)]
    pub border_subtle: Color,
}

impl Theme {
    /// Catppuccin Mocha (dark theme)
    pub fn dark() -> Theme {
        Theme {
            primary: Color::Rgb(203, 166, 247),         // #cba6f7
            secondary: Color::Rgb(137, 180, 250),       // #89b4fa
            accent: Color::Rgb(245, 194, 231),          // #f5c2e7
            error: Color::Rgb(243, 139, 168),           // #f38ba8
            warning: Color::Rgb(250, 179, 135),         // #fab387
            success: Color::Rgb(166, 227, 161),         // #a6e3a1
            info: Color::Rgb(116, 199, 236),            // #74c7ec
            text: Color::Rgb(205, 214, 244),            // #cdd6f4
            text_muted: Color::Rgb(108, 112, 134),      // #6c7086
            selected_item_text: Color::Rgb(30, 30, 46), // #1e1e2e
            background: Color::Rgb(30, 30, 46),         // #1e1e2e
            background_panel: Color::Rgb(49, 50, 68),   // #313244
            background_element: Color::Rgb(69, 71, 90), // #45475a
            border: Color::Rgb(69, 71, 90),             // #45475a
            border_active: Color::Rgb(203, 166, 247),   // #cba6f7
            border_subtle: Color::Rgb(49, 50, 68),      // #313244
        }
    }

    /// Catppuccin Latte (light theme)
    pub fn light() -> Theme {
        Theme {
            primary: Color::Rgb(136, 57, 239),             // #8839ef
            secondary: Color::Rgb(30, 102, 245),           // #1e66f5
            accent: Color::Rgb(234, 118, 203),             // #ea76cb
            error: Color::Rgb(210, 15, 57),                // #d20f39
            warning: Color::Rgb(254, 100, 11),             // #fe640b
            success: Color::Rgb(64, 160, 43),              // #40a02b
            info: Color::Rgb(4, 165, 229),                 // #04a5e5
            text: Color::Rgb(76, 79, 105),                 // #4c4f69
            text_muted: Color::Rgb(156, 160, 176),         // #9ca0b0
            selected_item_text: Color::Rgb(239, 241, 245), // #eff1f5
            background: Color::Rgb(239, 241, 245),         // #eff1f5
            background_panel: Color::Rgb(230, 233, 239),   // #e6e9ef
            background_element: Color::Rgb(204, 208, 218), // #ccd0da
            border: Color::Rgb(204, 208, 218),             // #ccd0da
            border_active: Color::Rgb(136, 57, 239),       // #8839ef
            border_subtle: Color::Rgb(230, 233, 239),      // #e6e9ef
        }
    }
}

/// Get status color from the theme
pub fn status_color(theme: &Theme, status: crate::types::SessionStatus) -> Color {
    match status {
        crate::types::SessionStatus::Running => theme.success,
        crate::types::SessionStatus::Waiting => theme.warning,
        crate::types::SessionStatus::Paused => theme.secondary,
        crate::types::SessionStatus::Compacting => theme.accent,
        crate::types::SessionStatus::Idle => theme.text_muted,
        crate::types::SessionStatus::Error => theme.error,
        crate::types::SessionStatus::Stopped => theme.text_muted,
    }
}
