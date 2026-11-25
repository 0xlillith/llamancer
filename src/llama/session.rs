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
    pub fn active(&self) -> bool {
        self.busy() || self.owns_server()
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
        Ok(())
    }
    pub fn refresh(&mut self, config: &Config) -> io::Result<()> {
        let client = Client::new(&config.endpoint)?;
        self.submit(move |_| client.snapshot().map(Outcome::Snapshot))?;
        self.last_refresh = Some(Instant::now());
        self.status = "Checking server…".into();
        Ok(())
    }
    pub fn probe(&mut self, config: &Config) -> io::Result<()> {
        let binary = config.server_binary.clone();
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
        if self.active() {
            return Err(io::Error::other(
                "Stop the managed server or wait for the current operation",
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
                let profile = config
                    .profiles
                    .get(&identity(&group))
                    .cloned()
                    .unwrap_or_default();
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
            if cancel.load(Ordering::Acquire) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Start cancelled",
                ));
            }
            drop(listener);
            let mut server = Owned::start(&config.server_binary, &args)?;
            if let Some(file) = preset {
                server.keep_preset(file);
            }
            Ok(Outcome::Started(server, version))
        })?;
        self.snapshot = None;
        self.status = "Probing llama-server and starting…".into();
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
        let client = Client::new(&config.endpoint)?;
        self.submit(move |_| {
            client.set_loaded(&model, load)?;
            Ok(Outcome::Changed)
        })?;
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
            self.snapshot = None;
            self.status = "Managed server stopped".into();
            Ok(())
        } else {
            Err(io::Error::other(
                "External servers are not owned by Llamancer",
            ))
        }
    }
    pub fn tick(&mut self, config: &Config) {
        if let Some(job) = self.pending.as_mut() {
            if let Some(outcome) = job.poll() {
                if let Some(job) = self.pending.take() {
                    job.finish();
                }
                match outcome {
                    Ok(Outcome::Started(server, version)) => {
                        self.owned = Some(server);
                        self.version = version.lines().take(2).collect::<Vec<_>>().join(" ");
                        self.status = "Server starting".into();
                        self.last_refresh = None;
                    }
                    Ok(Outcome::Snapshot(snapshot)) => {
                        self.cursor = self.cursor.min(snapshot.models.len().saturating_sub(1));
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
                        self.snapshot = None;
                        self.status = error.to_string();
                    }
                }
            }
        }
        if let Some(server) = self.owned.as_mut() {
            self.logs = server.logs();
            if !server.running().unwrap_or(false) {
                self.owned = None;
                self.snapshot = None;
                self.status = "Managed server exited; inspect logs".into();
            }
        }
        if self.owned.is_some()
            && !self.busy()
            && self
                .last_refresh
                .is_none_or(|last| last.elapsed() >= Duration::from_secs(2))
        {
            let _ = self.refresh(config);
        }
    }
}
