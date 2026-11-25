//! Non-invasive coding-agent launch plans; no shell interpolation or config rewrites.
use crate::llama::client::endpoint;
use std::{ffi::OsString, io, path::PathBuf, process::Command, sync::Arc};
// Clear endpoint-specific inherited credentials before injecting the selected
// session's values. Other agent credentials remain the user's responsibility.
const SESSION_KEYS: [&str; 4] = [
    "LLAMA_CPP_API_KEY",
    "LLAMA_API_KEY",
    "LLAMANCER_PI_KEY",
    "LLAMANCER_API_KEY",
];
#[derive(Clone)]
pub struct LaunchPlan {
    pub executable: String,
    pub args: Vec<OsString>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
    // Only foreground launches may own a temporary agent profile: it must
    // remain alive until the child exits, not just until a Kitty RPC returns.
    omp_profile: Option<Arc<OmpProfile>>,
}
struct OmpProfile {
    root: tempfile::TempDir,
}
impl OmpProfile {
    fn path(&self) -> &std::path::Path {
        self.root.path()
    }
    fn stop_daemons(&self) -> io::Result<()> {
        use std::{
            io::Read,
            os::fd::{AsRawFd, FromRawFd, OwnedFd},
            os::unix::fs::MetadataExt,
        };
        let daemon = self.path().join("daemon");
        let marker = format!("OMP_DAEMON_DIR={}", daemon.to_string_lossy());
        let matches = |p: &std::path::Path| -> bool {
            if !p
                .metadata()
                .is_ok_and(|m| m.uid() == unsafe { libc::geteuid() })
                || !std::fs::read_to_string(p.join("comm")).is_ok_and(|s| s.trim() == "omp daemon")
            {
                return false;
            }
            let Ok(file) = std::fs::File::open(p.join("environ")) else {
                return false;
            };
            let mut bytes = Vec::new();
            file.take(2 * 1024 * 1024).read_to_end(&mut bytes).is_ok()
                && bytes.split(|b| *b == 0).any(|v| v == marker.as_bytes())
        };
        for entry in std::fs::read_dir("/proc")?.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|s| s.parse::<i32>().ok())
            else {
                continue;
            };
            let path = entry.path();
            if !matches(&path) {
                continue;
            }
            // Keep the process identity pinned before checking it again. Never
            // signal a reused numeric PID or another OMP profile.
            let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
            if fd < 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::ESRCH) {
                    continue;
                }
                return Err(io::Error::other(
                    "Cannot safely stop temporary OMP daemon; pidfd unavailable",
                ));
            }
            let fd = unsafe { OwnedFd::from_raw_fd(fd as i32) };
            if !matches(&path) {
                continue;
            }
            let signal = |signal: i32| {
                let status = unsafe {
                    libc::syscall(
                        libc::SYS_pidfd_send_signal,
                        fd.as_raw_fd(),
                        signal,
                        std::ptr::null::<libc::siginfo_t>(),
                        0,
                    )
                };
                if status < 0 {
                    let error = io::Error::last_os_error();
                    if error.raw_os_error() != Some(libc::ESRCH) {
                        return Err(error);
                    }
                }
                Ok(())
            };
            let exited = |timeout| -> io::Result<bool> {
                let mut poll = libc::pollfd {
                    fd: fd.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                let result = unsafe { libc::poll(&mut poll, 1, timeout) };
                if result < 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(result > 0)
                }
            };
            signal(libc::SIGTERM)?;
            if !exited(3000)? {
                signal(libc::SIGKILL)?;
                if !exited(1000)? {
                    return Err(io::Error::other("Temporary OMP daemon did not exit"));
                }
            }
        }
        Ok(())
    }
}
impl Drop for OmpProfile {
    fn drop(&mut self) {
        let _ = self.stop_daemons();
    }
}
impl LaunchPlan {
    /// OMP's explicit models.yml and stored login keys take precedence over
    /// its implicit provider environment. Keep authenticated native sessions
    /// in a fresh private profile rather than editing or trusting those files.
    fn isolate_authenticated_omp(&mut self) -> io::Result<()> {
        if self.executable != "omp" {
            return Err(io::Error::other("OMP profile requested for another agent"));
        }
        let base = self
            .env
            .iter()
            .find(|(k, _)| k == "LLAMA_CPP_BASE_URL")
            .map(|(_, v)| v.as_str())
            .ok_or_else(|| io::Error::other("Missing OMP endpoint"))?;
        let base = endpoint(base)?.to_string();
        use std::io::Write;
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
        let profile = tempfile::Builder::new()
            .prefix("llamancer-omp-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()?;
        let daemon = profile.path().join("daemon");
        std::fs::DirBuilder::new().mode(0o700).create(&daemon)?;
        let write = |name: &str, bytes: &[u8]| -> io::Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(profile.path().join(name))?;
            file.write_all(bytes)
        };
        // JSON is a YAML subset. Only the selected endpoint and an environment
        // variable name are persisted; never the actual inference credential.
        let provider = serde_json::json!({"providers":{"llama.cpp":{
            "baseUrl":base,"api":"openai-responses",
            "apiKey":"LLAMA_CPP_API_KEY","authHeader":true,"auth":"apiKey",
            "discovery":{"type":"llama.cpp","timeoutMs":10000}
        }}});
        write(
            "models.yml",
            &serde_json::to_vec(&provider).map_err(io::Error::other)?,
        )?;
        write(
            "config.yml",
            b"startup:\n  setupWizard: false\nmodels:\n  offline: true\n",
        )?;
        let path = profile
            .path()
            .to_str()
            .ok_or_else(|| io::Error::other("Non-UTF8 OMP profile path"))?
            .to_owned();
        self.env.retain(|(k, _)| k != "PI_CODING_AGENT_DIR");
        self.env.push(("PI_CODING_AGENT_DIR".into(), path));
        self.env.push((
            "OMP_DAEMON_DIR".into(),
            daemon
                .to_str()
                .ok_or_else(|| io::Error::other("Non-UTF8 daemon path"))?
                .into(),
        ));
        self.args.push("--offline".into());
        self.omp_profile = Some(Arc::new(OmpProfile { root: profile }));
        Ok(())
    }
    pub fn with_key(&mut self, key: &crate::llama::auth::Key, agent: &str) -> io::Result<()> {
        if agent == "omp" && self.omp_profile.is_none() {
            self.isolate_authenticated_omp()?;
        }
        if agent == "pi-native" {
            return Err(io::Error::other(
                "Authenticated Pi launches require the scoped pi adapter",
            ));
        }
        let variable = match agent {
            "omp" => "LLAMA_CPP_API_KEY",
            "pi" => "LLAMANCER_PI_KEY",
            _ => "LLAMANCER_API_KEY",
        };
        self.env.retain(|(name, _)| name != variable);
        self.env.push((variable.into(), key.expose().into()));
        Ok(())
    }
    pub fn finish(&self) -> io::Result<()> {
        self.omp_profile
            .as_ref()
            .map_or(Ok(()), |profile| profile.stop_daemons())
    }
    pub fn validate(&self, mode: TerminalMode) -> io::Result<()> {
        if !self.cwd.is_dir() {
            return Err(io::Error::other("Agent project directory is unavailable"));
        }
        require_executable(&self.executable, &self.cwd)?;
        if mode != TerminalMode::Current {
            if self.omp_profile.is_some() {
                return Err(io::Error::other(
                    "Temporary OMP profile requires current terminal (A)",
                ));
            }
            require_executable("kitten", &self.cwd)?;
            if self.env.iter().any(|(name, value)| {
                name.ends_with("_API_KEY") || (name == "LLAMANCER_PI_KEY" && value != "local")
            }) {
                return Err(io::Error::other(
                    "Authenticated agent launch requires current terminal (A)",
                ));
            }
        }
        Ok(())
    }
    pub fn command(&self) -> Command {
        let mut cmd = Command::new(&self.executable);
        for key in SESSION_KEYS {
            cmd.env_remove(key);
        }
        if self.omp_profile.is_some() {
            for key in [
                "OMP_PROFILE",
                "PI_CONFIG_DIR",
                "PI_CODING_AGENT_DIR",
                "OMP_AUTH_BROKER_URL",
                "OMP_AUTH_BROKER_TOKEN",
                "OMP_DAEMON_DIR",
                "OMP_DAEMON_TCP",
                "OMP_DAEMON_RESUME",
                "OMP_DAEMON_IDLE_EXIT_GRACE_MS",
                "OMP_DAEMON_RESUME_LEASE_GRACE_MS",
            ] {
                cmd.env_remove(key);
            }
        }
        cmd.args(&self.args)
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .current_dir(&self.cwd);
        cmd
    }
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalMode {
    #[default]
    Current,
    KittyTab,
    KittyWindow,
}
impl TerminalMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::KittyTab => "kitty-tab",
            Self::KittyWindow => "kitty-window",
        }
    }
}
impl std::str::FromStr for TerminalMode {
    type Err = io::Error;
    fn from_str(s: &str) -> io::Result<Self> {
        match s {
            "current" => Ok(Self::Current),
            "kitty-tab" => Ok(Self::KittyTab),
            "kitty-window" => Ok(Self::KittyWindow),
            _ => Err(io::Error::other(
                "Choose current, kitty-tab or kitty-window",
            )),
        }
    }
}
impl LaunchPlan {
    pub fn terminal_command(&self, mode: TerminalMode) -> io::Result<Command> {
        if mode == TerminalMode::Current {
            return Ok(self.command());
        }
        if self.omp_profile.is_some() {
            return Err(io::Error::other(
                "Temporary OMP profile requires current terminal (A)",
            ));
        }
        if self.env.iter().any(|(name, value)| {
            name.ends_with("_API_KEY") || (name == "LLAMANCER_PI_KEY" && value != "local")
        }) {
            return Err(io::Error::other(
                "Authenticated agent launch requires current terminal (A)",
            ));
        }
        let mut cmd = Command::new("kitten");
        cmd.args([
            "@",
            "launch",
            if mode == TerminalMode::KittyTab {
                "--type=tab"
            } else {
                "--type=os-window"
            },
            "--cwd",
        ])
        .arg(&self.cwd);
        for key in SESSION_KEYS {
            cmd.arg("--env").arg(key);
        }
        for (key, value) in &self.env {
            cmd.arg("--env").arg(format!("{key}={value}"));
        }
        cmd.arg("--").arg(&self.executable).args(&self.args);
        Ok(cmd)
    }
}
fn require_executable(name: &str, cwd: &std::path::Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let executable = |p: &std::path::Path| {
        p.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    let path = std::path::Path::new(name);
    let found = if name.contains('/') {
        executable(&cwd.join(path))
    } else {
        std::env::var_os("PATH").is_some_and(|paths| {
            std::env::split_paths(&paths).any(|dir| executable(&cwd.join(dir).join(name)))
        })
    };
    if found {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Executable {name} not found or not executable; install it or update Settings"),
        ))
    }
}
/// Pi's documented extension registration, with a provider unique to the selected
/// endpoint/model. No edits to the user's models.json, settings.json or auth.json.
pub struct Pi<'a> {
    pub root: &'a std::path::Path,
    pub context: u64,
}
impl AgentAdapter for Pi<'_> {
    fn prepare(&self, base: &str, model: &str, cwd: PathBuf) -> io::Result<LaunchPlan> {
        use sha2::{Digest, Sha256};
        use std::io::Write;
        if model.is_empty()
            || model.chars().any(char::is_control)
            || self.context < 128
            || self.context > u32::MAX as u64
        {
            return Err(io::Error::other("Invalid Pi model or context limit"));
        }
        let base = endpoint(base)?.to_string();
        let hash = Sha256::digest(format!("{base}\n{model}"));
        let provider = format!("llamancer-local-{}", &format!("{hash:x}")[..16]);
        let root = self.root.join("integrations");
        std::fs::create_dir_all(&root)?;
        let extension = root.join("pi-provider.js");
        let mut file = tempfile::NamedTempFile::new_in(&root)?;
        file.write_all(include_bytes!("pi.js"))?;
        file.as_file().sync_all()?;
        file.persist(&extension).map_err(|e| e.error)?;
        Ok(LaunchPlan {
            executable: "pi".into(),
            args: vec![
                "--extension".into(),
                extension.into_os_string(),
                "--model".into(),
                format!("{provider}/{model}").into(),
            ],
            env: vec![
                ("LLAMANCER_ENDPOINT".into(), base),
                ("LLAMANCER_MODEL".into(), model.into()),
                ("LLAMANCER_PROVIDER".into(), provider),
                ("LLAMANCER_CONTEXT".into(), self.context.to_string()),
                ("LLAMANCER_PI_KEY".into(), "local".into()),
            ],
            cwd,
            omp_profile: None,
        })
    }
}
pub trait AgentAdapter {
    fn prepare(&self, endpoint: &str, model: &str, cwd: PathBuf) -> io::Result<LaunchPlan>;
}
pub struct Native(pub &'static str);
impl AgentAdapter for Native {
    fn prepare(&self, base: &str, model: &str, cwd: PathBuf) -> io::Result<LaunchPlan> {
        let base = endpoint(base)?.to_string();
        if model.is_empty() || model.chars().any(char::is_control) {
            return Err(io::Error::other("Invalid model ID"));
        }
        let var = match self.0 {
            "omp" => "LLAMA_CPP_BASE_URL",
            "pi" => "LLAMA_BASE_URL",
            _ => return Err(io::Error::other("Unknown native agent")),
        };
        Ok(LaunchPlan {
            executable: self.0.into(),
            args: vec!["--model".into(), format!("llama.cpp/{model}").into()],
            env: vec![(var.into(), base)],
            cwd,
            omp_profile: None,
        })
    }
}
pub struct Custom<'a> {
    pub executable: &'a str,
    pub args: &'a [String],
}
impl AgentAdapter for Custom<'_> {
    fn prepare(&self, base: &str, model: &str, cwd: PathBuf) -> io::Result<LaunchPlan> {
        if self.executable.is_empty() {
            return Err(io::Error::other(
                "Configure a custom agent executable first",
            ));
        }
        let base = endpoint(base)?.to_string();
        Ok(LaunchPlan {
            executable: self.executable.into(),
            args: self.args.iter().map(OsString::from).collect(),
            env: vec![
                ("LLAMANCER_ENDPOINT".into(), base),
                ("LLAMANCER_MODEL".into(), model.into()),
            ],
            cwd,
            omp_profile: None,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authenticated_omp_profile_is_private_scoped_and_temporary() {
        use std::os::unix::fs::PermissionsExt;
        let mut plan = Native("omp")
            .prepare("http://localhost:8080", "fixture", std::env::temp_dir())
            .unwrap();
        plan.isolate_authenticated_omp().unwrap();
        let path = plan.omp_profile.as_ref().unwrap().path().to_owned();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let models = std::fs::read_to_string(path.join("models.yml")).unwrap();
        assert!(models.contains("http://localhost:8080/"));
        assert!(models.contains("LLAMA_CPP_API_KEY"));
        assert!(!models.contains("fixture-secret"));
        plan.env
            .push(("LLAMA_CPP_API_KEY".into(), "fixture-secret".into()));
        let command = plan.command();
        for key in [
            "OMP_PROFILE",
            "PI_CONFIG_DIR",
            "OMP_AUTH_BROKER_URL",
            "OMP_AUTH_BROKER_TOKEN",
        ] {
            assert!(command.get_envs().any(|(k, v)| k == key && v.is_none()));
        }
        assert!(plan.terminal_command(TerminalMode::KittyTab).is_err());
        assert!(!std::fs::read_to_string(path.join("config.yml"))
            .unwrap()
            .contains("fixture-secret"));
        let copy = plan.clone();
        drop(plan);
        assert!(path.is_dir());
        drop(copy);
        assert!(!path.exists());
    }
    #[test]
    fn command_boundary_rejects_secrets_in_kitty_and_clears_inherited_keys() {
        let mut plan = Native("omp")
            .prepare("http://localhost:8080", "fixture", std::env::temp_dir())
            .unwrap();
        let cmd = plan.command();
        for key in SESSION_KEYS {
            assert!(cmd.get_envs().any(|(k, v)| k == key && v.is_none()));
        }
        plan.env
            .push(("LLAMA_CPP_API_KEY".into(), "fixture-only".into()));
        assert!(plan.terminal_command(TerminalMode::KittyTab).is_err());
        assert!(plan.terminal_command(TerminalMode::KittyWindow).is_err());
        assert!(plan.terminal_command(TerminalMode::Current).is_ok());
    }
    #[test]
    fn kitty_preserves_argv_without_shell() {
        let plan = Custom {
            executable: "my-agent",
            args: &["a b".into(), "; literal".into()],
        }
        .prepare(
            "http://localhost:8080",
            "model",
            PathBuf::from("/tmp/project with spaces"),
        )
        .unwrap();
        let cmd = plan.terminal_command(TerminalMode::KittyTab).unwrap();
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(cmd.get_program(), "kitten");
        assert!(args.contains(&"--type=tab".into()));
        assert!(args.ends_with(&["my-agent".into(), "a b".into(), "; literal".into()]));
        assert!(args.contains(&"/tmp/project with spaces".into()));
    }
    #[test]
    fn pi_registration_is_scoped_and_never_contains_a_token() {
        let root = tempfile::tempdir().unwrap();
        let adapter = Pi {
            root: root.path(),
            context: 4096,
        };
        let plan = adapter
            .prepare("http://localhost:8080", "test", root.path().into())
            .unwrap();
        assert_eq!(plan.args[0], "--extension");
        assert!(std::path::Path::new(&plan.args[1]).is_file());
        assert!(plan.args[3]
            .to_string_lossy()
            .starts_with("llamancer-local-"));
        assert_eq!(
            plan.env
                .iter()
                .find(|(k, _)| k == "LLAMANCER_CONTEXT")
                .unwrap()
                .1,
            "4096"
        );
        assert!(adapter
            .prepare("http://localhost:8080", "bad\nmodel", root.path().into())
            .is_err());
        assert!(require_executable("/nonexistent/agent", root.path()).is_err());
        assert!(require_executable("/bin/sh", root.path()).is_ok());
    }
    #[test]
    fn native_overrides_do_not_modify_files() {
        let omp = Native("omp")
            .prepare("http://localhost:8080", "model", PathBuf::from("/tmp"))
            .unwrap();
        assert_eq!(omp.args[1], "llama.cpp/model");
        assert_eq!(omp.env[0].0, "LLAMA_CPP_BASE_URL");
        assert_eq!(
            Native("pi")
                .prepare("http://localhost:8080", "model", PathBuf::from("/tmp"))
                .unwrap()
                .env[0]
                .0,
            "LLAMA_BASE_URL"
        );
    }
}
