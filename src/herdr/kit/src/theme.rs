//! Colours and status glyphs resolved from herdr's own config, so a plugin
//! agrees with the sidebar instead of carrying a second palette that drifts.

use std::path::PathBuf;

use ratatui::style::Color;
use serde::Deserialize;

use crate::api::AgentStatus;

/// herdr's two status glyph sets, selected by `ui.status_indicators`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Indicators {
    #[default]
    Dots,
    Symbols,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// Background shared by every picker cell.
    pub background: Color,
    /// What a row *is*.
    pub strong: Color,
    /// The context around it.
    pub muted: Color,
    pub accent: Color,
    pub blue: Color,
    pub green: Color,
    pub red: Color,
    pub yellow: Color,
    pub selection_bg: Color,
    /// Matched characters within a row.
    pub matched: Color,
    pub indicators: Indicators,
}

impl Default for Theme {
    /// Herdr's default Catppuccin Mocha palette.
    fn default() -> Self {
        Self::palette(
            [24, 24, 37],
            [205, 214, 244],
            [108, 112, 134],
            [137, 180, 250],
            [137, 180, 250],
            [166, 227, 161],
            [243, 139, 168],
            [249, 226, 175],
            [49, 50, 68],
            [203, 166, 247],
        )
    }
}

impl Theme {
    /// Reads herdr's config, falling back to the default for anything absent.
    ///
    /// Missing or unparseable config is normal, not an error: a plugin that
    /// refused to draw because a colour was misspelled would be worse than one
    /// that drew in the default palette.
    pub fn load() -> Self {
        let Some(path) = config_path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        toml::from_str::<ConfigFile>(&text)
            .map(|c| Self::from_config(&c))
            .unwrap_or_default()
    }

    fn from_config(config: &ConfigFile) -> Self {
        let base = config
            .theme
            .name
            .as_deref()
            .and_then(Self::named)
            .unwrap_or_default();
        let custom = &config.theme.custom;
        let pick = |value: Option<&String>, fallback| value.map_or(fallback, |s| parse_color(s));
        Self {
            strong: pick(custom.text.as_ref(), base.strong),
            background: pick(custom.panel_bg.as_ref(), base.background),
            muted: pick(custom.overlay0.as_ref(), base.muted),
            accent: pick(
                custom.accent.as_ref().or(config.ui.accent.as_ref()),
                base.accent,
            ),
            blue: pick(custom.blue.as_ref(), base.blue),
            green: pick(custom.green.as_ref(), base.green),
            red: pick(custom.red.as_ref(), base.red),
            yellow: pick(custom.yellow.as_ref(), base.yellow),
            selection_bg: pick(custom.selection_bg.as_ref(), base.selection_bg),
            matched: pick(custom.mauve.as_ref(), base.matched),
            indicators: config.ui.status_indicators,
        }
    }

