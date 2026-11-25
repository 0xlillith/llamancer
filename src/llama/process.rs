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
    reaped: bool,
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
            reaped: false,
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
        if self.reaped {
            return Ok(false);
        }
        // WNOWAIT observes exit without reaping the leader. Its PID/PGID stays
        // reserved until group cleanup, preventing a recycled group from being signaled.
        loop {
            let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
            // SAFETY: live owned child ID, writable siginfo, valid waitid flags.
            let result = unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.child.id(),
                    info.as_mut_ptr(),
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if result == 0 {
                // SAFETY: waitid succeeded and the zeroed result has si_pid=0 if no event.
                return Ok(unsafe { info.assume_init().si_pid() } == 0);
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }
    fn signal_group(&self, signal: libc::c_int) -> io::Result<()> {
        if self.reaped {
            return Ok(());
        }
        let group = i32::try_from(self.child.id()).map_err(io::Error::other)?;
        // SAFETY: process_group(0) made the unreaped owned child its own group leader.
        if unsafe { libc::kill(-group, signal) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(error)
        }
    }
    pub fn logs(&self) -> Vec<String> {
        self.logs
            .lock()
            .map(|l| l.iter().cloned().collect())
            .unwrap_or_default()
    }
    pub fn stop(&mut self) -> io::Result<()> {
        if self.reaped {
            return Ok(());
        }
        // Even an exited leader may have live router workers in its group.
        self.signal_group(libc::SIGTERM)?;
        let deadline = Instant::now() + Duration::from_secs(2);
        while self.running()? && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(50));
        }
        // Clean up remaining descendants before releasing the leader PID.
        self.signal_group(libc::SIGKILL)?;
        self.child.wait()?;
        self.reaped = true;
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
    fn exited_leader_does_not_leave_owned_worker_alive() {
        let root = tempfile::tempdir().unwrap();
        let heartbeat = root.path().join("heartbeat");
        let worker = root.path().join("worker.pid");
        // The parent exits while its background descendant keeps writing.
        let script =
            "(while :; do printf x >> \"$1\"; sleep 0.05; done) & echo $! > \"$2\"; exit 0";
        let args = vec![
            "-c".into(),
            script.into(),
            "fixture".into(),
            heartbeat.as_os_str().into(),
            worker.as_os_str().into(),
        ];
        let mut owned = Owned::start("/bin/sh", &args).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while owned.running().unwrap() || !heartbeat.exists() || !worker.exists() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(20));
        }
        owned.stop().unwrap();
        // Repeated stop is idempotent, and no background writes remain.
        owned.stop().unwrap();
        let size = std::fs::metadata(&heartbeat).unwrap().len();
        thread::sleep(Duration::from_millis(200));
        assert_eq!(std::fs::metadata(heartbeat).unwrap().len(), size);
        assert!(!owned.running().unwrap());
    }
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
