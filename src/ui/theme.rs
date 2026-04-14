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

    /// Tokyo Night
    pub fn tokyo_night() -> Theme {
        Theme {
            primary: Color::Rgb(122, 162, 247),         // #7aa2f7
            secondary: Color::Rgb(125, 207, 255),       // #7dcfff
            accent: Color::Rgb(187, 154, 247),          // #bb9af7
            error: Color::Rgb(247, 118, 142),           // #f7768e
            warning: Color::Rgb(224, 175, 104),         // #e0af68
            success: Color::Rgb(158, 206, 106),         // #9ece6a
            info: Color::Rgb(42, 195, 222),             // #2ac3de
            text: Color::Rgb(169, 177, 214),            // #a9b1d6
            text_muted: Color::Rgb(86, 95, 137),        // #565f89
            selected_item_text: Color::Rgb(26, 27, 38), // #1a1b26
            background: Color::Rgb(26, 27, 38),         // #1a1b26
            background_panel: Color::Rgb(36, 40, 59),   // #24283b
            background_element: Color::Rgb(55, 62, 98), // #373e62
            border: Color::Rgb(55, 62, 98),             // #373e62
            border_active: Color::Rgb(122, 162, 247),   // #7aa2f7
            border_subtle: Color::Rgb(36, 40, 59),      // #24283b
        }
    }

    /// Dracula
    pub fn dracula() -> Theme {
        Theme {
            primary: Color::Rgb(189, 147, 249),         // #bd93f9
            secondary: Color::Rgb(139, 233, 253),       // #8be9fd
            accent: Color::Rgb(255, 121, 198),          // #ff79c6
            error: Color::Rgb(255, 85, 85),             // #ff5555
            warning: Color::Rgb(241, 250, 140),         // #f1fa8c
            success: Color::Rgb(80, 250, 123),          // #50fa7b
            info: Color::Rgb(139, 233, 253),            // #8be9fd
            text: Color::Rgb(248, 248, 242),            // #f8f8f2
            text_muted: Color::Rgb(98, 114, 164),       // #6272a4
            selected_item_text: Color::Rgb(40, 42, 54), // #282a36
            background: Color::Rgb(40, 42, 54),         // #282a36
            background_panel: Color::Rgb(68, 71, 90),   // #44475a
            background_element: Color::Rgb(68, 71, 90), // #44475a
            border: Color::Rgb(68, 71, 90),             // #44475a
            border_active: Color::Rgb(189, 147, 249),   // #bd93f9
            border_subtle: Color::Rgb(68, 71, 90),      // #44475a
        }
    }

    /// Gruvbox Dark
    pub fn gruvbox() -> Theme {
        Theme {
            primary: Color::Rgb(215, 153, 33),          // #d79921
            secondary: Color::Rgb(69, 133, 136),        // #458588
            accent: Color::Rgb(177, 98, 134),           // #b16286
            error: Color::Rgb(204, 36, 29),             // #cc241d
            warning: Color::Rgb(254, 128, 25),          // #fe8019
            success: Color::Rgb(152, 151, 26),          // #98971a
            info: Color::Rgb(131, 165, 152),            // #83a598
            text: Color::Rgb(235, 219, 178),            // #ebdbb2
            text_muted: Color::Rgb(146, 131, 116),      // #928374
            selected_item_text: Color::Rgb(40, 40, 40), // #282828
            background: Color::Rgb(40, 40, 40),         // #282828
            background_panel: Color::Rgb(60, 56, 54),   // #3c3836
            background_element: Color::Rgb(80, 73, 69), // #504945
            border: Color::Rgb(80, 73, 69),             // #504945
            border_active: Color::Rgb(215, 153, 33),    // #d79921
            border_subtle: Color::Rgb(60, 56, 54),      // #3c3836
        }
    }

    /// Nord
    pub fn nord() -> Theme {
        Theme {
            primary: Color::Rgb(136, 192, 208),         // #88c0d0
            secondary: Color::Rgb(129, 161, 193),       // #81a1c1
            accent: Color::Rgb(180, 142, 173),          // #b48ead
            error: Color::Rgb(191, 97, 106),            // #bf616a
            warning: Color::Rgb(235, 203, 139),         // #ebcb8b
            success: Color::Rgb(163, 190, 140),         // #a3be8c
            info: Color::Rgb(143, 188, 187),            // #8fbcbb
            text: Color::Rgb(236, 239, 244),            // #eceff4
            text_muted: Color::Rgb(76, 86, 106),        // #4c566a
            selected_item_text: Color::Rgb(46, 52, 64), // #2e3440
            background: Color::Rgb(46, 52, 64),         // #2e3440
            background_panel: Color::Rgb(59, 66, 82),   // #3b4252
            background_element: Color::Rgb(67, 76, 94), // #434c5e
            border: Color::Rgb(67, 76, 94),             // #434c5e
            border_active: Color::Rgb(136, 192, 208),   // #88c0d0
            border_subtle: Color::Rgb(59, 66, 82),      // #3b4252
        }
    }

    /// Solarized Dark
    pub fn solarized() -> Theme {
        Theme {
            primary: Color::Rgb(38, 139, 210),         // #268bd2
            secondary: Color::Rgb(42, 161, 152),       // #2aa198
            accent: Color::Rgb(211, 54, 130),          // #d33682
            error: Color::Rgb(220, 50, 47),            // #dc322f
            warning: Color::Rgb(181, 137, 0),          // #b58900
            success: Color::Rgb(133, 153, 0),          // #859900
            info: Color::Rgb(42, 161, 152),            // #2aa198
            text: Color::Rgb(131, 148, 150),           // #839496
            text_muted: Color::Rgb(88, 110, 117),      // #586e75
            selected_item_text: Color::Rgb(0, 43, 54), // #002b36
            background: Color::Rgb(0, 43, 54),         // #002b36
            background_panel: Color::Rgb(7, 54, 66),   // #073642
            background_element: Color::Rgb(7, 54, 66), // #073642
            border: Color::Rgb(88, 110, 117),          // #586e75
            border_active: Color::Rgb(38, 139, 210),   // #268bd2
            border_subtle: Color::Rgb(7, 54, 66),      // #073642
        }
    }

    /// Rosé Pine
    pub fn rose_pine() -> Theme {
        Theme {
            primary: Color::Rgb(235, 188, 186),         // #ebbcba  rose
            secondary: Color::Rgb(49, 116, 143),        // #31748f  pine
            accent: Color::Rgb(196, 167, 231),          // #c4a7e7  iris
            error: Color::Rgb(235, 111, 146),           // #eb6f92  love
            warning: Color::Rgb(246, 193, 119),         // #f6c177  gold
            success: Color::Rgb(49, 116, 143),          // #31748f  pine
            info: Color::Rgb(156, 207, 216),            // #9ccfd8  foam
            text: Color::Rgb(224, 222, 244),            // #e0def4
            text_muted: Color::Rgb(110, 106, 134),      // #6e6a86  muted
            selected_item_text: Color::Rgb(25, 23, 36), // #191724  base
            background: Color::Rgb(25, 23, 36),         // #191724  base
            background_panel: Color::Rgb(33, 32, 46),   // #21202e  surface
            background_element: Color::Rgb(38, 35, 58), // #26233a  overlay
            border: Color::Rgb(38, 35, 58),             // #26233a
            border_active: Color::Rgb(235, 188, 186),   // #ebbcba
            border_subtle: Color::Rgb(33, 32, 46),      // #21202e
        }
    }

    /// Kanagawa
    pub fn kanagawa() -> Theme {
        Theme {
            primary: Color::Rgb(220, 165, 97),     // #dca561  autumn yellow
            secondary: Color::Rgb(126, 156, 216),  // #7e9cd8  crystal blue
            accent: Color::Rgb(149, 127, 184),     // #957fb8  oni violet
            error: Color::Rgb(195, 64, 67),        // #c34043  autumn red
            warning: Color::Rgb(255, 160, 102),    // #ffa066  surimi orange
            success: Color::Rgb(118, 148, 106),    // #76946a  autumn green
            info: Color::Rgb(127, 180, 202),       // #7fb4ca  spring blue
            text: Color::Rgb(220, 215, 186),       // #dcd7ba  fuji white
            text_muted: Color::Rgb(114, 113, 105), // #727169  fuji grey
            selected_item_text: Color::Rgb(22, 22, 29), // #16161d  sumi ink
            background: Color::Rgb(22, 22, 29),    // #16161d  sumi ink
            background_panel: Color::Rgb(30, 30, 41), // #1e1e29
            background_element: Color::Rgb(54, 54, 70), // #363646
            border: Color::Rgb(54, 54, 70),        // #363646
            border_active: Color::Rgb(220, 165, 97), // #dca561
            border_subtle: Color::Rgb(30, 30, 41), // #1e1e29
        }
    }

    /// Everforest Dark
    pub fn everforest() -> Theme {
        Theme {
            primary: Color::Rgb(163, 190, 140),         // #a3be8c  green
            secondary: Color::Rgb(125, 174, 163),       // #7daea3  blue
            accent: Color::Rgb(214, 153, 182),          // #d699b6  purple
            error: Color::Rgb(230, 126, 128),           // #e67e80  red
            warning: Color::Rgb(219, 188, 127),         // #dbbc7f  yellow
            success: Color::Rgb(163, 190, 140),         // #a3be8c  green
            info: Color::Rgb(125, 174, 163),            // #7daea3  aqua
            text: Color::Rgb(211, 198, 170),            // #d3c6aa  fg
            text_muted: Color::Rgb(127, 132, 120),      // #7f8478  grey1
            selected_item_text: Color::Rgb(39, 46, 34), // #272e22  bg_dim
            background: Color::Rgb(45, 53, 38),         // #2d3526  bg0
            background_panel: Color::Rgb(52, 60, 44),   // #343c2c  bg1
            background_element: Color::Rgb(62, 71, 53), // #3e4735  bg3
            border: Color::Rgb(62, 71, 53),             // #3e4735
            border_active: Color::Rgb(163, 190, 140),   // #a3be8c
            border_subtle: Color::Rgb(52, 60, 44),      // #343c2c
        }
    }

    /// One Dark (Atom)
    pub fn one_dark() -> Theme {
        Theme {
            primary: Color::Rgb(97, 175, 239),          // #61afef  blue
            secondary: Color::Rgb(86, 182, 194),        // #56b6c2  cyan
            accent: Color::Rgb(198, 120, 221),          // #c678dd  magenta
            error: Color::Rgb(224, 108, 117),           // #e06c75  red
            warning: Color::Rgb(229, 192, 123),         // #e5c07b  yellow
            success: Color::Rgb(152, 195, 121),         // #98c379  green
            info: Color::Rgb(86, 182, 194),             // #56b6c2  cyan
            text: Color::Rgb(171, 178, 191),            // #abb2bf  fg
            text_muted: Color::Rgb(92, 99, 112),        // #5c6370  comment
            selected_item_text: Color::Rgb(40, 44, 52), // #282c34  bg
            background: Color::Rgb(40, 44, 52),         // #282c34  bg
            background_panel: Color::Rgb(50, 56, 66),   // #323842
            background_element: Color::Rgb(62, 68, 81), // #3e4451
            border: Color::Rgb(62, 68, 81),             // #3e4451
            border_active: Color::Rgb(97, 175, 239),    // #61afef
            border_subtle: Color::Rgb(50, 56, 66),      // #323842
        }
    }

    /// Moonfly
    pub fn moonfly() -> Theme {
        Theme {
            primary: Color::Rgb(128, 170, 255),         // #80aaff  blue
            secondary: Color::Rgb(120, 220, 232),       // #78dce8  turquoise
            accent: Color::Rgb(207, 135, 224),          // #cf87e0  violet
            error: Color::Rgb(255, 95, 135),            // #ff5f87  cranberry
            warning: Color::Rgb(228, 191, 128),         // #e4bf80  khaki
            success: Color::Rgb(138, 206, 104),         // #8ace68  emerald
            info: Color::Rgb(120, 220, 232),            // #78dce8  turquoise
            text: Color::Rgb(189, 189, 189),            // #bdbdbd  white
            text_muted: Color::Rgb(99, 99, 99),         // #636363  grey
            selected_item_text: Color::Rgb(8, 8, 8),    // #080808  bg
            background: Color::Rgb(8, 8, 8),            // #080808  bg
            background_panel: Color::Rgb(28, 28, 28),   // #1c1c1c
            background_element: Color::Rgb(48, 48, 48), // #303030
            border: Color::Rgb(48, 48, 48),             // #303030
            border_active: Color::Rgb(128, 170, 255),   // #80aaff
            border_subtle: Color::Rgb(28, 28, 28),      // #1c1c1c
        }
    }

    /// Look up a theme by name
    pub fn from_name(name: &str) -> Theme {
        match name {
            "light" => Theme::light(),
            "tokyo-night" => Theme::tokyo_night(),
            "dracula" => Theme::dracula(),
            "gruvbox" => Theme::gruvbox(),
            "nord" => Theme::nord(),
            "solarized" => Theme::solarized(),
            "rose-pine" => Theme::rose_pine(),
            "kanagawa" => Theme::kanagawa(),
            "everforest" => Theme::everforest(),
            "one-dark" => Theme::one_dark(),
            "moonfly" => Theme::moonfly(),
            _ => Theme::dark(),
        }
    }

    /// List all available theme names
    pub fn available() -> Vec<&'static str> {
        vec![
            "dark",
            "light",
            "tokyo-night",
            "dracula",
            "gruvbox",
            "nord",
            "solarized",
            "rose-pine",
            "kanagawa",
            "everforest",
            "one-dark",
            "moonfly",
        ]
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
