//! Owned single-model server process. Never signal processes we did not spawn.
use super::client::endpoint;
use crate::models::profile::Profile;
use std::{
    collections::VecDeque,
    ffi::OsString,
    io::{self, BufRead, BufReader, Read},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

pub fn command_output(binary: &str, args: &[&str]) -> io::Result<String> {
    let out = tempfile::tempfile()?;
    let err = out.try_clone()?;
    let mut child = Command::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out.try_clone()?))
        .stderr(Stdio::from(err))
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                return Err(io::Error::other("Executable probe failed"));
            }
            use std::io::{Seek, SeekFrom};
            let mut out = out;
            out.seek(SeekFrom::Start(0))?;
            let mut bytes = Vec::new();
            out.take(1024 * 1024).read_to_end(&mut bytes)?;
            return Ok(String::from_utf8_lossy(&bytes).into_owned());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Executable probe timed out",
            ));
        }
        thread::sleep(Duration::from_millis(40));
    }
}
pub fn endpoint_args(endpoint_value: &str) -> io::Result<Vec<OsString>> {
    let url = endpoint(endpoint_value)?;
    if url.scheme() != "http"
        || !matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Managed servers require a local HTTP endpoint",
        ));
    }
    let port = url.port_or_known_default().unwrap_or(80);
    let host = if url.host_str() == Some("[::1]") {
        "::1"
    } else {
        "127.0.0.1"
    };
    Ok(vec![
        "--host".into(),
        host.into(),
        "--port".into(),
        port.to_string().into(),
    ])
}
pub fn args(
    path: &Path,
    endpoint_value: &str,
    profile: &Profile,
    help: &str,
) -> io::Result<Vec<OsString>> {
    let mut args = vec!["--model".into(), path.as_os_str().to_owned()];
    args.extend(endpoint_args(endpoint_value)?);
    args.extend(["--alias".into(), "llamancer".into()]);
    args.extend(profile_args(profile, help)?);
    Ok(args)
}
pub fn profile_args(profile: &Profile, help: &str) -> io::Result<Vec<OsString>> {
    profile.validate()?;
    let mut a = Vec::new();
    let e = &profile.engine;
    let g = &profile.generation;
    let mut add = |flag: &str, value: Option<String>| -> io::Result<()> {
        if let Some(value) = value {
            if !help
                .split(|c: char| c.is_whitespace() || c == ',' || c == '=')
                .any(|s| s == flag)
            {
                return Err(io::Error::other(format!(
                    "Installed llama-server does not advertise {flag}"
                )));
            }
            a.push(flag.into());
            a.push(value.into());
        }
        Ok(())
    };
    macro_rules! option {
        ($flag:literal,$value:expr) => {
            add($flag, $value.map(|v| v.to_string()))?
        };
    }
    option!("--ctx-size", e.context_size);
    option!("--n-gpu-layers", e.gpu_layers);
    option!("--threads", e.threads);
    option!("--batch-size", e.batch_size);
    option!("--ubatch-size", e.ubatch_size);
    // Current builds accept on/off/auto; older boolean-only builds are rejected rather than guessed.
    if e.flash_attention.is_some() && !help.contains("on|off|auto") {
        return Err(io::Error::other(
            "Flash attention option requires a compatible llama-server build",
        ));
    }
    add(
        "--flash-attn",
        e.flash_attention
            .map(|v| if v { "on" } else { "off" }.into()),
    )?;
    add("--cache-type-k", e.cache_type_k.clone())?;
    add("--cache-type-v", e.cache_type_v.clone())?;
    if let Some(path) = &e.projector {
        if !Path::new(path).is_file() {
            return Err(io::Error::other("Configured projector file is unavailable"));
        }
    }
    add("--mmproj", e.projector.clone())?;
    option!("--temp", g.temperature);
    option!("--top-p", g.top_p);
    option!("--top-k", g.top_k);
    option!("--min-p", g.min_p);
    option!("--repeat-penalty", g.repeat_penalty);
    option!("--seed", g.seed);
    if help.contains("--jinja") {
        a.push("--jinja".into());
    }
    Ok(a)
}
pub struct Owned {
    child: Child,
    preset: Option<tempfile::NamedTempFile>,
    key_file: Option<tempfile::NamedTempFile>,
    logs: Arc<Mutex<VecDeque<String>>>,
}
impl Owned {
    pub fn start(binary: &str, args: &[OsString]) -> io::Result<Self> {
        let mut cmd = Command::new(binary);
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Ignore inherited LLAMA_ARG settings; profiles are the authority for managed processes.
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("LLAMA_ARG_") {
                cmd.env_remove(key);
            }
        }
        cmd.env_remove("LLAMA_API_KEY");
        let mut child = cmd.spawn()?;
        let logs = Arc::new(Mutex::new(VecDeque::new()));
        fn drain(stream: impl Read + Send + 'static, logs: Arc<Mutex<VecDeque<String>>>) {
            thread::spawn(move || {
                let mut reader = BufReader::new(stream);
                loop {
                    let mut line = Vec::new();
                    let Ok(n) = reader.by_ref().take(4096).read_until(b'\n', &mut line) else {
                        break;
                    };
                    if n == 0 {
                        break;
                    }
                    let clean = String::from_utf8_lossy(&line)
                        .chars()
                        .filter(|c| !c.is_control())
                        .collect();
                    if let Ok(mut buffer) = logs.lock() {
                        if buffer.len() >= 200 {
                            buffer.pop_front();
                        }
                        buffer.push_back(clean);
                    }
                }
            });
        }
        if let Some(s) = child.stdout.take() {
            drain(s, Arc::clone(&logs));
        }
        if let Some(s) = child.stderr.take() {
            drain(s, Arc::clone(&logs));
        }
        Ok(Self {
            child,
            logs,
            preset: None,
            key_file: None,
        })
    }
    pub fn keep_key_file(&mut self, file: tempfile::NamedTempFile) {
        self.key_file = Some(file);
    }
    pub fn keep_preset(&mut self, file: tempfile::NamedTempFile) {
        self.preset = Some(file);
    }
    pub fn running(&mut self) -> io::Result<bool> {
        Ok(self.child.try_wait()?.is_none())
    }
    pub fn logs(&self) -> Vec<String> {
        self.logs
            .lock()
            .map(|l| l.iter().cloned().collect())
            .unwrap_or_default()
    }
    pub fn stop(&mut self) -> io::Result<()> {
        if self.running()? {
            let group = format!("-{}", self.child.id());
            let _ = Command::new("kill").args(["-TERM", "--", &group]).status();
            let deadline = Instant::now() + Duration::from_secs(2);
            while self.running()? && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(50));
            }
            if self.running()? {
                let _ = Command::new("kill").args(["-KILL", "--", &group]).status();
                if self.running()? {
                    self.child.kill()?;
                }
                self.child.wait()?;
            }
        }
        Ok(())
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_remote_and_unsupported_flags() {
        let mut p = Profile::default();
        p.engine.context_size = Some(4096);
        assert!(args(
            Path::new("model.gguf"),
            "http://example.com:8080",
            &p,
            "--ctx-size"
        )
        .is_err());
        assert!(args(Path::new("model.gguf"), "http://localhost:8080", &p, "").is_err());
        let a = args(
            Path::new("model with spaces.gguf"),
            "http://localhost:8080",
            &p,
            "--ctx-size --jinja",
        )
        .unwrap();
        assert_eq!(a[1], "model with spaces.gguf");
        assert!(a.iter().any(|s| s == "4096"));
    }
}
