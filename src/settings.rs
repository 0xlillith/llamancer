//! Draft settings editing. Changes are applied only after validation and persistence.
use crate::{config::Config, ui::theme::Theme};
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Theme,
    Icons,
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
    DownloadRoot,
    RemoveRoot,
    ImportModel,
    RestoreModel,
    RemoveImport,
}
impl Field {
    pub const ALL: [Self; 17] = [
        Self::AddRoot,
        Self::DownloadRoot,
        Self::ImportModel,
        Self::RemoveRoot,
        Self::RemoveImport,
        Self::RestoreModel,
        Self::ServerBinary,
        Self::Device,
        Self::Endpoint,
        Self::ApiKeyFile,
        Self::Agent,
        Self::AgentTerminal,
        Self::CustomAgent,
        Self::WorkspaceRoot,
        Self::ProjectDir,
        Self::Theme,
        Self::Icons,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Theme => "Theme (T to cycle built-ins)",
            Self::Icons => "Icons (auto/nerd/plain; I to cycle)",
            Self::Agent => "Default agent",
            Self::AgentTerminal => "Agent terminal (new-window/current/kitty-tab/kitty-window)",
            Self::Endpoint => "Endpoint",
            Self::ApiKeyFile => "API key file (absolute private path or none)",
            Self::ServerBinary => "llama-server executable",
            Self::Device => "GPU device (auto/none/device IDs)",
            Self::ProjectDir => "Active project (agent working folder)",
            Self::WorkspaceRoot => "Workspace root (parent of all projects)",
            Self::CustomAgent => "Custom agent executable",
            Self::AddRoot => "Add model root",
            Self::DownloadRoot => "Default download root (or none)",
            Self::RemoveRoot => "Remove model root (exact path)",
            Self::ImportModel => "Import GGUF by reference (absolute path)",
            Self::RestoreModel => "Restore hidden model (exact identity path)",
            Self::RemoveImport => "Remove imported model reference (exact path)",
        }
    }
    pub fn hint(self) -> &'static str {
        match self {
            Self::AddRoot=>"Required for downloading. Enter default to create the suggested private model folder, or choose an existing absolute folder.",
            Self::DownloadRoot=>"Optional. none asks which configured folder to use when there are several.",
            Self::RemoveRoot=>"Removes a library reference, not the files.",
            Self::ImportModel=>"Optional. Index one existing GGUF without copying it.",
            Self::RemoveImport=>"Remove the reference, not the model file.",
            Self::RestoreModel=>"Restore a hidden entry; does not change the file.",
            Self::Theme=>"Appearance only. Press T to cycle built-in themes.",
            Self::Icons=>"Auto detects supported local fonts; plain works everywhere.",
            Self::Agent=>"Optional coding agent: omp, pi, pi-native or custom.",
            Self::AgentTerminal=>"new-window opens independent Kitty; current is an explicit foreground alternative.",
            Self::Endpoint=>"Local default: http://127.0.0.1:8080. Only change for another server.",
            Self::ApiKeyFile=>"Optional. none is normal for local managed sessions. Use a private key file for external servers.",
            Self::ServerBinary=>"Normally select an installed llama.cpp backend with B rather than typing a path.",
            Self::Device=>"auto lets the backend choose. none forces CPU. B selects a tested GPU.",
            Self::ProjectDir=>"Active project: the exact folder in which the coding agent works. Press P to switch.",
            Self::WorkspaceRoot=>"Optional parent of your projects, e.g. /home/user/src. P selects an individual project.",
            Self::CustomAgent=>"Only required when Default agent is custom.",
        }
    }
    pub fn category(self) -> &'static str {
        match self {
            Self::Theme | Self::Icons => "Appearance",
            Self::AddRoot
            | Self::RemoveRoot
            | Self::DownloadRoot
            | Self::ImportModel
            | Self::RemoveImport
            | Self::RestoreModel => "Storage",
            Self::Agent | Self::AgentTerminal | Self::CustomAgent => "Agent",
            Self::ProjectDir | Self::WorkspaceRoot => "Workspace",
            _ => "Inference",
        }
    }
    pub fn initial(self, config: &Config) -> String {
        match self {
            Self::AddRoot => {
                if config.model_roots.is_empty() {
                    "default".into()
                } else {
                    String::new()
                }
            }
            Self::Theme => config.theme.clone(),
            Self::Icons => config.icon_mode.label().into(),
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
            Self::DownloadRoot => config
                .download_root
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "none".into()),
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
pub fn default_model_root() -> io::Result<PathBuf> {
    crate::backends::root()?
        .parent()
        .map(|p| p.join("models"))
        .ok_or_else(|| io::Error::other("No user data directory"))
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
        Field::Icons => draft.icon_mode = value.parse()?,
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
        Field::DownloadRoot => {
            draft.download_root = if value == "none" {
                None
            } else {
                let path = PathBuf::from(value);
                if !path.is_absolute() || !draft.model_roots.contains(&path) || !path.is_dir() {
                    return Err(invalid("Choose an existing configured model root or none"));
                }
                Some(path)
            };
        }
        Field::AddRoot => {
            let path = if value == "default" {
                let path = default_model_root()?;
                // Explicit confirmation in the editor is required to create this directory.
                std::fs::create_dir_all(&path)?;
                path
            } else {
                PathBuf::from(value)
            };
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
            if draft.download_root.as_ref() == Some(&path) {
                draft.download_root = None;
            }
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
