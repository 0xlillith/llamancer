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
    pub download_root: Option<PathBuf>,
    pub model_imports: Vec<PathBuf>,
    pub hidden_models: BTreeSet<PathBuf>,
    pub default_agent: String,
    pub agent_terminal: crate::agents::TerminalMode,
    pub endpoint: String,
    pub api_key_file: Option<PathBuf>,
    pub server_binary: String,
    pub device: String,
    pub custom_agent: String,
    pub custom_agent_args: Vec<String>,
    pub project_dir: Option<PathBuf>,
    pub workspace_root: Option<PathBuf>,
    pub favorites: BTreeSet<PathBuf>,
    pub profiles: BTreeMap<PathBuf, crate::models::profile::Profile>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: 1,
            theme: "dracula".into(),
            model_roots: Vec::new(),
            download_root: None,
            model_imports: Vec::new(),
            hidden_models: BTreeSet::new(),
            default_agent: "omp".into(),
            agent_terminal: Default::default(),
            endpoint: "http://127.0.0.1:8080".into(),
            api_key_file: None,
            server_binary: "llama-server".into(),
            device: "auto".into(),
            custom_agent: String::new(),
            custom_agent_args: Vec::new(),
            project_dir: None,
            workspace_root: None,
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

/// Read bounded, ordinary local state files without blocking on substituted
/// FIFOs or following a final-component symlink. User-created regular files
/// may be world-readable, but must not be writable by other users.
pub(crate) const MAX_STATE_BYTES: u64 = 16 * 1024 * 1024;
pub(crate) fn read_state(path: &Path, limit: u64) -> io::Result<String> {
    use std::{
        io::Read,
        os::unix::fs::{MetadataExt, OpenOptionsExt},
    };
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.nlink() != 1
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o022 != 0
    {
        return Err(io::Error::other(
            "Unsafe local state file; use a regular user-owned file not writable by others",
        ));
    }
    if meta.len() > limit {
        return Err(io::Error::other("Local state exceeds size limit"));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::other("Local state exceeds size limit"));
    }
    String::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Local state is not UTF-8"))
}
pub(crate) fn check_state_size(contents: &str) -> io::Result<()> {
    if contents.len() as u64 > MAX_STATE_BYTES {
        return Err(io::Error::other("Local state exceeds size limit"));
    }
    Ok(())
}

impl Config {
    pub fn scan_roots(&self) -> Vec<PathBuf> {
        self.model_roots
            .iter()
            .chain(&self.model_imports)
            .cloned()
            .collect()
    }
    pub fn filter_groups(
        &self,
        groups: Vec<crate::models::group::Group>,
    ) -> Vec<crate::models::group::Group> {
        groups
            .into_iter()
            .filter(|g| {
                !self
                    .hidden_models
                    .contains(&crate::models::library::identity(g))
            })
            .collect()
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        let contents = match read_state(path, MAX_STATE_BYTES) {
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
        crate::llama::client::endpoint(&config.endpoint)?;
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
        check_state_size(&contents)?;
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
    fn state_rejects_symlinks_fifos_and_oversized_files() {
        use std::{
            ffi::CString,
            os::unix::{
                ffi::OsStrExt,
                fs::{symlink, PermissionsExt},
            },
        };
        let dir = tempfile::tempdir().unwrap();
        let regular = dir.path().join("regular");
        fs::write(&regular, "fixture").unwrap();
        assert_eq!(read_state(&regular, 32).unwrap(), "fixture");
        assert!(read_state(&regular, 3).is_err());
        let link = dir.path().join("link");
        symlink(&regular, &link).unwrap();
        assert!(read_state(&link, 32).is_err());
        let fifo = dir.path().join("fifo");
        let name = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(read_state(&fifo, 32).is_err());
        fs::set_permissions(&regular, fs::Permissions::from_mode(0o666)).unwrap();
        assert!(read_state(&regular, 32).is_err());
    }
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
