use std::fmt;
use std::str::FromStr;
use std::sync::RwLock;

use ratatui::style::{Color, Modifier, Style};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// ThemeName
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeName {
    Default,
    CatppuccinMocha,
    CatppuccinLatte,
    Nord,
    NordLight,
    SolarizedDark,
    SolarizedLight,
    TokyoNight,
    TokyoNightLight,
    GitHubDark,
    GitHubLight,
    GruvboxDark,
    GruvboxLight,
    Dracula,
    DraculaLight,
    RosePine,
    RosePineDawn,
}

impl fmt::Display for ThemeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

impl ThemeName {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::CatppuccinMocha => "Catppuccin Mocha",
            Self::CatppuccinLatte => "Catppuccin Latte",
            Self::Nord => "Nord",
            Self::NordLight => "Nord Light",
            Self::SolarizedDark => "Solarized Dark",
            Self::SolarizedLight => "Solarized Light",
            Self::TokyoNight => "Tokyo Night",
            Self::TokyoNightLight => "Tokyo Night Light",
            Self::GitHubDark => "GitHub Dark",
            Self::GitHubLight => "GitHub Light",
            Self::GruvboxDark => "Gruvbox Dark",
            Self::GruvboxLight => "Gruvbox Light",
            Self::Dracula => "Dracula",
            Self::DraculaLight => "Dracula Light",
            Self::RosePine => "Rosé Pine",
            Self::RosePineDawn => "Rosé Pine Dawn",
        }
    }
}

impl FromStr for ThemeName {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Try serde (enum variant name) first, then display name.
        serde_json::from_str::<ThemeName>(&format!("\"{s}\"")).or_else(|_| {
            ALL_THEMES
                .iter()
                .find(|t| t.display_name().eq_ignore_ascii_case(s))
                .copied()
                .ok_or(())
        })
    }
}

// ---------------------------------------------------------------------------
// Dark / Light theme lists for cycling
// ---------------------------------------------------------------------------

const ALL_THEMES: &[ThemeName] = &[
    ThemeName::Default,
    ThemeName::CatppuccinMocha,
    ThemeName::Nord,
    ThemeName::SolarizedDark,
    ThemeName::TokyoNight,
    ThemeName::GitHubDark,
    ThemeName::GruvboxDark,
    ThemeName::Dracula,
    ThemeName::RosePine,
    ThemeName::CatppuccinLatte,
    ThemeName::NordLight,
    ThemeName::SolarizedLight,
    ThemeName::TokyoNightLight,
    ThemeName::GitHubLight,
    ThemeName::GruvboxLight,
    ThemeName::DraculaLight,
    ThemeName::RosePineDawn,
];

// ---------------------------------------------------------------------------
// ThemePalette
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct ThemePalette {
    pub title_color: Color,
    pub title_modifier: Modifier,
    pub selected_fg: Color,
    pub selected_bg: Color,
    pub status_available: Color,
    pub status_unavailable: Color,
    pub incoming_message: Color,
    pub outgoing_message: Color,
    pub timestamp: Color,
    pub help: Color,
    pub active_border: Color,
    pub inactive_border: Color,
    pub search_highlight_fg: Color,
    pub search_highlight_bg: Color,
    pub search_highlight_sel_fg: Color,
    pub search_highlight_sel_bg: Color,
    pub bg: Color,
    pub fg: Color,
}

// ---------------------------------------------------------------------------
// Default palette (uses named colors for max terminal compat)
// ---------------------------------------------------------------------------

const DEFAULT_PALETTE: ThemePalette = ThemePalette {
    title_color: Color::Cyan,
    title_modifier: Modifier::BOLD,
    selected_fg: Color::White,
    selected_bg: Color::DarkGray,
    status_available: Color::Green,
    status_unavailable: Color::DarkGray,
    incoming_message: Color::White,
    outgoing_message: Color::Cyan,
    timestamp: Color::DarkGray,
    help: Color::DarkGray,
    active_border: Color::Cyan,
    inactive_border: Color::DarkGray,
    search_highlight_fg: Color::Black,
    search_highlight_bg: Color::Yellow,
    search_highlight_sel_fg: Color::Yellow,
    search_highlight_sel_bg: Color::DarkGray,
    bg: Color::Reset,
    fg: Color::Reset,
};

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

