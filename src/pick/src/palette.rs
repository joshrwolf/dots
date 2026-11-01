//! Picker colours, read from the shared theme package so the picker matches
//! every other application. Without an adapter the picker uses the terminal's
//! own palette, which Rex already themes.

use std::path::{Path, PathBuf};

use ratatui::style::Color;
use serde::Deserialize;

/// The roles a picker paints. Each is a colour, so a cell never falls back to
/// a terminal default that might not recede or stand out as intended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
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
}

impl Default for Palette {
    /// The terminal's own colours.
    fn default() -> Self {
        Self {
            background: Color::Reset,
            strong: Color::Reset,
            muted: Color::DarkGray,
            accent: Color::Blue,
            blue: Color::Blue,
            green: Color::Green,
            red: Color::Red,
            yellow: Color::Yellow,
            selection_bg: Color::Indexed(8),
            matched: Color::Magenta,
        }
    }
}

/// `pick.toml` in a theme directory. Every role is optional and falls back to
/// the terminal default, so a theme names only what it wants to change.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Adapter {
    background: Option<Hex>,
    strong: Option<Hex>,
    muted: Option<Hex>,
    accent: Option<Hex>,
    blue: Option<Hex>,
    green: Option<Hex>,
    red: Option<Hex>,
    yellow: Option<Hex>,
    selection_bg: Option<Hex>,
    matched: Option<Hex>,
}

#[derive(Debug, Clone, Copy)]
struct Hex(Color);

impl<'de> Deserialize<'de> for Hex {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        parse_hex(&text)
            .map(Hex)
            .ok_or_else(|| serde::de::Error::custom(format!("{text:?} is not a #rrggbb colour")))
    }
}

fn parse_hex(text: &str) -> Option<Color> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |range| u8::from_str_radix(&digits[range], 16).ok();
    Some(Color::Rgb(channel(0..2)?, channel(2..4)?, channel(4..6)?))
}

/// Why a theme's adapter could not be used.
#[derive(Debug, thiserror::Error)]
#[error("reading picker colours from {}", path.display())]
pub struct LoadError {
    path: PathBuf,
    #[source]
    source: AdapterError,
}

#[derive(Debug, thiserror::Error)]
enum AdapterError {
    #[error(transparent)]
    Read(#[from] std::io::Error),
    #[error(transparent)]
    Parse(#[from] toml::de::Error),
}

impl Palette {
    /// The current theme's adapter, at `$XDG_CONFIG_HOME/theme/current/pick.toml`.
    ///
    /// No adapter is normal and gives the terminal palette. A broken or
    /// unreadable one is an error: silently drawing other colours would hide
    /// the mistake.
    pub fn load() -> Result<Self, LoadError> {
        match adapter_path() {
            Some(path) => Ok(Self::read(&path)?.unwrap_or_default()),
            None => Ok(Self::default()),
        }
    }

    pub fn from_file(path: &Path) -> Result<Self, LoadError> {
        Self::read(path)?.ok_or_else(|| LoadError {
            path: path.to_owned(),
            source: std::io::Error::from(std::io::ErrorKind::NotFound).into(),
        })
    }

    /// The adapter at `path`, or `None` if there is none.
    fn read(path: &Path) -> Result<Option<Self>, LoadError> {
        let error = |source| LoadError {
            path: path.to_owned(),
            source,
        };
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(missing) if missing.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(unreadable) => return Err(error(unreadable.into())),
        };
        let adapter: Adapter = toml::from_str(&text).map_err(|e| error(e.into()))?;
        Ok(Some(Self::from_adapter(&adapter)))
    }

    fn from_adapter(adapter: &Adapter) -> Self {
        let base = Self::default();
        let pick = |value: Option<Hex>, fallback| value.map_or(fallback, |Hex(color)| color);
        Self {
            background: pick(adapter.background, base.background),
            strong: pick(adapter.strong, base.strong),
            muted: pick(adapter.muted, base.muted),
            accent: pick(adapter.accent, base.accent),
            blue: pick(adapter.blue, base.blue),
            green: pick(adapter.green, base.green),
            red: pick(adapter.red, base.red),
            yellow: pick(adapter.yellow, base.yellow),
            selection_bg: pick(adapter.selection_bg, base.selection_bg),
            matched: pick(adapter.matched, base.matched),
        }
    }
}

fn adapter_path() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(config.join("theme/current/pick.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_adapter_overrides_only_the_roles_it_names() {
        let adapter: Adapter = toml::from_str("accent = \"#0969da\"\nred = \"#CF222E\"").unwrap();
        let palette = Palette::from_adapter(&adapter);
        assert_eq!(palette.accent, Color::Rgb(0x09, 0x69, 0xda));
        assert_eq!(palette.red, Color::Rgb(0xcf, 0x22, 0x2e));
        assert_eq!(palette.muted, Palette::default().muted);
    }

    #[test]
    fn malformed_colours_and_unknown_roles_are_rejected() {
        for text in [
            "accent = \"0969da\"",
            "accent = \"#0969d\"",
            "accent = \"#zz69da\"",
            "acent = \"#0969da\"",
        ] {
            assert!(toml::from_str::<Adapter>(text).is_err(), "{text}");
        }
    }

    #[test]
    fn a_missing_adapter_is_absent_but_an_unreadable_one_is_an_error() {
        let dir = std::env::temp_dir().join(format!("dots-pick-adapter-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(Palette::read(&dir.join("missing.toml")).unwrap(), None);
        assert!(Palette::from_file(&dir.join("missing.toml")).is_err());
        // A directory exists but cannot be read as a file.
        let error = Palette::read(&dir).unwrap_err();
        assert!(
            matches!(&error.source, AdapterError::Read(read) if read.kind() != std::io::ErrorKind::NotFound),
            "{error:?}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_broken_adapter_is_an_error_naming_its_file() {
        let dir = std::env::temp_dir().join(format!("dots-pick-palette-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pick.toml");
        std::fs::write(&path, "accent = 3").unwrap();
        let error = Palette::from_file(&path).unwrap_err();
        assert!(error.to_string().contains("pick.toml"), "{error}");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