    /// Resolves the picker-facing subset of one of Herdr's built-in palettes.
    #[expect(
        clippy::too_many_lines,
        reason = "keeping every palette in one exhaustive match makes drift and omissions reviewable"
    )]
    fn named(name: &str) -> Option<Self> {
        let palette = match canonical_name(name)? {
            "catppuccin" => Self::default(),
            "catppuccin-latte" => Self::palette(
                [239, 241, 245],
                [76, 79, 105],
                [156, 160, 176],
                [30, 102, 245],
                [30, 102, 245],
                [64, 160, 43],
                [210, 15, 57],
                [223, 142, 29],
                [189, 208, 245],
                [136, 57, 239],
            ),
            "terminal" => Self {
                background: Color::Reset,
                strong: Color::Reset,
                muted: Color::Gray,
                accent: Color::Blue,
                blue: Color::Blue,
                green: Color::Green,
                red: Color::LightRed,
                yellow: Color::Yellow,
                selection_bg: Color::Reset,
                matched: Color::Gray,
                indicators: Indicators::Dots,
            },
            "tokyo-night" => Self::palette(
                [26, 27, 38],
                [192, 202, 245],
                [86, 95, 137],
                [122, 162, 247],
                [122, 162, 247],
                [158, 206, 106],
                [247, 118, 142],
                [224, 175, 104],
                [45, 54, 80],
                [187, 154, 247],
            ),
            "tokyo-night-day" => Self::palette(
                [225, 226, 231],
                [55, 96, 191],
                [137, 144, 179],
                [46, 125, 233],
                [46, 125, 233],
                [88, 117, 57],
                [245, 42, 101],
                [140, 108, 62],
                [182, 202, 231],
                [120, 71, 189],
            ),
            "dracula" => Self::palette(
                [40, 42, 54],
                [248, 248, 242],
                [98, 114, 164],
                [189, 147, 249],
                [139, 233, 253],
                [80, 250, 123],
                [255, 85, 85],
                [241, 250, 140],
                [70, 63, 93],
                [255, 121, 198],
            ),
            "nord" => Self::palette(
                [46, 52, 64],
                [236, 239, 244],
                [76, 86, 106],
                [136, 192, 208],
                [129, 161, 193],
                [163, 190, 140],
                [191, 97, 106],
                [235, 203, 139],
                [64, 80, 93],
                [180, 142, 173],
            ),
            "gruvbox" => Self::palette(
                [40, 40, 40],
                [235, 219, 178],
                [146, 131, 116],
                [215, 153, 33],
                [131, 165, 152],
                [184, 187, 38],
                [251, 73, 52],
                [250, 189, 47],
                [75, 63, 39],
                [211, 134, 155],
            ),
            "gruvbox-light" => Self::palette(
                [251, 241, 199],
                [60, 56, 54],
                [146, 131, 116],
                [7, 102, 120],
                [7, 102, 120],
                [121, 116, 14],
                [157, 0, 6],
                [181, 118, 20],
                [235, 219, 178],
                [143, 63, 113],
            ),
            "one-dark" => Self::palette(
                [40, 44, 52],
                [171, 178, 191],
                [92, 99, 112],
                [97, 175, 239],
                [97, 175, 239],
                [152, 195, 121],
                [224, 108, 117],
                [229, 192, 123],
                [51, 70, 89],
                [198, 120, 221],
            ),
            "one-light" => Self::palette(
                [250, 250, 250],
                [56, 58, 66],
                [160, 161, 167],
                [64, 120, 242],
                [64, 120, 242],
                [80, 161, 79],
                [228, 86, 73],
                [193, 132, 1],
                [205, 219, 248],
                [166, 38, 164],
            ),
            "solarized" => Self::palette(
                [0, 43, 54],
                [147, 161, 161],
                [88, 110, 117],
                [38, 139, 210],
                [38, 139, 210],
                [133, 153, 0],
                [220, 50, 47],
                [181, 137, 0],
                [8, 62, 85],
                [211, 54, 130],
            ),
            "solarized-light" => Self::palette(
                [253, 246, 227],
                [101, 123, 131],
                [147, 161, 161],
                [38, 139, 210],
                [38, 139, 210],
                [133, 153, 0],
                [220, 50, 47],
                [181, 137, 0],
                [201, 220, 223],
                [211, 54, 130],
            ),
            "kanagawa" => Self::palette(
                [31, 31, 40],
                [220, 215, 186],
                [114, 113, 105],
                [126, 156, 216],
                [126, 156, 216],
                [118, 148, 106],
                [195, 64, 67],
                [192, 163, 110],
                [50, 56, 75],
                [149, 127, 184],
            ),
            "kanagawa-lotus" => Self::palette(
                [242, 236, 188],
                [84, 84, 100],
                [160, 156, 172],
                [77, 105, 155],
                [77, 105, 155],
                [111, 137, 78],
                [200, 64, 83],
                [119, 113, 63],
                [220, 213, 172],
                [98, 76, 131],
            ),
            "rose-pine" => Self::palette(
                [25, 23, 36],
                [224, 222, 244],
                [110, 106, 134],
                [196, 167, 231],
                [49, 116, 143],
                [49, 116, 143],
                [235, 111, 146],
                [246, 193, 119],
                [59, 52, 75],
                [196, 167, 231],
            ),
            "rose-pine-dawn" => Self::palette(
                [250, 244, 237],
                [70, 66, 97],
                [152, 147, 165],
                [144, 122, 169],
                [40, 105, 131],
                [40, 105, 131],
                [180, 99, 122],
                [234, 157, 52],
                [242, 233, 225],
                [144, 122, 169],
            ),
            "vesper" => Self::palette(
                [26, 26, 26],
                [255, 255, 255],
                [92, 92, 92],
                [255, 199, 153],
                [176, 176, 176],
                [153, 255, 228],
                [255, 128, 128],
                [255, 199, 153],
                [35, 35, 35],
                [255, 209, 168],
            ),
            _ => return None,
        };
        Some(palette)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "each call is one literal palette; grouping channels would make the tables harder to audit against Herdr"
    )]
    fn palette(
        background: [u8; 3],
        strong: [u8; 3],
        muted: [u8; 3],
        accent: [u8; 3],
        blue: [u8; 3],
        green: [u8; 3],
        red: [u8; 3],
        yellow: [u8; 3],
        selection_bg: [u8; 3],
        matched: [u8; 3],
    ) -> Self {
        let color = |[red, green, blue]: [u8; 3]| Color::Rgb(red, green, blue);
        Self {
            background: color(background),
            strong: color(strong),
            muted: color(muted),
            accent: color(accent),
            blue: color(blue),
            green: color(green),
            red: color(red),
            yellow: color(yellow),
            selection_bg: color(selection_bg),
            matched: color(matched),
            indicators: Indicators::Dots,
        }
    }

    /// The glyph and colour for a status, in herdr's own state order.
    ///
    /// Idle and unknown are resting states and read better muted than tinted;
    /// colour is reserved for the three that want a response.
    pub fn status(&self, status: AgentStatus) -> (char, Color) {
        let symbols = self.indicators == Indicators::Symbols;
        match status {
            AgentStatus::Blocked => (if symbols { '×' } else { '●' }, self.red),
            AgentStatus::Working => (if symbols { '◐' } else { '●' }, self.yellow),
            AgentStatus::Done => (if symbols { '✓' } else { '●' }, self.green),
            AgentStatus::Idle => ('○', self.muted),
            AgentStatus::Unknown => ('·', self.muted),
        }
    }
}