static CURRENT_THEME: RwLock<(ThemeName, ThemePalette)> =
    RwLock::new((ThemeName::Default, DEFAULT_PALETTE));

pub fn set_theme(name: ThemeName) {
    let palette = palette_for(name);
    *CURRENT_THEME.write().unwrap() = (name, palette);
}

pub fn current_theme_name() -> ThemeName {
    CURRENT_THEME.read().unwrap().0
}

fn current() -> ThemePalette {
    CURRENT_THEME.read().unwrap().1
}

// ---------------------------------------------------------------------------
// Cycling
// ---------------------------------------------------------------------------

pub fn cycle_all(current_name: ThemeName) -> ThemeName {
    cycle_in_list(ALL_THEMES, current_name)
}

fn cycle_in_list(list: &[ThemeName], current_name: ThemeName) -> ThemeName {
    if let Some(pos) = list.iter().position(|&t| t == current_name) {
        list[(pos + 1) % list.len()]
    } else {
        list[0]
    }
}

// ---------------------------------------------------------------------------
// Style functions (signatures unchanged — reads from global)
// ---------------------------------------------------------------------------

pub fn title_style() -> Style {
    let p = current();
    Style::default().fg(p.title_color).add_modifier(p.title_modifier)
}

pub fn selected_style() -> Style {
    let p = current();
    Style::default().bg(p.selected_bg).fg(p.selected_fg)
}

pub fn status_available() -> Style {
    Style::default().fg(current().status_available)
}

pub fn status_unavailable() -> Style {
    Style::default().fg(current().status_unavailable)
}

pub fn incoming_message() -> Style {
    Style::default().fg(current().incoming_message)
}

pub fn outgoing_message() -> Style {
    Style::default().fg(current().outgoing_message)
}

pub fn timestamp_style() -> Style {
    Style::default().fg(current().timestamp)
}

pub fn help_style() -> Style {
    Style::default().fg(current().help)
}

pub fn active_border() -> Style {
    Style::default().fg(current().active_border)
}

pub fn inactive_border() -> Style {
    Style::default().fg(current().inactive_border)
}

pub fn search_highlight() -> Style {
    let p = current();
    Style::default().fg(p.search_highlight_fg).bg(p.search_highlight_bg)
}

pub fn search_highlight_selected() -> Style {
    let p = current();
    Style::default()
        .fg(p.search_highlight_sel_fg)
        .bg(p.search_highlight_sel_bg)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
}

pub fn background() -> Color {
    current().bg
}

pub fn foreground() -> Color {
    current().fg
}

// ---------------------------------------------------------------------------
// Palette definitions per theme
// ---------------------------------------------------------------------------

