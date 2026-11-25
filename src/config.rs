//! Versioned application settings and atomic persistence.
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub schema_version: u32,
    pub theme: String,
    pub model_roots: Vec<PathBuf>,
    pub default_agent: String,
    pub endpoint: String,
    pub server_binary: String,
    pub custom_agent: String,
    pub custom_agent_args: Vec<String>,
    pub project_dir: Option<PathBuf>,
    pub favorites: BTreeSet<PathBuf>,
    pub profiles: BTreeMap<PathBuf, crate::models::profile::Profile>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: 1,
            theme: "dracula".into(),
            model_roots: Vec::new(),
            default_agent: "omp".into(),
            endpoint: "http://127.0.0.1:8080".into(),
            server_binary: "llama-server".into(),
            custom_agent: String::new(),
            custom_agent_args: Vec::new(),
            project_dir: None,
            favorites: BTreeSet::new(),
            profiles: BTreeMap::new(),
        }
    }
}

pub fn config_dir() -> io::Result<PathBuf> {
    if let Some(root) = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return Ok(root.join("llamancer"));
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is unavailable"))?;
    Ok(home.join(".config/llamancer"))
}

impl Config {
    pub fn load(path: &Path) -> io::Result<Self> {
        let contents = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e),
        };
        let config: Self =
            toml::from_str(&contents).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if config.schema_version != 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unsupported configuration schema",
            ));
        }
        for profile in config.profiles.values() {
            profile.validate()?;
        }
        Ok(config)
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        use std::io::Write;
        for profile in self.profiles.values() {
            profile.validate()?;
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;
        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        let contents = toml::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        tmp.write_all(contents.as_bytes())?;
        tmp.as_file().sync_all()?;
        tmp.persist(path).map_err(|e| e.error)?;
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut c = Config::load(&path).unwrap();
        c.model_roots.push("/models".into());
        c.favorites.insert("/models/favorite.gguf".into());
        let mut profile = crate::models::profile::Profile::default();
        profile.engine.context_size = Some(32768);
        profile.generation.temperature = Some(0.2);
        c.profiles.insert("/models/favorite.gguf".into(), profile);
        c.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap(), c);
    }
    #[test]
    fn rejects_bad_schema_and_preserves_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "schema_version = 99").unwrap();
        assert!(Config::load(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "schema_version = 99");
    }
}
