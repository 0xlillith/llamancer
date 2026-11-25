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
    AgentTerminal,
    Endpoint,
    ApiKeyFile,
    ServerBinary,
    Device,
    ProjectDir,
    WorkspaceRoot,
    CustomAgent,
    AddRoot,
    RemoveRoot,
    ImportModel,
    RestoreModel,
    RemoveImport,
}
impl Field {
    pub const ALL: [Self; 15] = [
        Self::Theme,
        Self::Agent,
        Self::AgentTerminal,
        Self::Endpoint,
        Self::ApiKeyFile,
        Self::ServerBinary,
        Self::Device,
        Self::ProjectDir,
        Self::CustomAgent,
        Self::AddRoot,
        Self::RemoveRoot,
        Self::ImportModel,
        Self::RestoreModel,
        Self::RemoveImport,
        Self::WorkspaceRoot,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Theme => "Theme (T to cycle built-ins)",
            Self::Agent => "Default agent",
            Self::AgentTerminal => "Agent terminal (current/kitty-tab/kitty-window)",
            Self::Endpoint => "Endpoint",
            Self::ApiKeyFile => "API key file (absolute private path or none)",
            Self::ServerBinary => "llama-server executable",
            Self::Device => "GPU device (auto/none/device IDs)",
            Self::ProjectDir => "Active project directory (or none)",
            Self::WorkspaceRoot => "Workspace parent directory (or none)",
            Self::CustomAgent => "Custom agent executable",
            Self::AddRoot => "Add model root",
            Self::RemoveRoot => "Remove model root (exact path)",
            Self::ImportModel => "Import GGUF by reference (absolute path)",
            Self::RestoreModel => "Restore hidden model (exact identity path)",
            Self::RemoveImport => "Remove imported model reference (exact path)",
        }
    }
    pub fn initial(self, config: &Config) -> String {
        match self {
            Self::Theme => config.theme.clone(),
            Self::Agent => config.default_agent.clone(),
            Self::AgentTerminal => config.agent_terminal.label().into(),
            Self::Endpoint => config.endpoint.clone(),
            Self::ApiKeyFile => config
                .api_key_file
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "none".into()),
            Self::ServerBinary => config.server_binary.clone(),
            Self::Device => config.device.clone(),
            Self::WorkspaceRoot => config
                .workspace_root
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "none".into()),
            Self::ProjectDir => config
                .project_dir
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "none".into()),
            Self::CustomAgent => config.custom_agent.clone(),
            Self::RemoveImport => config
                .model_imports
                .first()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            Self::RestoreModel => config
                .hidden_models
                .iter()
                .next()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
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
            if !matches!(value, "omp" | "pi" | "pi-native" | "custom") {
                return Err(invalid("Choose omp, pi, pi-native or custom"));
            }
            draft.default_agent = value.into();
        }
        Field::AgentTerminal => draft.agent_terminal = value.parse()?,
        Field::Endpoint => {
            let endpoint = crate::llama::client::endpoint(value)?;
            if endpoint != crate::llama::client::endpoint(&config.endpoint)? {
                draft.api_key_file = None;
            }
            draft.endpoint = endpoint.to_string();
        }
        Field::ApiKeyFile => {
            draft.api_key_file = if value == "none" {
                None
            } else {
                Some(PathBuf::from(value))
            };
            crate::llama::auth::Key::for_endpoint(
                draft.api_key_file.as_deref(),
                &crate::llama::client::endpoint(&draft.endpoint)?,
            )?;
        }
        Field::ServerBinary => draft.server_binary = value.into(),
        Field::Device => {
            if !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b',' | b'.' | b':'))
            {
                return Err(invalid("Use auto, none or comma-separated device IDs"));
            }
            draft.device = value.into();
        }
        Field::CustomAgent => draft.custom_agent = value.into(),
        Field::ProjectDir | Field::WorkspaceRoot => {
            let selected = if value == "none" {
                None
            } else {
                let path = PathBuf::from(value);
                if !path.is_absolute() || !path.is_dir() {
                    return Err(invalid("Enter an existing absolute directory or none"));
                }
                Some(path.canonicalize()?)
            };
            if field == Field::WorkspaceRoot {
                draft.workspace_root = selected;
            } else {
                draft.project_dir = selected;
            }
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
        Field::ImportModel => {
            let path = PathBuf::from(value);
            if !path.is_absolute()
                || !path.is_file()
                || !path
                    .extension()
                    .is_some_and(|s| s.eq_ignore_ascii_case("gguf"))
            {
                return Err(invalid("Enter an existing absolute GGUF file path"));
            }
            let path = crate::models::file_reference(&path)?;
            if !draft.model_imports.contains(&path) {
                draft.model_imports.push(path);
            }
        }
        Field::RemoveImport => {
            let path = PathBuf::from(value);
            if !draft.model_imports.contains(&path) {
                return Err(invalid("No imported model with this path"));
            }
            draft.model_imports.retain(|p| p != &path);
        }
        Field::RestoreModel => {
            let path = PathBuf::from(value);
            if !draft.hidden_models.remove(&path) {
                return Err(invalid("No hidden model with this identity"));
            }
        }
        Field::RemoveRoot => {
            let path = PathBuf::from(value);
            draft.model_roots.retain(|p| p != &path);
        }
    }
    if draft.api_key_file.is_some() {
        crate::llama::auth::Key::for_endpoint(
            draft.api_key_file.as_deref(),
            &crate::llama::client::endpoint(&draft.endpoint)?,
        )?;
    }
    let theme = Theme::load(root, &draft.theme)?;
    Ok((draft, theme))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removes_only_import_reference_and_prefills_restore() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("model.gguf");
        std::fs::write(&path, b"fixture").unwrap();
        let mut config = Config::default();
        config.model_imports.push(path.clone());
        config.hidden_models.insert(path.clone());
        assert_eq!(Field::RestoreModel.initial(&config), path.to_string_lossy());
        let (next, _) = apply(
            &config,
            Field::RemoveImport,
            path.to_str().unwrap(),
            root.path(),
        )
        .unwrap();
        assert!(next.model_imports.is_empty());
        assert!(path.is_file());
        assert!(next.hidden_models.contains(&path));
    }
    #[test]
    fn changing_endpoint_clears_previous_credential() {
        use std::io::Write;
        let root = tempfile::tempdir().unwrap();
        let mut key = tempfile::NamedTempFile::new_in(root.path()).unwrap();
        writeln!(key, "fixture-only").unwrap();
        let config = Config {
            api_key_file: Some(key.path().into()),
            ..Config::default()
        };
        let (same, _) = apply(
            &config,
            Field::Endpoint,
            "http://127.0.0.1:8080/v1",
            root.path(),
        )
        .unwrap();
        assert_eq!(same.api_key_file, config.api_key_file);
        let (changed, _) = apply(
            &config,
            Field::Endpoint,
            "https://example.invalid",
            root.path(),
        )
        .unwrap();
        assert!(changed.api_key_file.is_none());
    }
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