fn config_path() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("HERDR_CONFIG_PATH") {
        return Some(PathBuf::from(explicit));
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            if cfg!(windows) {
                std::env::var_os("APPDATA").map(PathBuf::from)
            } else {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
            }
        })?;
    Some(base.join("herdr/config.toml"))
}

#[derive(Debug, Default, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    theme: ThemeTable,
    #[serde(default)]
    ui: UiTable,
}

#[derive(Debug, Default, Deserialize)]
struct ThemeTable {
    name: Option<String>,
    #[serde(default)]
    custom: CustomColors,
}

#[derive(Debug, Default, Deserialize)]
struct CustomColors {
    accent: Option<String>,
    panel_bg: Option<String>,
    selection_bg: Option<String>,
    text: Option<String>,
    overlay0: Option<String>,
    mauve: Option<String>,
    green: Option<String>,
    yellow: Option<String>,
    red: Option<String>,
    blue: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct UiTable {
    #[serde(default)]
    status_indicators: Indicators,
    /// Legacy setting still accepted by Herdr. `theme.custom.accent` wins.
    accent: Option<String>,
}

fn canonical_name(name: &str) -> Option<&'static str> {
    match name.to_ascii_lowercase().replace([' ', '_'], "-").as_str() {
        "catppuccin" | "catppuccin-mocha" => Some("catppuccin"),
        "catppuccin-latte" | "latte" | "light" => Some("catppuccin-latte"),
        "terminal" => Some("terminal"),
        "tokyo-night" | "tokyonight" => Some("tokyo-night"),
        "tokyo-night-day" | "tokyo-day" | "tokyonight-day" => Some("tokyo-night-day"),
        "dracula" => Some("dracula"),
        "nord" => Some("nord"),
        "gruvbox" | "gruvbox-dark" => Some("gruvbox"),
        "gruvbox-light" => Some("gruvbox-light"),
        "one-dark" | "onedark" => Some("one-dark"),
        "one-light" | "onelight" => Some("one-light"),
        "solarized" | "solarized-dark" => Some("solarized"),
        "solarized-light" => Some("solarized-light"),
        "kanagawa" => Some("kanagawa"),
        "kanagawa-lotus" | "lotus" => Some("kanagawa-lotus"),
        "rose-pine" | "rosepine" => Some("rose-pine"),
        "rose-pine-dawn" | "rosepine-dawn" | "dawn" => Some("rose-pine-dawn"),
        "vesper" => Some("vesper"),
        _ => None,
    }
}

