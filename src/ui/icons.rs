//! Optional icon vocabulary. Auto mode is deliberately conservative: terminal
//! font selection is not standardized, and SSH hosts cannot inspect client fonts.
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    #[default]
    Auto,
    Nerd,
    Plain,
}
impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Nerd => "nerd",
            Self::Plain => "plain",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::Auto => Self::Nerd,
            Self::Nerd => Self::Plain,
            Self::Plain => Self::Auto,
        }
    }
    pub fn enabled(self, detected: bool) -> bool {
        match self {
            Self::Auto => detected,
            Self::Nerd => true,
            Self::Plain => false,
        }
    }
}
impl std::str::FromStr for Mode {
    type Err = std::io::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "auto" => Ok(Self::Auto),
            "nerd" => Ok(Self::Nerd),
            "plain" => Ok(Self::Plain),
            _ => Err(std::io::Error::other("Choose auto, nerd or plain")),
        }
    }
}
#[derive(Clone, Copy)]
pub enum Icon {
    Brand,
    Library,
    Discover,
    Downloads,
    Server,
    Settings,
    Project,
    Agent,
    Check,
    Warning,
    Memory,
    Chart,
    Theme,
}
impl Icon {
    pub fn glyph(self, nerd: bool) -> &'static str {
        if nerd {
            match self {
                Self::Brand => "\u{f0e7}",
                Self::Library => "\u{f1c0}",
                Self::Discover => "\u{f002}",
                Self::Downloads => "\u{f019}",
                Self::Server => "\u{f233}",
                Self::Settings => "\u{f013}",
                Self::Project => "\u{f07b}",
                Self::Agent => "\u{f121}",
                Self::Check => "\u{f00c}",
                Self::Warning => "\u{f071}",
                Self::Memory => "\u{f2db}",
                Self::Chart => "\u{f080}",
                Self::Theme => "\u{f1fc}",
            }
        } else {
            match self {
                Self::Brand => "◇",
                Self::Library => "▤",
                Self::Discover => "⌕",
                Self::Downloads => "↓",
                Self::Server => "◉",
                Self::Settings => "≡",
                Self::Project => "▸",
                Self::Agent => ">",
                Self::Check => "+",
                Self::Warning => "!",
                Self::Memory => "▥",
                Self::Chart => "▥",
                Self::Theme => "◈",
            }
        }
    }
}
fn eligible(term: &str, program: &str, graphical: bool, remote: bool, multiplexed: bool) -> bool {
    graphical
        && !remote
        && !multiplexed
        && (term == "xterm-kitty"
            || term == "wezterm"
            || program == "WezTerm"
            || program == "konsole")
}
/// One bounded optional local fontconfig query, never a dependency or a render-time process.
pub fn detect() -> bool {
    let var = |k: &str| std::env::var(k).unwrap_or_default();
    if !eligible(
        &var("TERM"),
        &var("TERM_PROGRAM"),
        !var("DISPLAY").is_empty() || !var("WAYLAND_DISPLAY").is_empty(),
        ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"]
            .iter()
            .any(|k| std::env::var_os(k).is_some()),
        ["TMUX", "STY"]
            .iter()
            .any(|k| std::env::var_os(k).is_some()),
    ) {
        return false;
    }
    crate::llama::process::command_output_unlocked(
        "/usr/bin/fc-list",
        &[
            ":charset=f0e7 f1c0 f002 f019 f233 f013 f07b f121 f00c f071 f2db f080 f1fc",
            "--format=%{family}\\n",
        ],
    )
    .is_ok_and(|out| {
        out.lines().any(|l| {
            let name = l.to_ascii_lowercase();
            name.contains("nerd font") || name.contains("symbols nerd")
        })
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conservative_detection_and_explicit_override() {
        assert!(eligible("xterm-kitty", "", true, false, false));
        assert!(!eligible("xterm-kitty", "", true, true, false));
        assert!(!eligible("xterm-kitty", "", true, false, true));
        assert!(!eligible("xterm-kitty", "", false, false, false));
        assert!(!eligible("xterm-256color", "", true, false, false));
        assert!(Mode::Nerd.enabled(false));
        assert!(!Mode::Plain.enabled(true));
        assert!(Mode::Auto.enabled(true));
        assert!(!Mode::Auto.enabled(false));
        assert_eq!(Mode::Auto.next().next().next(), Mode::Auto);
        assert!("invalid".parse::<Mode>().is_err());
        assert_eq!(Icon::Library.glyph(false), "▤");
    }
}
