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
    pub const BUILTINS: [&'static str; 8] = [
        "dracula",
        "andromeda",
        "catppuccin-mocha",
        "tokyo-night",
        "nord",
        "rose-pine",
        "gruvbox-dark",
        "high-contrast",
    ];
    fn builtin(name: &str) -> Option<Self> {
        let colors = match name {
            "dracula" => return Some(Self::default()),
            "andromeda" => [
                "#171624", "#e7e3f5", "#b7a1f5", "#f5a9d0", "#8fe4ec", "#a6e3b2", "#343048",
            ],
            "catppuccin-mocha" => [
                "#1e1e2e", "#cdd6f4", "#cba6f7", "#f5c2e7", "#89dceb", "#a6e3a1", "#45475a",
            ],
            "tokyo-night" => [
                "#1a1b26", "#c0caf5", "#bb9af7", "#f7768e", "#7dcfff", "#9ece6a", "#33467c",
            ],
            "nord" => [
                "#2e3440", "#eceff4", "#b48ead", "#d8dee9", "#88c0d0", "#a3be8c", "#434c5e",
            ],
            "rose-pine" => [
                "#191724", "#e0def4", "#c4a7e7", "#ebbcba", "#9ccfd8", "#9ccfd8", "#403d52",
            ],
            "gruvbox-dark" => [
                "#282828", "#ebdbb2", "#d3869b", "#fabd2f", "#8ec07c", "#b8bb26", "#504945",
            ],
            "high-contrast" => [
                "#000000", "#ffffff", "#d4bfff", "#ffffff", "#80ffff", "#80ff80", "#303030",
            ],
            _ => return None,
        };
        let [background, foreground, purple, pink, cyan, green, selection] =
            colors.map(str::to_owned);
        Some(Self {
            background,
            foreground,
            purple,
            pink,
            cyan,
            green,
            selection,
        })
    }
    pub fn next_builtin(name: &str) -> &'static str {
        Self::BUILTINS
            .iter()
            .position(|&n| n == name)
            .map_or(Self::BUILTINS[0], |i| {
                Self::BUILTINS[(i + 1) % Self::BUILTINS.len()]
            })
    }
    pub fn load(root: &Path, name: &str) -> io::Result<Self> {
        if let Some(theme) = Self::builtin(name) {
            return Ok(theme);
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
        let contents = crate::config::read_state(
            &root.join("themes").join(format!("{name}.toml")),
            64 * 1024,
        )?;
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
    fn builtins_available_without_theme_files() {
        let root = tempfile::tempdir().unwrap();
        for (i, name) in Theme::BUILTINS.iter().enumerate() {
            let theme = Theme::load(root.path(), name).unwrap();
            for value in [
                &theme.background,
                &theme.foreground,
                &theme.purple,
                &theme.pink,
                &theme.cyan,
                &theme.green,
                &theme.selection,
            ] {
                assert!(color(value).is_some());
            }
            assert_eq!(
                Theme::next_builtin(name),
                Theme::BUILTINS[(i + 1) % Theme::BUILTINS.len()]
            );
        }
        assert_eq!(Theme::next_builtin("custom"), "dracula");
    }
    #[test]
    fn traversal_rejected() {
        assert!(Theme::load(Path::new("."), "../secret").is_err());
    }
}