/// Parses the same color forms Herdr accepts. Invalid values resolve to cyan,
/// matching Herdr's diagnosed fallback rather than silently choosing a
/// different base-theme color in the plugin.
fn parse_color(value: &str) -> Color {
    let value = value.trim().to_ascii_lowercase();
    if matches!(value.as_str(), "reset" | "default" | "none" | "transparent") {
        return Color::Reset;
    }
    if let Some(hex) = value.strip_prefix('#') {
        let digits: Vec<u8> = hex
            .chars()
            .filter_map(|ch| ch.to_digit(16).and_then(|digit| u8::try_from(digit).ok()))
            .collect();
        match (hex.len(), digits.as_slice()) {
            (3, [red, green, blue]) => {
                return Color::Rgb(red * 17, green * 17, blue * 17);
            }
            (
                6,
                [
                    red_high,
                    red_low,
                    green_high,
                    green_low,
                    blue_high,
                    blue_low,
                ],
            ) => {
                return Color::Rgb(
                    red_high * 16 + red_low,
                    green_high * 16 + green_low,
                    blue_high * 16 + blue_low,
                );
            }
            _ => {}
        }
    }
    if let Some(inner) = value.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
        let mut parts = inner.split(',').map(str::trim);
        if let (Some(red), Some(green), Some(blue), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
            && let (Ok(red), Ok(green), Ok(blue)) =
                (red.parse::<u8>(), green.parse::<u8>(), blue.parse::<u8>())
        {
            return Color::Rgb(red, green, blue);
        }
    }
    match value.as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" | "purple" => Color::Magenta,
        "white" => Color::White,
        "gray" | "grey" => Color::Gray,
        "darkgray" | "darkgrey" => Color::DarkGray,
        "lightred" => Color::LightRed,
        "lightgreen" => Color::LightGreen,
        "lightyellow" => Color::LightYellow,
        "lightblue" => Color::LightBlue,
        "lightmagenta" => Color::LightMagenta,
        "lightcyan" => Color::LightCyan,
        _ => Color::Cyan,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_tokens_override_and_the_rest_survive() {
        let config: ConfigFile = toml::from_str(
            r##"
            [theme]
            name = "one-light"
            [theme.custom]
            blue = "#0969da"
            red = "not a colour"
            [ui]
            status_indicators = "symbols"
            window_title = "{workspace}"
            "##,
        )
        .unwrap();
        let theme = Theme::from_config(&config);
        assert_eq!(theme.blue, Color::Rgb(0x09, 0x69, 0xda));
        assert_eq!(
            theme.red,
            Color::Cyan,
            "matches Herdr's invalid-color fallback"
        );
        assert_eq!(theme.indicators, Indicators::Symbols);
    }

    #[test]
    fn an_empty_config_is_the_default_theme() {
        let theme = Theme::from_config(&toml::from_str("").unwrap());
        assert_eq!(theme.indicators, Indicators::Dots);
        assert_eq!(theme.strong, Theme::default().strong);
    }

    #[test]
    fn named_themes_and_aliases_use_herdrs_palette() {
        for name in ["one-light", "one_light", "onelight"] {
            let config: ConfigFile = toml::from_str(&format!("[theme]\nname = {name:?}")).unwrap();
            let theme = Theme::from_config(&config);
            assert_eq!(theme.strong, Color::Rgb(56, 58, 66), "alias {name}");
            assert_eq!(theme.selection_bg, Color::Rgb(205, 219, 248));
        }
    }

    #[test]
    fn every_herdr_builtin_has_a_picker_palette() {
        for name in [
            "catppuccin",
            "catppuccin-latte",
            "terminal",
            "tokyo-night",
            "tokyo-night-day",
            "dracula",
            "nord",
            "gruvbox",
            "gruvbox-light",
            "one-dark",
            "one-light",
            "solarized",
            "solarized-light",
            "kanagawa",
            "kanagawa-lotus",
            "rose-pine",
            "rose-pine-dawn",
            "vesper",
        ] {
            assert!(Theme::named(name).is_some(), "missing palette {name}");
        }
    }

    #[test]
    fn unknown_theme_names_fall_back_to_herdrs_default() {
        let config: ConfigFile = toml::from_str("[theme]\nname = 'no-such-theme'").unwrap();
        assert_eq!(Theme::from_config(&config), Theme::default());
    }

    #[test]
    fn every_picker_token_can_be_overridden_with_herdr_color_syntax() {
        let config: ConfigFile = toml::from_str(
            r##"
            [theme]
            name = "terminal"
            [theme.custom]
            panel_bg = "#010203"
            text = "#123"
            overlay0 = "rgb(4, 5, 6)"
            accent = "purple"
            blue = "#070809"
            green = "lightgreen"
            red = "rgb(10, 11, 12)"
            yellow = "yellow"
            selection_bg = "transparent"
            mauve = "#def"

            [theme.custom.light]
            text = "#ffffff"
            "##,
        )
        .unwrap();
        let theme = Theme::from_config(&config);
        assert_eq!(theme.background, Color::Rgb(1, 2, 3));
        assert_eq!(theme.strong, Color::Rgb(0x11, 0x22, 0x33));
        assert_eq!(theme.muted, Color::Rgb(4, 5, 6));
        assert_eq!(theme.accent, Color::Magenta);
        assert_eq!(theme.blue, Color::Rgb(7, 8, 9));
        assert_eq!(theme.green, Color::LightGreen);
        assert_eq!(theme.red, Color::Rgb(10, 11, 12));
        assert_eq!(theme.yellow, Color::Yellow);
        assert_eq!(theme.selection_bg, Color::Reset);
        assert_eq!(theme.matched, Color::Rgb(0xdd, 0xee, 0xff));
    }

    #[test]
    fn custom_accent_takes_precedence_over_legacy_ui_accent() {
        let config: ConfigFile =
            toml::from_str("[theme.custom]\naccent = 'red'\n[ui]\naccent = 'green'\n").unwrap();
        assert_eq!(Theme::from_config(&config).accent, Color::Red);

        let legacy: ConfigFile = toml::from_str("[ui]\naccent = 'green'\n").unwrap();
        assert_eq!(Theme::from_config(&legacy).accent, Color::Green);
    }

    #[test]
    fn resting_states_are_muted_and_active_ones_are_not() {
        let theme = Theme::default();
        assert_eq!(theme.status(AgentStatus::Idle).1, theme.muted);
        assert_eq!(theme.status(AgentStatus::Unknown).1, theme.muted);
        assert_eq!(theme.status(AgentStatus::Blocked).1, theme.red);
    }

    #[test]
    fn symbols_and_dots_cover_every_status() {
        for indicators in [Indicators::Dots, Indicators::Symbols] {
            let theme = Theme {
                indicators,
                ..Theme::default()
            };
            let glyphs: Vec<char> = [
                AgentStatus::Blocked,
                AgentStatus::Working,
                AgentStatus::Done,
                AgentStatus::Idle,
                AgentStatus::Unknown,
            ]
            .into_iter()
            .map(|s| theme.status(s).0)
            .collect();
            assert_eq!(glyphs.len(), 5);
            assert!(
                glyphs
                    .iter()
                    .all(|g| crate::columns::width(&g.to_string()) == 1)
            );
        }
    }
}
