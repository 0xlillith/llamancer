//! Draft settings editing. Changes are applied only after validation and persistence.
use crate::{config::Config, ui::theme::Theme};
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Theme,
    Agent,
    Endpoint,
    ServerBinary,
    ProjectDir,
    CustomAgent,
    AddRoot,
    RemoveRoot,
}
impl Field {
    pub const ALL: [Self; 8] = [
        Self::Theme,
        Self::Agent,
        Self::Endpoint,
        Self::ServerBinary,
        Self::ProjectDir,
        Self::CustomAgent,
        Self::AddRoot,
        Self::RemoveRoot,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Theme => "Theme",
            Self::Agent => "Default agent",
            Self::Endpoint => "Endpoint",
            Self::ServerBinary => "llama-server executable",
            Self::ProjectDir => "Agent project directory",
            Self::CustomAgent => "Custom agent executable",
            Self::AddRoot => "Add model root",
            Self::RemoveRoot => "Remove model root (exact path)",
        }
    }
    pub fn initial(self, config: &Config) -> String {
        match self {
            Self::Theme => config.theme.clone(),
            Self::Agent => config.default_agent.clone(),
            Self::Endpoint => config.endpoint.clone(),
            Self::ServerBinary => config.server_binary.clone(),
            Self::ProjectDir => config
                .project_dir
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            Self::CustomAgent => config.custom_agent.clone(),
            _ => String::new(),
        }
    }
}
pub fn apply(
    config: &Config,
    field: Field,
    value: &str,
    root: &Path,
) -> io::Result<(Config, Theme)> {
    let value = value.trim();
    let invalid = |message| io::Error::new(io::ErrorKind::InvalidInput, message);
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(invalid("Enter a nonempty value without control characters"));
    }
    let mut draft = config.clone();
    match field {
        Field::Theme => draft.theme = value.into(),
        Field::Agent => {
            if !matches!(value, "omp" | "pi" | "custom") {
                return Err(invalid("Choose omp, pi or custom"));
            }
            draft.default_agent = value.into();
        }
        Field::Endpoint => {
            draft.endpoint = crate::llama::client::endpoint(value)?.to_string();
        }
        Field::ServerBinary => draft.server_binary = value.into(),
        Field::CustomAgent => draft.custom_agent = value.into(),
        Field::ProjectDir => {
            let path = PathBuf::from(value);
            if !path.is_absolute() || !path.is_dir() {
                return Err(invalid("Enter an existing absolute project directory"));
            }
            draft.project_dir = Some(path.canonicalize()?);
        }
        Field::AddRoot => {
            let path = PathBuf::from(value);
            if !path.is_absolute() || !path.is_dir() {
                return Err(invalid("Enter an existing absolute directory"));
            }
            let path = path.canonicalize()?;
            if !draft.model_roots.contains(&path) {
                draft.model_roots.push(path);
            }
        }
        Field::RemoveRoot => {
            let path = PathBuf::from(value);
            draft.model_roots.retain(|p| p != &path);
        }
    }
    let theme = Theme::load(root, &draft.theme)?;
    Ok((draft, theme))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_and_preserves_original() {
        let root = tempfile::tempdir().unwrap();
        let config = Config::default();
        assert!(apply(&config, Field::Theme, "../bad", root.path()).is_err());
        assert!(apply(
            &config,
            Field::Endpoint,
            "http://user:pass@host",
            root.path()
        )
        .is_err());
        let (draft, _) = apply(
            &config,
            Field::AddRoot,
            root.path().to_str().unwrap(),
            root.path(),
        )
        .unwrap();
        assert!(config.model_roots.is_empty());
        assert_eq!(draft.model_roots.len(), 1);
    }
}
