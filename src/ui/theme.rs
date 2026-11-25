//! Named terminal colors loaded from TOML.
use ratatui::style::Color;
use serde::Deserialize;
use std::{io, path::Path};

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub background: String,
    pub foreground: String,
    pub purple: String,
    pub pink: String,
    pub cyan: String,
    pub green: String,
    pub selection: String,
}
impl Default for Theme {
    fn default() -> Self {
        Self {
            background: "#282a36".into(),
            foreground: "#f8f8f2".into(),
            purple: "#bd93f9".into(),
            pink: "#ff79c6".into(),
            cyan: "#8be9fd".into(),
            green: "#50fa7b".into(),
            selection: "#44475a".into(),
        }
    }
}
pub fn color(value: &str) -> Option<Color> {
    let hex = value.strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let n = u32::from_str_radix(hex, 16).ok()?;
    Some(Color::Rgb((n >> 16) as u8, (n >> 8) as u8, n as u8))
}
impl Theme {
    pub fn load(root: &Path, name: &str) -> io::Result<Self> {
        if name == "dracula" {
            return Ok(Self::default());
        }
        if name == "high-contrast" {
            return Ok(Self {
                background: "#000000".into(),
                foreground: "#ffffff".into(),
                purple: "#d4bfff".into(),
                pink: "#ffffff".into(),
                cyan: "#80ffff".into(),
                green: "#80ff80".into(),
                selection: "#303030".into(),
            });
        }
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid theme name",
            ));
        }
        let contents = std::fs::read_to_string(root.join("themes").join(format!("{name}.toml")))?;
        let t: Self =
            toml::from_str(&contents).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        for value in [
            &t.background,
            &t.foreground,
            &t.purple,
            &t.pink,
            &t.cyan,
            &t.green,
            &t.selection,
        ] {
            if color(value).is_none() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Invalid theme color",
                ));
            }
        }
        Ok(t)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hex_validation() {
        assert_eq!(color("#ff79c6"), Some(Color::Rgb(255, 121, 198)));
        assert!(color("#ああ").is_none());
        assert!(color("#xyzxyz").is_none());
    }
    #[test]
    fn traversal_rejected() {
        assert!(Theme::load(Path::new("."), "../secret").is_err());
    }
}