fn palette_for(name: ThemeName) -> ThemePalette {
    match name {
        ThemeName::Default => DEFAULT_PALETTE,

        // ----- Catppuccin Mocha -----
        ThemeName::CatppuccinMocha => ThemePalette {
            title_color: Color::Rgb(137, 180, 250),       // Blue
            title_modifier: Modifier::BOLD,
            selected_fg: Color::Rgb(205, 214, 244),        // Text
            selected_bg: Color::Rgb(88, 91, 112),          // Surface2
            status_available: Color::Rgb(166, 227, 161),   // Green
            status_unavailable: Color::Rgb(108, 112, 134), // Overlay0
            incoming_message: Color::Rgb(205, 214, 244),   // Text
            outgoing_message: Color::Rgb(137, 180, 250),   // Blue
            timestamp: Color::Rgb(108, 112, 134),          // Overlay0
            help: Color::Rgb(108, 112, 134),               // Overlay0
            active_border: Color::Rgb(137, 180, 250),      // Blue
            inactive_border: Color::Rgb(69, 71, 90),       // Surface1
            search_highlight_fg: Color::Rgb(30, 30, 46),   // Base
            search_highlight_bg: Color::Rgb(249, 226, 175),// Yellow
            search_highlight_sel_fg: Color::Rgb(249, 226, 175),
            search_highlight_sel_bg: Color::Rgb(88, 91, 112),
            bg: Color::Rgb(30, 30, 46),                    // Base
            fg: Color::Rgb(205, 214, 244),                 // Text
        },

        // ----- Catppuccin Latte -----
        ThemeName::CatppuccinLatte => ThemePalette {
            title_color: Color::Rgb(30, 102, 245),         // Blue
            title_modifier: Modifier::BOLD,
            selected_fg: Color::Rgb(76, 79, 105),          // Text
            selected_bg: Color::Rgb(188, 192, 204),        // Surface2
            status_available: Color::Rgb(64, 160, 43),     // Green
            status_unavailable: Color::Rgb(140, 143, 161), // Overlay0
            incoming_message: Color::Rgb(76, 79, 105),     // Text
            outgoing_message: Color::Rgb(30, 102, 245),    // Blue
            timestamp: Color::Rgb(140, 143, 161),          // Overlay0
            help: Color::Rgb(140, 143, 161),               // Overlay0
            active_border: Color::Rgb(30, 102, 245),       // Blue
            inactive_border: Color::Rgb(172, 176, 190),    // Surface1
            search_highlight_fg: Color::Rgb(239, 241, 245),// Base
            search_highlight_bg: Color::Rgb(223, 142, 29), // Yellow
            search_highlight_sel_fg: Color::Rgb(223, 142, 29),
            search_highlight_sel_bg: Color::Rgb(188, 192, 204),
            bg: Color::Rgb(239, 241, 245),                 // Base
            fg: Color::Rgb(76, 79, 105),                   // Text
        },

        // ----- Nord -----
        ThemeName::Nord => ThemePalette {
            title_color: Color::Rgb(136, 192, 208),        // Nord8 (frost cyan)
            title_modifier: Modifier::BOLD,
            selected_fg: Color::Rgb(236, 239, 244),        // Nord6 (snow)
            selected_bg: Color::Rgb(76, 86, 106),          // Nord3
            status_available: Color::Rgb(163, 190, 140),   // Nord14 (green)
            status_unavailable: Color::Rgb(76, 86, 106),   // Nord3
            incoming_message: Color::Rgb(236, 239, 244),   // Nord6
            outgoing_message: Color::Rgb(136, 192, 208),   // Nord8
            timestamp: Color::Rgb(76, 86, 106),            // Nord3
            help: Color::Rgb(76, 86, 106),                 // Nord3
            active_border: Color::Rgb(136, 192, 208),      // Nord8
            inactive_border: Color::Rgb(59, 66, 82),       // Nord2
            search_highlight_fg: Color::Rgb(46, 52, 64),   // Nord0
            search_highlight_bg: Color::Rgb(235, 203, 139),// Nord13 (yellow)
            search_highlight_sel_fg: Color::Rgb(235, 203, 139),
            search_highlight_sel_bg: Color::Rgb(76, 86, 106),
            bg: Color::Rgb(46, 52, 64),                    // Nord0
            fg: Color::Rgb(236, 239, 244),                 // Nord6
        },

        // ----- Nord Light -----
        ThemeName::NordLight => ThemePalette {
            title_color: Color::Rgb(94, 129, 172),         // Nord10 (blue)
            title_modifier: Modifier::BOLD,
            selected_fg: Color::Rgb(46, 52, 64),           // Nord0
            selected_bg: Color::Rgb(216, 222, 233),        // Nord4
            status_available: Color::Rgb(163, 190, 140),   // Nord14
            status_unavailable: Color::Rgb(147, 161, 182), // muted
            incoming_message: Color::Rgb(46, 52, 64),      // Nord0
            outgoing_message: Color::Rgb(94, 129, 172),    // Nord10
            timestamp: Color::Rgb(147, 161, 182),
            help: Color::Rgb(147, 161, 182),
            active_border: Color::Rgb(94, 129, 172),       // Nord10
            inactive_border: Color::Rgb(216, 222, 233),    // Nord4
            search_highlight_fg: Color::Rgb(236, 239, 244),// Nord6
            search_highlight_bg: Color::Rgb(208, 135, 112),// Nord12 (orange)
            search_highlight_sel_fg: Color::Rgb(208, 135, 112),
            search_highlight_sel_bg: Color::Rgb(216, 222, 233),
            bg: Color::Rgb(236, 239, 244),                 // Nord6
            fg: Color::Rgb(46, 52, 64),                    // Nord0
        },

        // ----- Solarized Dark -----
        ThemeName::SolarizedDark => ThemePalette {
            title_color: Color::Rgb(38, 139, 210),         // blue
            title_modifier: Modifier::BOLD,
            selected_fg: Color::Rgb(253, 246, 227),        // base3
            selected_bg: Color::Rgb(7, 54, 66),            // base02
            status_available: Color::Rgb(133, 153, 0),     // green
            status_unavailable: Color::Rgb(88, 110, 117),  // base01
            incoming_message: Color::Rgb(147, 161, 161),   // base1
            outgoing_message: Color::Rgb(38, 139, 210),    // blue
            timestamp: Color::Rgb(88, 110, 117),           // base01
            help: Color::Rgb(88, 110, 117),                // base01
            active_border: Color::Rgb(38, 139, 210),       // blue
            inactive_border: Color::Rgb(7, 54, 66),        // base02
            search_highlight_fg: Color::Rgb(0, 43, 54),    // base03
            search_highlight_bg: Color::Rgb(181, 137, 0),  // yellow
            search_highlight_sel_fg: Color::Rgb(181, 137, 0),
            search_highlight_sel_bg: Color::Rgb(7, 54, 66),
            bg: Color::Rgb(0, 43, 54),                     // base03
            fg: Color::Rgb(147, 161, 161),                 // base1
        },

        // ----- Solarized Light -----
        ThemeName::SolarizedLight => ThemePalette {
            title_color: Color::Rgb(38, 139, 210),         // blue
            title_modifier: Modifier::BOLD,
            selected_fg: Color::Rgb(0, 43, 54),            // base03
            selected_bg: Color::Rgb(238, 232, 213),        // base2
            status_available: Color::Rgb(133, 153, 0),     // green
            status_unavailable: Color::Rgb(147, 161, 161), // base1
            incoming_message: Color::Rgb(101, 123, 131),   // base00
            outgoing_message: Color::Rgb(38, 139, 210),    // blue
            timestamp: Color::Rgb(147, 161, 161),          // base1
            help: Color::Rgb(147, 161, 161),               // base1
            active_border: Color::Rgb(38, 139, 210),       // blue
            inactive_border: Color::Rgb(238, 232, 213),    // base2
            search_highlight_fg: Color::Rgb(253, 246, 227),// base3
            search_highlight_bg: Color::Rgb(181, 137, 0),  // yellow
            search_highlight_sel_fg: Color::Rgb(181, 137, 0),
            search_highlight_sel_bg: Color::Rgb(238, 232, 213),
            bg: Color::Rgb(253, 246, 227),                 // base3
            fg: Color::Rgb(101, 123, 131),                 // base00
        },

        // ----- Tokyo Night -----
        ThemeName::TokyoNight => ThemePalette {
            title_color: Color::Rgb(122, 162, 247),        // blue
            title_modifier: Modifier::BOLD,
            selected_fg: Color::Rgb(192, 202, 245),        // fg
            selected_bg: Color::Rgb(52, 59, 88),           // bg_highlight
            status_available: Color::Rgb(158, 206, 106),   // green
            status_unavailable: Color::Rgb(68, 75, 106),   // comment
            incoming_message: Color::Rgb(192, 202, 245),   // fg
            outgoing_message: Color::Rgb(122, 162, 247),   // blue
            timestamp: Color::Rgb(68, 75, 106),            // comment
            help: Color::Rgb(68, 75, 106),                 // comment
            active_border: Color::Rgb(122, 162, 247),      // blue
            inactive_border: Color::Rgb(41, 46, 66),       // bg_dark
            search_highlight_fg: Color::Rgb(26, 27, 38),   // bg
            search_highlight_bg: Color::Rgb(224, 175, 104),// yellow
            search_highlight_sel_fg: Color::Rgb(224, 175, 104),
            search_highlight_sel_bg: Color::Rgb(52, 59, 88),
            bg: Color::Rgb(26, 27, 38),                    // bg
            fg: Color::Rgb(192, 202, 245),                 // fg
        },

        // ----- Tokyo Night Light -----
        ThemeName::TokyoNightLight => ThemePalette {
            title_color: Color::Rgb(52, 84, 138),          // blue
            title_modifier: Modifier::BOLD,
            selected_fg: Color::Rgb(56, 62, 87),           // fg
            selected_bg: Color::Rgb(212, 214, 226),        // bg_highlight
            status_available: Color::Rgb(72, 131, 56),     // green
            status_unavailable: Color::Rgb(144, 148, 171), // comment
            incoming_message: Color::Rgb(56, 62, 87),      // fg
            outgoing_message: Color::Rgb(52, 84, 138),     // blue
            timestamp: Color::Rgb(144, 148, 171),          // comment
            help: Color::Rgb(144, 148, 171),               // comment
            active_border: Color::Rgb(52, 84, 138),        // blue
            inactive_border: Color::Rgb(212, 214, 226),    // bg_highlight
            search_highlight_fg: Color::Rgb(213, 214, 219),// bg
            search_highlight_bg: Color::Rgb(142, 109, 37), // yellow
            search_highlight_sel_fg: Color::Rgb(142, 109, 37),
            search_highlight_sel_bg: Color::Rgb(212, 214, 226),
            bg: Color::Rgb(213, 214, 219),                 // bg
            fg: Color::Rgb(56, 62, 87),                    // fg
        },

        // ----- GitHub Dark -----
        ThemeName::GitHubDark => ThemePalette {
            title_color: Color::Rgb(88, 166, 255),         // blue
            title_modifier: Modifier::BOLD,
            selected_fg: Color::Rgb(230, 237, 243),        // fg
            selected_bg: Color::Rgb(48, 54, 61),           // highlight
            status_available: Color::Rgb(63, 185, 80),     // green
            status_unavailable: Color::Rgb(110, 118, 129), // muted
            incoming_message: Color::Rgb(230, 237, 243),   // fg
            outgoing_message: Color::Rgb(88, 166, 255),    // blue
            timestamp: Color::Rgb(110, 118, 129),          // muted
            help: Color::Rgb(110, 118, 129),               // muted
            active_border: Color::Rgb(88, 166, 255),       // blue
            inactive_border: Color::Rgb(48, 54, 61),       // border
            search_highlight_fg: Color::Rgb(13, 17, 23),   // bg
            search_highlight_bg: Color::Rgb(210, 153, 34), // yellow
            search_highlight_sel_fg: Color::Rgb(210, 153, 34),
            search_highlight_sel_bg: Color::Rgb(48, 54, 61),
            bg: Color::Rgb(13, 17, 23),                    // bg
            fg: Color::Rgb(230, 237, 243),                 // fg
        },

        // ----- GitHub Light -----
        ThemeName::GitHubLight => ThemePalette {
            title_color: Color::Rgb(9, 105, 218),          // blue
            title_modifier: Modifier::BOLD,
            selected_fg: Color::Rgb(31, 35, 40),           // fg
            selected_bg: Color::Rgb(218, 224, 231),        // highlight
            status_available: Color::Rgb(26, 127, 55),     // green
            status_unavailable: Color::Rgb(101, 109, 118), // muted
            incoming_message: Color::Rgb(31, 35, 40),      // fg
            outgoing_message: Color::Rgb(9, 105, 218),     // blue
            timestamp: Color::Rgb(101, 109, 118),          // muted
            help: Color::Rgb(101, 109, 118),               // muted
            active_border: Color::Rgb(9, 105, 218),        // blue
            inactive_border: Color::Rgb(218, 224, 231),    // border
            search_highlight_fg: Color::Rgb(255, 255, 255),// white
            search_highlight_bg: Color::Rgb(159, 106, 0),  // yellow
            search_highlight_sel_fg: Color::Rgb(159, 106, 0),
            search_highlight_sel_bg: Color::Rgb(218, 224, 231),
            bg: Color::Rgb(255, 255, 255),                 // white
            fg: Color::Rgb(31, 35, 40),                    // fg
        },

        // ----- Gruvbox Dark -----
        ThemeName::GruvboxDark => ThemePalette {
            title_color: Color::Rgb(131, 165, 152),        // aqua
            title_modifier: Modifier::BOLD,
            selected_fg: Color::Rgb(235, 219, 178),        // fg
            selected_bg: Color::Rgb(80, 73, 69),           // bg2
            status_available: Color::Rgb(184, 187, 38),    // green
            status_unavailable: Color::Rgb(146, 131, 116), // gray
            incoming_message: Color::Rgb(235, 219, 178),   // fg
            outgoing_message: Color::Rgb(131, 165, 152),   // aqua
            timestamp: Color::Rgb(146, 131, 116),          // gray
            help: Color::Rgb(146, 131, 116),               // gray
            active_border: Color::Rgb(131, 165, 152),      // aqua
            inactive_border: Color::Rgb(60, 56, 54),       // bg1
            search_highlight_fg: Color::Rgb(40, 40, 40),   // bg
            search_highlight_bg: Color::Rgb(250, 189, 47), // yellow
            search_highlight_sel_fg: Color::Rgb(250, 189, 47),
            search_highlight_sel_bg: Color::Rgb(80, 73, 69),
            bg: Color::Rgb(40, 40, 40),                    // bg0
            fg: Color::Rgb(235, 219, 178),                 // fg
        },

        // ----- Gruvbox Light -----
        ThemeName::GruvboxLight => ThemePalette {
            title_color: Color::Rgb(69, 133, 136),         // aqua
            title_modifier: Modifier::BOLD,
            selected_fg: Color::Rgb(60, 56, 54),           // fg
            selected_bg: Color::Rgb(213, 196, 161),        // bg2
            status_available: Color::Rgb(121, 116, 14),    // green
            status_unavailable: Color::Rgb(168, 153, 132), // gray
            incoming_message: Color::Rgb(60, 56, 54),      // fg
            outgoing_message: Color::Rgb(69, 133, 136),    // aqua
            timestamp: Color::Rgb(168, 153, 132),          // gray
            help: Color::Rgb(168, 153, 132),               // gray
            active_border: Color::Rgb(69, 133, 136),       // aqua
            inactive_border: Color::Rgb(213, 196, 161),    // bg2
            search_highlight_fg: Color::Rgb(251, 241, 199),// bg
            search_highlight_bg: Color::Rgb(181, 118, 20), // yellow
            search_highlight_sel_fg: Color::Rgb(181, 118, 20),
            search_highlight_sel_bg: Color::Rgb(213, 196, 161),
            bg: Color::Rgb(251, 241, 199),                 // bg0
            fg: Color::Rgb(60, 56, 54),                    // fg
        },

        // ----- Dracula -----
        ThemeName::Dracula => ThemePalette {
            title_color: Color::Rgb(139, 233, 253),        // cyan
            title_modifier: Modifier::BOLD,
            selected_fg: Color::Rgb(248, 248, 242),        // fg
            selected_bg: Color::Rgb(68, 71, 90),           // current line
            status_available: Color::Rgb(80, 250, 123),    // green
            status_unavailable: Color::Rgb(98, 114, 164),  // comment
            incoming_message: Color::Rgb(248, 248, 242),   // fg
            outgoing_message: Color::Rgb(139, 233, 253),   // cyan
            timestamp: Color::Rgb(98, 114, 164),           // comment
            help: Color::Rgb(98, 114, 164),                // comment
            active_border: Color::Rgb(139, 233, 253),      // cyan
            inactive_border: Color::Rgb(68, 71, 90),       // current line
            search_highlight_fg: Color::Rgb(40, 42, 54),   // bg
            search_highlight_bg: Color::Rgb(241, 250, 140),// yellow
            search_highlight_sel_fg: Color::Rgb(241, 250, 140),
            search_highlight_sel_bg: Color::Rgb(68, 71, 90),
            bg: Color::Rgb(40, 42, 54),                    // bg
            fg: Color::Rgb(248, 248, 242),                 // fg
        },

        // ----- Dracula Light (soft inversion) -----
        ThemeName::DraculaLight => ThemePalette {
            title_color: Color::Rgb(98, 114, 164),         // blue-purple
            title_modifier: Modifier::BOLD,
            selected_fg: Color::Rgb(40, 42, 54),           // dark fg
            selected_bg: Color::Rgb(220, 220, 232),        // light highlight
            status_available: Color::Rgb(28, 140, 60),     // green
            status_unavailable: Color::Rgb(160, 160, 180), // muted
            incoming_message: Color::Rgb(40, 42, 54),      // dark fg
            outgoing_message: Color::Rgb(98, 114, 164),    // blue-purple
            timestamp: Color::Rgb(160, 160, 180),          // muted
            help: Color::Rgb(160, 160, 180),               // muted
            active_border: Color::Rgb(98, 114, 164),       // blue-purple
            inactive_border: Color::Rgb(220, 220, 232),    // light border
            search_highlight_fg: Color::Rgb(248, 248, 242),// light bg
            search_highlight_bg: Color::Rgb(180, 130, 20), // yellow-dark
            search_highlight_sel_fg: Color::Rgb(180, 130, 20),
            search_highlight_sel_bg: Color::Rgb(220, 220, 232),
            bg: Color::Rgb(248, 248, 242),                 // inverted
            fg: Color::Rgb(40, 42, 54),                    // inverted
        },

        // ----- Rosé Pine -----
        ThemeName::RosePine => ThemePalette {
            title_color: Color::Rgb(156, 207, 216),        // foam
            title_modifier: Modifier::BOLD,
            selected_fg: Color::Rgb(224, 222, 244),        // text
            selected_bg: Color::Rgb(57, 53, 82),           // highlight med
            status_available: Color::Rgb(156, 207, 216),   // foam
            status_unavailable: Color::Rgb(110, 106, 134), // muted
            incoming_message: Color::Rgb(224, 222, 244),   // text
            outgoing_message: Color::Rgb(196, 167, 231),   // iris
            timestamp: Color::Rgb(110, 106, 134),          // muted
            help: Color::Rgb(110, 106, 134),               // muted
            active_border: Color::Rgb(196, 167, 231),      // iris
            inactive_border: Color::Rgb(38, 35, 58),       // highlight low
            search_highlight_fg: Color::Rgb(25, 23, 36),   // base
            search_highlight_bg: Color::Rgb(246, 193, 119),// gold
            search_highlight_sel_fg: Color::Rgb(246, 193, 119),
            search_highlight_sel_bg: Color::Rgb(57, 53, 82),
            bg: Color::Rgb(25, 23, 36),                    // base
            fg: Color::Rgb(224, 222, 244),                 // text
        },

        // ----- Rosé Pine Dawn -----
        ThemeName::RosePineDawn => ThemePalette {
            title_color: Color::Rgb(86, 148, 159),         // foam
            title_modifier: Modifier::BOLD,
            selected_fg: Color::Rgb(87, 82, 121),          // text
            selected_bg: Color::Rgb(223, 218, 210),        // highlight med
            status_available: Color::Rgb(86, 148, 159),    // foam
            status_unavailable: Color::Rgb(152, 147, 165), // muted
            incoming_message: Color::Rgb(87, 82, 121),     // text
            outgoing_message: Color::Rgb(144, 122, 169),   // iris
            timestamp: Color::Rgb(152, 147, 165),          // muted
            help: Color::Rgb(152, 147, 165),               // muted
            active_border: Color::Rgb(144, 122, 169),      // iris
            inactive_border: Color::Rgb(242, 233, 222),    // highlight low
            search_highlight_fg: Color::Rgb(250, 244, 237),// base
            search_highlight_bg: Color::Rgb(234, 157, 52), // gold
            search_highlight_sel_fg: Color::Rgb(234, 157, 52),
            search_highlight_sel_bg: Color::Rgb(223, 218, 210),
            bg: Color::Rgb(250, 244, 237),                 // base
            fg: Color::Rgb(87, 82, 121),                   // text
        },
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cycle_all_wraps() {
        let last = *ALL_THEMES.last().unwrap();
        assert_eq!(cycle_all(last), ALL_THEMES[0]);
    }

    #[test]
    fn test_cycle_all_advances() {
        assert_eq!(cycle_all(ThemeName::Default), ThemeName::CatppuccinMocha);
    }

    #[test]
    fn test_cycle_all_crosses_dark_light() {
        assert_eq!(cycle_all(ThemeName::RosePine), ThemeName::CatppuccinLatte);
    }

    #[test]
    fn test_set_and_get_theme() {
        set_theme(ThemeName::Dracula);
        assert_eq!(current_theme_name(), ThemeName::Dracula);
        // Reset to default for other tests
        set_theme(ThemeName::Default);
    }

    #[test]
    fn test_theme_name_from_str() {
        assert_eq!("Default".parse::<ThemeName>(), Ok(ThemeName::Default));
        assert_eq!("catppuccin mocha".parse::<ThemeName>(), Ok(ThemeName::CatppuccinMocha));
        assert_eq!("CatppuccinMocha".parse::<ThemeName>(), Ok(ThemeName::CatppuccinMocha));
        assert!("nonexistent".parse::<ThemeName>().is_err());
    }

    #[test]
    fn test_all_palettes_constructible() {
        for &name in ALL_THEMES.iter() {
            let _palette = palette_for(name);
        }
    }
}
