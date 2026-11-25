//! Non-invasive coding-agent launch plans; no shell interpolation or config rewrites.
use crate::llama::client::endpoint;
use std::{ffi::OsString, io, path::PathBuf, process::Command};
#[derive(Debug, Clone)]
pub struct LaunchPlan {
    pub executable: String,
    pub args: Vec<OsString>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
}
impl LaunchPlan {
    pub fn command(&self) -> Command {
        let mut cmd = Command::new(&self.executable);
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
    pub fn terminal_command(&self, mode: TerminalMode) -> Command {
        if mode == TerminalMode::Current {
            return self.command();
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
        for (key, value) in &self.env {
            cmd.arg("--env").arg(format!("{key}={value}"));
        }
        cmd.arg("--").arg(&self.executable).args(&self.args);
        cmd
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
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
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
        let cmd = plan.terminal_command(TerminalMode::KittyTab);
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
