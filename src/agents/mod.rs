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
