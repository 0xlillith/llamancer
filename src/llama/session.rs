//! UI-independent server controller. Network and discovery run on a bounded worker.
use super::{
    client::{Client, Snapshot},
    process::{self, Owned},
    router,
};
use crate::{
    config::Config,
    jobs::Job,
    models::{group::Group, library::identity},
};
use std::{
    io,
    net::TcpListener,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};
enum Outcome {
    Started(Owned, String),
    Snapshot(Snapshot),
    Changed,
    Probed(String, String),
}
#[derive(Default)]
pub struct Controller {
    owned: Option<Owned>,
    pending: Option<Job<Outcome>>,
    last_refresh: Option<Instant>,
    startup_deadline: Option<Instant>,
    attached: bool,
    failures: u32,
    selected_id: Option<String>,
    single: Option<Group>,
    pub generation: u64,
    pub active_context: Option<u32>,
    router_contexts: std::collections::BTreeMap<String, Option<u32>>,
    discard_pending: bool,
    pub snapshot: Option<Snapshot>,
    pub cursor: usize,
    pub status: String,
    pub version: String,
    pub devices: String,
    pub logs: Vec<String>,
    pub show_logs: bool,
}
impl Controller {
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn owns_server(&self) -> bool {
        self.owned.is_some()
    }
    pub fn is_single_model(&self, group: &Group) -> bool {
        self.owns_server()
            && self
                .single
                .as_ref()
                .is_some_and(|g| identity(g) == identity(group))
    }
    pub fn router_context_matches(&self, g: &Group, config: &Config) -> bool {
        self.router_contexts
            .get(&router::id(g))
            .is_some_and(|c| *c == config.effective_profile(g).engine.context_size)
    }
    pub fn active(&self) -> bool {
        self.busy() || self.owns_server()
    }
    pub fn polling(&self) -> bool {
        self.active() || self.attached
    }
    pub fn disconnect(&mut self) -> io::Result<()> {
        if self.active() {
            return Err(io::Error::other(
                "Stop owned server or wait for current request",
            ));
        }
        self.attached = false;
        self.snapshot = None;
        self.status = "Detached from external endpoint".into();
        self.selected_id = None;
        Ok(())
    }
    pub fn selected(&self) -> Option<&super::client::Model> {
        self.snapshot.as_ref()?.models.get(self.cursor)
    }
    pub fn select_id(&mut self, id: &str) -> io::Result<()> {
        self.cursor = self
            .snapshot
            .as_ref()
            .and_then(|s| s.models.iter().position(|m| m.id == id))
            .ok_or_else(|| {
                io::Error::other("Model not registered; restart router after scanning")
            })?;
        Ok(())
    }
    pub fn move_cursor(&mut self, down: bool) {
        let max = self
            .snapshot
            .as_ref()
            .map(|s| s.models.len().saturating_sub(1))
            .unwrap_or(0);
        self.cursor = if down {
            (self.cursor + 1).min(max)
        } else {
            self.cursor.saturating_sub(1)
        };
    }
    fn submit(
        &mut self,
        task: impl FnOnce(std::sync::Arc<std::sync::atomic::AtomicBool>) -> io::Result<Outcome>
            + Send
            + 'static,
    ) -> io::Result<()> {
        if self.busy() {
            return Err(io::Error::other("Server operation in progress"));
        }
        self.pending = Some(Job::start(task)?);
        self.discard_pending = false;
        Ok(())
    }
    pub fn refresh(&mut self, config: &Config) -> io::Result<()> {
        let client = Client::from_config(config)?;
        self.submit(move |_| client.snapshot().map(Outcome::Snapshot))?;
        self.attached = true;
        self.last_refresh = Some(Instant::now());
        self.status = "Checking server…".into();
        Ok(())
    }
    pub fn probe(&mut self, config: &Config) -> io::Result<()> {
        let binary = config.server_binary.clone();
        if !self.busy() {
            self.devices.clear();
        }
        self.submit(move |_| {
            let version = process::command_output(&binary, &["--version"])?;
            let devices = process::command_output(&binary, &["--list-devices"])
                .unwrap_or_else(|_| "Device enumeration unavailable".into());
            Ok(Outcome::Probed(version, devices))
        })?;
        self.status = "Probing installed backend…".into();
        Ok(())
    }
    pub fn start(
        &mut self,
        config: &Config,
        groups: Vec<Group>,
        single: Option<Group>,
    ) -> io::Result<()> {
        if self.active() || self.attached {
            return Err(io::Error::other(
                "Stop or detach the existing server before starting another",
            ));
        }
        let config = config.clone();
        let client_url = super::client::endpoint(&config.endpoint)?;
        let host = client_url.host_str().unwrap_or_default();
        if client_url.scheme() != "http" || !matches!(host, "localhost" | "127.0.0.1" | "[::1]") {
            return Err(io::Error::other("Managed servers require loopback HTTP"));
        }
        let address = if host == "[::1]" {
            format!("[::1]:{}", client_url.port_or_known_default().unwrap_or(80))
        } else {
            format!(
                "127.0.0.1:{}",
                client_url.port_or_known_default().unwrap_or(80)
            )
        };
        let router_contexts = if single.is_none() {
            groups
                .iter()
                .filter(|g| g.complete && !g.projector)
                .map(|g| {
                    (
                        router::id(g),
                        config.effective_profile(g).engine.context_size,
                    )
                })
                .collect()
        } else {
            Default::default()
        };
        let launch_mode = single.clone();
        let requested_context = single
            .as_ref()
            .and_then(|g| config.effective_profile(g).engine.context_size);
        self.submit(move |cancel| {
            let listener = TcpListener::bind(address).map_err(|_| {
                io::Error::other("Endpoint already occupied; attach instead or choose another port")
            })?;
            let help = process::command_output(&config.server_binary, &["--help"])?;
            let version = process::command_output(&config.server_binary, &["--version"])
                .unwrap_or_else(|_| "Version unavailable".into());
            let (preset, mut args) = if let Some(group) = single {
                if !group.complete || group.projector {
                    return Err(io::Error::other("Select a complete model group"));
                }
                let profile = config.effective_profile(&group);
                (
                    None,
                    process::args(&group.files[0].path, &config.endpoint, &profile, &help)?,
                )
            } else {
                let (file, args) = router::prepare(&groups, &config, &help)?;
                (Some(file), args)
            };
            if config.device != "auto" {
                if !help.contains("--device") {
                    return Err(io::Error::other(
                        "Installed backend does not advertise --device",
                    ));
                }
                args.extend(["--device".into(), config.device.clone().into()]);
            }
            let key = super::auth::Key::for_endpoint(config.api_key_file.as_deref(), &client_url)?;
            let key_file = if let Some(key) = key {
                if !help.contains("--api-key-file") {
                    return Err(io::Error::other(
                        "Installed backend does not support API key files",
                    ));
                }
                let file = key.temporary_file()?;
                args.extend(["--api-key-file".into(), file.path().as_os_str().to_owned()]);
                Some(file)
            } else {
                None
            };
            // Disable verbose request/prompt logging for managed inference where available.
            if help.contains("--log-verbosity") {
                args.extend(["--log-verbosity".into(), "2".into()]);
            }
            if cancel.load(Ordering::Acquire) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Start cancelled",
                ));
            }
            drop(listener);
            let mut server = Owned::start(&config.server_binary, &args)?;
            if let Some(file) = key_file {
                server.keep_key_file(file);
            }
            if let Some(file) = preset {
                server.keep_preset(file);
            }
            Ok(Outcome::Started(server, version))
        })?;
        self.startup_deadline = Some(Instant::now() + Duration::from_secs(90));
        self.active_context = requested_context;
        self.router_contexts = router_contexts;
        self.generation = self.generation.wrapping_add(1);
        self.single = launch_mode;
        self.selected_id = None;
        self.attached = false;
        self.failures = 0;
        self.snapshot = None;
        self.status = "Probing llama-server and starting…".into();
        Ok(())
    }
    /// Restart only a process owned by this controller, preserving single/router mode.
    /// Router presets use the latest indexed groups and current configuration.
    pub fn restart(&mut self, config: &Config, groups: Vec<Group>) -> io::Result<()> {
        if self.busy() || !self.owns_server() {
            return Err(io::Error::other("Restart requires an idle owned server"));
        }
        let single = self.single.clone();
        if single.is_none() && !groups.iter().any(|g| g.complete && !g.projector) {
            return Err(io::Error::other(
                "Scan at least one complete model before restarting",
            ));
        }
        self.stop()?;
        self.start(config, groups, single)?;
        self.status = "Restarting managed server…".into();
        Ok(())
    }
    pub fn set_loaded(&mut self, config: &Config, load: bool) -> io::Result<()> {
        let snapshot = self
            .snapshot
            .as_ref()
            .ok_or_else(|| io::Error::other("Refresh the server first"))?;
        if !snapshot.router {
            return Err(io::Error::other("Load/unload requires router mode"));
        }
        let model = self
            .selected()
            .ok_or_else(|| io::Error::other("Select a router model"))?
            .id
            .clone();
        let client = Client::from_config(config)?;
        let selected = model.clone();
        self.submit(move |_| {
            client.set_loaded(&model, load)?;
            Ok(Outcome::Changed)
        })?;
        self.selected_id = Some(selected);
        self.status = if load {
            "Requesting model load…"
        } else {
            "Requesting model unload…"
        }
        .into();
        Ok(())
    }
    pub fn stop(&mut self) -> io::Result<()> {
        if self.busy() {
            return Err(io::Error::other("Wait for the current server operation"));
        }
        if let Some(mut server) = self.owned.take() {
            server.stop()?;
            self.logs = server.logs();
            self.attached = false;
            self.snapshot = None;
            self.selected_id = None;
            self.startup_deadline = None;
            self.status = "Managed server stopped".into();
            Ok(())
        } else {
            Err(io::Error::other(
                "External servers are not owned by Llamancer",
            ))
        }
    }
    /// Finish a pending bounded operation and release only the process we own.
    pub fn shutdown(&mut self) -> io::Result<()> {
        self.startup_deadline = None;
        if let Some(job) = self.pending.take() {
            job.cancel();
            job.finish();
        }
        if self.owned.is_some() {
            self.stop()?;
        }
        Ok(())
    }
    pub fn tick(&mut self, config: &Config) {
        // Check the owned child before applying results from its endpoint. A stale
        // successful response must not turn an exited owned process into an attachment.
        if let Some(server) = self.owned.as_mut() {
            self.logs = server.logs();
            if !server.running().unwrap_or(false) {
                self.owned = None;
                self.startup_deadline = None;
                self.attached = false;
                self.snapshot = None;
                self.selected_id = None;
                self.status = "Managed server exited; inspect logs".into();
                if let Some(job) = &self.pending {
                    job.cancel();
                    self.discard_pending = true;
                }
            }
        }

        // A healthy endpoint must be observed before the startup deadline.
        // Cancel the worker and discard any late result (including a newly
        // spawned Owned server, whose Drop implementation stops it).
        if self
            .startup_deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.startup_deadline = None;
            self.attached = false;
            self.snapshot = None;
            self.selected_id = None;
            if let Some(job) = &self.pending {
                job.cancel();
                self.discard_pending = true;
            }
            if let Some(mut server) = self.owned.take() {
                let _ = server.stop();
                self.logs = server.logs();
            }
            self.status = "Managed server startup timed out (90 seconds); inspect logs".into();
        }

        if let Some(job) = self.pending.as_mut() {
            if let Some(outcome) = job.poll() {
                if let Some(job) = self.pending.take() {
                    job.finish();
                }
                if self.discard_pending {
                    self.discard_pending = false;
                    return;
                }
                match outcome {
                    Ok(Outcome::Started(server, version)) => {
                        self.owned = Some(server);
                        self.version = version.lines().take(2).collect::<Vec<_>>().join(" ");
                        self.status = "Server starting".into();
                        self.last_refresh = None;
                    }
                    Ok(Outcome::Snapshot(snapshot)) => {
                        let previous = self.selected_id.take();
                        let selected = self.selected().map(|m| m.id.clone()).or(previous);
                        self.cursor = selected
                            .and_then(|id| snapshot.models.iter().position(|m| m.id == id))
                            .unwrap_or_else(|| {
                                self.cursor.min(snapshot.models.len().saturating_sub(1))
                            });
                        self.failures = 0;
                        self.startup_deadline = None;
                        self.snapshot = Some(snapshot);
                        self.status = "Connected".into();
                    }
                    Ok(Outcome::Probed(version, devices)) => {
                        self.version = version.lines().take(2).collect::<Vec<_>>().join(" ");
                        self.devices = devices.lines().take(20).collect::<Vec<_>>().join("\n");
                        self.status = "Backend probe complete".into();
                    }
                    Ok(Outcome::Changed) => {
                        self.snapshot = None;
                        self.last_refresh = None;
                        self.status = "Router accepted request".into();
                        let _ = self.refresh(config);
                    }
                    Err(error) => {
                        if !self.owns_server() && !self.attached {
                            self.startup_deadline = None;
                        }
                        self.failures = self.failures.saturating_add(1);
                        self.selected_id = self
                            .selected()
                            .map(|m| m.id.clone())
                            .or(self.selected_id.take());
                        self.snapshot = None;
                        self.status = error.to_string();
                    }
                }
            }
        }
        if (self.owned.is_some() || self.attached)
            && !self.busy()
            && self.last_refresh.is_none_or(|last| {
                last.elapsed() >= Duration::from_secs(2u64.pow(self.failures.min(4) + 1))
            })
        {
            if let Err(error) = self.refresh(config) {
                self.last_refresh = Some(Instant::now());
                self.failures = self.failures.saturating_add(1);
                self.status = error.to_string();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    fn serve(listener: TcpListener, ids: &'static str) -> std::thread::JoinHandle<()> {
        listener.set_nonblocking(true).unwrap();
        std::thread::spawn(move || {
            for (expected, response) in [
                ("/health", r#"{"status":"ok"}"#),
                ("/props", r#"{"role":"router"}"#),
                ("/models", ids),
            ] {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(e)
                            if e.kind() == io::ErrorKind::WouldBlock
                                && Instant::now() < deadline =>
                        {
                            std::thread::sleep(Duration::from_millis(5))
                        }
                        Err(e) => panic!("fixture: {e}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                assert_eq!(line.split_whitespace().nth(1), Some(expected));
                loop {
                    line.clear();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                }
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",response.len(),response).unwrap();
            }
        })
    }
    fn wait(controller: &mut Controller, config: &Config) {
        let deadline = Instant::now() + Duration::from_secs(8);
        while controller.busy() {
            controller.tick(config);
            assert!(Instant::now() < deadline, "{}", controller.status);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn startup_deadline_stops_owned_server_and_discards_late_refresh() {
        let config = Config::default();
        let server = Owned::start(
            "/bin/sh",
            &["-c".into(), "while :; do sleep 1; done".into()],
        )
        .unwrap();
        let mut controller = Controller {
            owned: Some(server),
            attached: true,
            startup_deadline: Some(Instant::now() - Duration::from_secs(1)),
            ..Controller::default()
        };
        controller
            .submit(|cancel| {
                while !cancel.load(Ordering::Acquire) {
                    std::thread::yield_now();
                }
                Ok(Outcome::Snapshot(Snapshot {
                    router: true,
                    models: Vec::new(),
                }))
            })
            .unwrap();
        controller.tick(&config);
        wait(&mut controller, &config);
        assert!(!controller.polling());
        assert!(controller.snapshot.is_none());
        assert!(controller.status.contains("startup timed out"));
    }
    #[test]
    fn external_failure_clears_snapshot_and_reconnect_preserves_selection() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let config = Config {
            endpoint: format!("http://{addr}"),
            ..Config::default()
        };
        let worker = serve(listener, r#"{"data":[{"id":"a"},{"id":"b"}]}"#);
        let mut controller = Controller::default();
        controller.refresh(&config).unwrap();
        wait(&mut controller, &config);
        worker.join().unwrap();
        controller.select_id("b").unwrap();
        assert!(controller.start(&config, Vec::new(), None).is_err());
        controller.refresh(&config).unwrap();
        wait(&mut controller, &config);
        assert!(controller.snapshot.is_none());
        assert!(controller.polling());
        let listener = TcpListener::bind(addr).unwrap();
        let worker = serve(listener, r#"{"data":[{"id":"b"},{"id":"a"}]}"#);
        controller.last_refresh = Some(Instant::now() - Duration::from_secs(60));
        controller.tick(&config);
        wait(&mut controller, &config);
        worker.join().unwrap();
        assert_eq!(controller.selected().unwrap().id, "b");
        controller.disconnect().unwrap();
        assert!(!controller.polling());
    }
}
