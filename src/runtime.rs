//! Application orchestration. Terminal I/O stays in the binary; workers own long operations.
use crate::backends::{self, Kind};
use crate::{
    agents::{AgentAdapter, Custom, LaunchPlan, Native, Pi},
    app::{Action, App},
    config::{self, Config},
    jobs::Job,
    models::{self, Scan},
    settings::{self, Field},
    ui::theme::Theme,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::{
    io,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
enum BackendResult {
    Message(String),
    Benchmark(crate::benchmark::Record),
    Select(PathBuf, String),
}

pub enum Effect {
    None,
    Launch(LaunchPlan, crate::agents::TerminalMode),
    Open(String),
    Monitor,
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentScope {
    generation: u64,
    model: Option<String>,
}
pub struct Runtime {
    pub app: App,
    pub theme: Theme,
    root: PathBuf,
    path: PathBuf,
    scan: Option<Job<Scan>>,
    rescan_pending: bool,
    pending_agent: Option<(Option<String>, std::time::Instant, bool)>,
    probe: Option<Job<()>>,
    probe_attempted: bool,
    auto_release: std::collections::HashSet<AgentScope>,
    release_pending: std::collections::HashSet<AgentScope>,
    backend: Option<(Job<BackendResult>, Arc<AtomicU64>, u64)>,
    _lock: crate::diagnostics::SessionLock,
    diagnostics: crate::diagnostics::Diagnostics,
}
impl Runtime {
    pub fn new() -> io::Result<Self> {
        let root = config::config_dir()?;
        let lock = crate::diagnostics::SessionLock::acquire(&root)?;
        let diagnostics = crate::diagnostics::Diagnostics::new(&root);
        let path = root.join("config.toml");
        let config = Config::load(&path)?;
        let theme = Theme::load(&root, &config.theme)?;
        if !path.exists() {
            config.save(&path)?;
        }
        let downloads = crate::downloads::Controller::new(root.join("downloads.toml"))?;
        let _ = diagnostics.record(crate::diagnostics::Event::Startup, None);
        Ok(Self {
            app: App {
                config,
                nerd_available: crate::ui::icons::detect(),
                downloads: Some(downloads),
                ..App::default()
            },
            theme,
            root,
            path,
            scan: None,
            rescan_pending: false,
            pending_agent: None,
            probe: None,
            probe_attempted: false,
            auto_release: Default::default(),
            release_pending: Default::default(),
            backend: None,
            _lock: lock,
            diagnostics,
        })
    }
    pub fn active(&self) -> bool {
        self.probe.is_some()
            || self.scan.is_some()
            || self.backend.is_some()
            || self.app.server.polling()
            || self.app.discovery.busy()
            || self.app.downloads.as_ref().is_some_and(|d| d.busy())
    }
    pub fn tick(&mut self) {
        self.app.activity_frame = self.app.activity_frame.wrapping_add(1);
        if let Some((job, progress, size)) = self.backend.as_mut() {
            self.app.backend_progress =
                (*size > 0).then(|| (progress.load(Ordering::Acquire), *size));
            if let Some(outcome) = job.poll() {
                if let Some((job, _, _)) = self.backend.take() {
                    job.finish();
                }
                self.app.backend_busy = false;
                self.app.backend_progress = None;
                self.app.backend_status = match outcome {
                    Ok(BackendResult::Message(message)) => message,
                    Ok(BackendResult::Benchmark(record)) => {
                        let summary = record.summary();
                        let mut next = self.app.config.clone();
                        crate::benchmark::record(&mut next.benchmarks, record);
                        match self.persist(next) {
                            Ok(()) => summary,
                            Err(e) => format!("{summary}\nSave failed: {e}"),
                        }
                    }
                    Ok(BackendResult::Select(path, device)) => {
                        let mut next = self.app.config.clone();
                        next.server_binary = path.to_string_lossy().into_owned();
                        next.device = device;
                        match self.persist(next) {
                            Ok(()) => {
                                "Backend selected. Return to Server or Library to start inference."
                                    .into()
                            }
                            Err(e) => e.to_string(),
                        }
                    }
                    Err(e) => format!("Backend: {e}"),
                };
            }
        }

        self.app.server.tick(&self.app.config);
        self.release_finished();
        self.app.discovery.tick();
        if let Some(d) = self.app.downloads.as_mut() {
            let before = d.bundles().iter().filter(|g| g.state == "Complete").count();
            d.tick();
            let after = d.bundles().iter().filter(|g| g.state == "Complete").count();
            self.rescan_pending |= after > before;
        }
        if let Some(job) = self.scan.as_mut() {
            if let Some(outcome) = job.poll() {
                let cancelled = job.is_cancelled();
                if let Some(job) = self.scan.take() {
                    job.finish();
                }
                self.app.scanning = false;
                match outcome {
                    _ if cancelled => {
                        let _ = self
                            .diagnostics
                            .record(crate::diagnostics::Event::ScanCancelled, None);
                        self.app.notice = "Scan cancelled".into();
                    }
                    Ok(scan) => {
                        let _ = self.diagnostics.record(
                            crate::diagnostics::Event::ScanCompleted,
                            Some(scan.candidates.len()),
                        );
                        self.app.notice = format!(
                            "Scan complete: {} files, {} skipped",
                            scan.candidates.len(),
                            scan.skipped
                        );
                        self.app.invalid = scan.invalid;
                        self.app.invalid_total = scan.invalid_total;
                        self.app.invalid_cursor = 0;
                        let previous = self.app.selected().map(models::library::identity);
                        self.app.candidates = self
                            .app
                            .config
                            .filter_groups(models::group::group(scan.candidates));
                        self.app.model_cursor = previous
                            .as_ref()
                            .and_then(|id| {
                                self.app.visible().iter().position(|&i| {
                                    models::library::identity(&self.app.candidates[i]) == *id
                                })
                            })
                            .unwrap_or(0);
                        self.app.clamp_cursor();
                        self.app.details = self.app.details
                            && previous.as_ref().is_some_and(|id| {
                                self.app
                                    .selected()
                                    .is_some_and(|g| models::library::identity(g) == *id)
                            });
                    }
                    Err(e) => {
                        let _ = self
                            .diagnostics
                            .record(crate::diagnostics::Event::ScanFailed, None);
                        self.app.notice = format!("Scan ended: {}", e.kind());
                    }
                }
            }
        }
        if self.rescan_pending && self.scan.is_none() && !self.mutation_active() {
            match self.scan() {
                Ok(()) => self.rescan_pending = false,
                Err(e) => {
                    self.rescan_pending = false;
                    self.app.notice = format!("Library refresh: {e}");
                }
            }
        }
    }
    /// Only the explicit launch confirmation can arm this continuation.
    pub fn poll_launch(&mut self) -> io::Result<Effect> {
        let Some((target, deadline, exclusive)) = self.pending_agent.as_ref() else {
            return Ok(Effect::None);
        };
        let server = &self.app.server;
        if (self.app.probe_warning.is_none() && std::time::Instant::now() >= *deadline)
            || !server.active()
        {
            let scope = AgentScope {
                generation: self.app.server.generation,
                model: target.clone(),
            };
            let exclusive = *exclusive;
            self.pending_agent = None;
            if let Some(job) = self.probe.take() {
                job.cancel();
            }
            self.app.probe_warning = None;
            self.app.launching_agent = false;
            let status = server.status.clone();
            if exclusive {
                self.window_closed(Some(scope));
            }
            return Err(io::Error::other(format!(
                "Agent launch cancelled: {status}"
            )));
        }
        if server.busy() || !server.owns_server() {
            return Ok(Effect::None);
        }
        let Some(snapshot) = server.snapshot.as_ref() else {
            return Ok(Effect::None);
        };
        let ready = match target {
            Some(id) => {
                snapshot.router
                    && snapshot
                        .models
                        .iter()
                        .any(|m| m.id == *id && m.state() == "loaded")
            }
            None => !snapshot.router && !snapshot.models.is_empty(),
        };
        if !ready {
            return Ok(Effect::None);
        }
        if self.app.probe_warning.is_some() {
            return Ok(Effect::None);
        }
        let model = snapshot
            .models
            .iter()
            .find(|m| target.as_ref().is_none_or(|id| m.id == *id))
            .ok_or_else(|| io::Error::other("No ready model"))?;
        let profile = self.active_profile(&model.id);
        if profile.tool_preflight && !self.probe_attempted {
            if self.probe.is_none() {
                let client = crate::llama::client::Client::from_config(&self.app.config)?;
                let id = model.id.clone();
                let responses = self.app.config.default_agent == "omp";
                self.probe = Some(Job::start(move |_| {
                    client.tool_preflight(&id, profile.agent_thinking, responses)
                })?);
                self.app.notice = "Checking tool arguments (no commands executed)…".into();
                return Ok(Effect::None);
            }
            let Some(result) = self.probe.as_mut().and_then(|j| j.poll()) else {
                return Ok(Effect::None);
            };
            if let Some(job) = self.probe.take() {
                job.finish();
            }
            self.probe_attempted = true;
            if let Err(e) = result {
                self.app.probe_warning =
                    Some(format!("{e}. Enter launches anyway; Esc cancels agent."));
                self.app.notice = self.app.probe_warning.clone().unwrap();
                return Ok(Effect::None);
            }
        }
        let (target, _, exclusive) = self.pending_agent.take().unwrap();
        self.app.launching_agent = false;
        if let Some(id) = target {
            self.app.server.select_id(&id)?;
        } else {
            self.app.server.cursor = 0;
        }
        let scope = self.agent_scope();
        let result = self.launch_agent(crate::agents::TerminalMode::NewWindow);
        if result.is_err() && exclusive {
            self.window_closed(scope);
        }
        result
    }
    fn open_launch(&mut self) -> io::Result<()> {
        let group = self
            .app
            .selected()
            .cloned()
            .ok_or_else(|| io::Error::other("Select a model first"))?;
        self.app.launch_cursor = if group.complete && !group.projector {
            if self.app.config.api_key_file.is_some()
                || !crate::doctor::executable("kitty", std::env::var_os("PATH").as_deref())
            {
                1
            } else {
                0
            }
        } else {
            2
        };
        self.app.launch_menu = Some(group);
        self.app.details = false;
        Ok(())
    }
    fn confirm_launch(&mut self) -> io::Result<Effect> {
        let group = self
            .app
            .launch_menu
            .take()
            .ok_or_else(|| io::Error::other("Select a model"))?;
        let identity = models::library::identity(&group);
        let group = self
            .app
            .candidates
            .iter()
            .find(|g| models::library::identity(g) == identity)
            .cloned()
            .ok_or_else(|| io::Error::other("Model no longer indexed; select it again"))?;
        if self.app.launch_cursor == 2 {
            if let Some(i) = self
                .app
                .visible()
                .iter()
                .position(|&i| models::library::identity(&self.app.candidates[i]) == identity)
            {
                self.app.model_cursor = i;
                self.app.details = true;
            }
            return Ok(Effect::None);
        }
        if !group.complete || group.projector {
            return Err(io::Error::other("Select a complete model, not a projector"));
        }
        if self.mutation_active() {
            return Err(io::Error::other("Wait for model quarantine"));
        }
        let agent = self.app.launch_cursor == 0;
        if agent {
            if self.app.config.api_key_file.is_some() {
                return Err(io::Error::other("Authenticated agent sessions require explicit current-terminal launch (A). Choose Run model only."));
            }
            if !crate::doctor::executable("kitty", std::env::var_os("PATH").as_deref()) {
                return Err(io::Error::other(
                    "Install Kitty for a separate agent window, or choose Run model only.",
                ));
            }
            if self.app.config.default_agent == "pi-native" {
                return Err(io::Error::other(
                    "Pi native requires router mode; use Server.",
                ));
            }
            if self.app.config.workspace_root.is_some()
                && self
                    .app
                    .config
                    .project_dir
                    .as_ref()
                    .is_none_or(|p| !p.is_dir())
            {
                return Err(io::Error::other(
                    "Choose an active project (P in Settings) before launching the agent.",
                ));
            }
            let exe = match self.app.config.default_agent.as_str() {
                "omp" => "omp",
                "pi" => "pi",
                "custom" => self.app.config.custom_agent.as_str(),
                _ => return Err(io::Error::other("Configure an agent first")),
            };
            if !crate::doctor::executable(exe, std::env::var_os("PATH").as_deref()) {
                return Err(io::Error::other(format!(
                    "Agent executable {exe} unavailable; install/configure it or Run model only."
                )));
            }
        }
        let mut exclusive = false;
        let target = if self.app.server.owns_server()
            && self.app.server.snapshot.as_ref().is_some_and(|s| s.router)
        {
            if !self
                .app
                .server
                .router_context_matches(&group, &self.app.config)
            {
                return Err(io::Error::other("Saved inference profile differs from the running router. Stop server in Server, then launch this model again (or restart router)."));
            }
            let id = crate::llama::router::id(&group);
            self.app.server.select_id(&id)?;
            if self
                .app
                .server
                .selected()
                .is_none_or(|m| m.state() != "loaded")
            {
                exclusive = self
                    .app
                    .server
                    .selected()
                    .is_some_and(|m| m.state() == "unloaded");
                self.app.server.set_loaded(&self.app.config, true)?;
            }
            Some(id)
        } else if self.app.server.is_single_model(&group)
            && self.app.server.profile_matches(&group, &self.app.config)
        {
            None
        } else {
            if !crate::doctor::executable(
                &self.app.config.server_binary,
                std::env::var_os("PATH").as_deref(),
            ) {
                self.app.tab = 3;
                self.handle_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::NONE))?;
                self.app.notice =
                    "Choose a backend, then return to Library and open the model.".into();
                return Ok(Effect::None);
            }
            if self.app.server.owns_server() {
                if self.app.server.snapshot.as_ref().is_none_or(|s| s.router) {
                    return Err(io::Error::other(
                        "Wait for the managed server to become ready before switching models.",
                    ));
                }
                self.app.server.stop()?;
            }
            self.app
                .server
                .start(&self.app.config, Vec::new(), Some(group))?;
            exclusive = true;
            None
        };
        self.app.tab = 3;
        let scope = AgentScope {
            generation: self.app.server.generation,
            model: target.clone(),
        };
        self.release_pending.remove(&scope);
        if !agent {
            self.auto_release.remove(&scope);
        }
        if agent {
            self.probe_attempted = false;
            self.app.probe_warning = None;
            if exclusive {
                self.auto_release.insert(scope);
            }
            self.pending_agent = Some((
                target,
                std::time::Instant::now() + std::time::Duration::from_secs(120),
                exclusive,
            ));
            self.app.launching_agent = true;
            self.app.notice =
                "Starting model; agent opens in a new window when ready. Esc cancels agent launch."
                    .into();
            return self.poll_launch();
        }
        Ok(Effect::None)
    }
    fn active_profile(&self, id: &str) -> models::profile::Profile {
        if !self.app.server.owns_server() {
            return Default::default();
        }
        self.app
            .candidates
            .iter()
            .find(|g| self.app.server.is_single_model(g) || crate::llama::router::id(g) == id)
            .map(|g| self.app.config.effective_profile(g))
            .unwrap_or_default()
    }
    fn change_launch_preference(&mut self, index: usize, back: bool) -> io::Result<()> {
        use models::profile::{apply_recommended, recommended, Thinking};
        let group = self
            .app
            .launch_menu
            .as_ref()
            .ok_or_else(|| io::Error::other("Select a model"))?;
        let mut next = self.app.config.clone();
        let profile = next
            .profiles
            .entry(models::library::identity(group))
            .or_default();
        match index {
            5 => {
                if next.default_agent != "omp" {
                    return Err(io::Error::other(
                        "Thinking selector currently supports OMP only",
                    ));
                }
                let values = [
                    None,
                    Some(Thinking::Off),
                    Some(Thinking::Minimal),
                    Some(Thinking::Low),
                    Some(Thinking::Medium),
                    Some(Thinking::High),
                    Some(Thinking::Xhigh),
                    Some(Thinking::Max),
                    Some(Thinking::Auto),
                ];
                let i = values
                    .iter()
                    .position(|v| *v == profile.agent_thinking)
                    .unwrap_or(0);
                profile.agent_thinking = values[if back {
                    (i + values.len() - 1) % values.len()
                } else {
                    (i + 1) % values.len()
                }];
            }
            6 => profile.tool_preflight = !profile.tool_preflight,
            7 => {
                if !recommended(group) {
                    return Err(io::Error::other(
                        "No reviewed model-card preset for this revision",
                    ));
                }
                apply_recommended(profile);
            }
            _ => return Ok(()),
        }
        profile.validate()?;
        self.persist(next)?;
        self.app.notice = "Saved model preference; generation changes require relaunch".into();
        Ok(())
    }
    pub fn agent_scope(&self) -> Option<AgentScope> {
        let s = &self.app.server;
        if !s.owns_server() {
            return None;
        }
        let snap = s.snapshot.as_ref()?;
        let model = if snap.router {
            Some(s.selected()?.id.clone())
        } else {
            None
        };
        Some(AgentScope {
            generation: s.generation,
            model,
        })
    }
    pub fn window_closed(&mut self, scope: Option<AgentScope>) {
        if let Some(scope) = scope {
            if self.auto_release.remove(&scope) {
                self.release_pending.insert(scope);
            }
        }
    }
    fn release_finished(&mut self) {
        let generation = self.app.server.generation;
        self.auto_release
            .retain(|s| s.generation == generation && self.app.server.active());
        self.release_pending
            .retain(|s| s.generation == generation && self.app.server.active());
        if self.app.server.busy() {
            return;
        }
        let Some(scope) = self.release_pending.iter().next().cloned() else {
            return;
        };
        let result = if let Some(id) = &scope.model {
            if self.app.server.snapshot.is_none() {
                return;
            }
            self.app
                .server
                .select_id(id)
                .and_then(|()| self.app.server.set_loaded(&self.app.config, false))
        } else {
            self.app.server.stop()
        };
        self.release_pending.remove(&scope);
        self.app.notice = match result {
            Ok(()) => "Agent closed; releasing its inference resources".into(),
            Err(e) => format!("Agent cleanup: {e}"),
        };
    }
    fn save_context(&mut self, value: &str, global: bool) -> io::Result<()> {
        let mut next = self.app.config.clone();
        let mut profile = crate::models::profile::Profile::default();
        crate::models::profile::Field::Context.set(&mut profile, value)?;
        if global {
            next.default_context = profile.engine.context_size;
        } else {
            let id = models::library::identity(
                self.app
                    .launch_menu
                    .as_ref()
                    .ok_or_else(|| io::Error::other("Select a model"))?,
            );
            next.profiles.entry(id).or_default().engine.context_size = profile.engine.context_size;
        }
        self.persist(next)?;
        self.app.notice =
            "Context saved for future launches; running models require restart".into();
        Ok(())
    }
    pub fn release_session(&mut self) -> io::Result<Effect> {
        self.server_key(if self.app.server.owns_server() {
            KeyCode::Char('x')
        } else {
            KeyCode::Char('D')
        })
    }
    pub fn input_failed(&self) {
        let _ = self
            .diagnostics
            .record(crate::diagnostics::Event::InputFailed, None);
    }
    pub fn shutdown(&mut self) -> io::Result<()> {
        if let Some((job, _, _)) = self.backend.take() {
            job.cancel();
            job.finish();
        }

        if let Some(job) = self.scan.take() {
            job.cancel();
            job.finish();
        }
        let result = self.app.server.shutdown();
        self.app.discovery.shutdown();
        let downloads = self.app.downloads.as_mut().map_or(Ok(()), |d| d.shutdown());
        let _ = self
            .diagnostics
            .record(crate::diagnostics::Event::Shutdown, None);
        result.and(downloads)
    }
    /// Text focus is not a modal dialog: mouse tabs and result rows remain usable.
    pub fn modal(&self) -> bool {
        self.app.launch_menu.is_some()
            || self.app.launching_agent
            || self.app.backend_open
            || self.app.setup_open
            || self.app.projects.is_some()
            || self.app.fit_report.is_some()
            || self.app.download_destination.is_some()
            || self.app.show_invalid
            || self.app.profile_editor.is_some()
            || self.app.edit.is_some()
            || self.app.help
            || self.app.hide_confirmation.is_some()
            || self.app.quarantine_confirmation.is_some()
    }
    pub fn focus_search(&mut self) {
        if self.modal() {
            return;
        }
        match self.app.tab {
            0 => {
                self.app.searching = true;
                self.app.details = false;
            }
            1 if !self.app.discovery.busy() => self.app.discovery.editing = true,
            _ => {}
        }
    }
    pub fn blur_search(&mut self) {
        self.app.searching = false;
        self.app.discovery.editing = false;
    }
    pub fn select_row(&mut self, index: usize) {
        if self.app.launch_menu.is_some() {
            if self.app.context_input.is_some() {
                return;
            }
            if index < 8 {
                self.app.launch_cursor = index;
            }
            return;
        }

        if self.app.setup_open {
            if index < crate::setup::LABELS.len() {
                self.app.setup_cursor = index;
            }
            return;
        }
        if self.app.benchmark_open {
            return;
        }
        if let Some(choice) = &mut self.app.download_destination {
            if index < choice.roots.len() {
                choice.cursor = index;
            }
            return;
        }
        if let Some(projects) = &self.app.projects {
            if index < projects.len() {
                self.app.project_cursor = index;
            }
            return;
        }
        if self.app.backend_open {
            if !self.app.backend_busy
                && self.app.backend_confirmation.is_none()
                && index < Kind::ALL.len()
            {
                self.app.backend_cursor = index;
            }
            return;
        }
        if self.app.show_invalid {
            if index < self.app.invalid.len() {
                self.app.invalid_cursor = index;
            }
            return;
        }
        if self.modal() {
            return;
        }
        match self.app.tab {
            0 if index < self.app.visible().len() => {
                self.app.model_cursor = index;
                self.app.inspector_scroll = 0;
            }
            1 if index < self.app.discovery.len() => self.app.discovery.cursor = index,
            2 => {
                if let Some(d) = self.app.downloads.as_mut() {
                    d.select_bundle(index);
                }
            }
            3 => {
                if self
                    .app
                    .server
                    .snapshot
                    .as_ref()
                    .is_some_and(|s| index < s.models.len())
                {
                    self.app.server.cursor = index;
                }
            }
            4 if index < Field::ALL.len() => self.app.setting = index,
            _ => {}
        }
    }
    pub fn handle_key(&mut self, key: KeyEvent) -> io::Result<Effect> {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.app.dispatch(Action::Quit);
            return Ok(Effect::None);
        }
        if self.app.launching_agent {
            if self.app.probe_warning.is_some() && key.code == KeyCode::Enter {
                self.app.probe_warning = None;
                if let Some((_, deadline, _)) = self.pending_agent.as_mut() {
                    *deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                }
                return self.poll_launch();
            }
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('x')) {
                if let Some((target, _, exclusive)) = self.pending_agent.take() {
                    if exclusive {
                        self.auto_release.remove(&AgentScope {
                            generation: self.app.server.generation,
                            model: target,
                        });
                    }
                }
                if let Some(job) = self.probe.take() {
                    job.cancel();
                }
                self.app.probe_warning = None;
                self.app.launching_agent = false;
                self.app.notice =
                    "Automatic agent launch cancelled; inference remains available.".into();
            }
            return Ok(Effect::None);
        }
        if self.app.launch_menu.is_some() {
            if let Some(mut input) = self.app.context_input.take() {
                match key.code {
                    KeyCode::Esc => {}
                    KeyCode::Enter => {
                        if let Err(e) = self.save_context(&input, self.app.launch_cursor == 4) {
                            self.app.context_input = Some(input);
                            return Err(e);
                        }
                    }
                    _ => {
                        edit_text(&mut input, key, 16);
                        self.app.context_input = Some(input);
                    }
                }
                return Ok(Effect::None);
            }
            match key.code {
                KeyCode::Esc => self.app.launch_menu = None,
                KeyCode::Up => self.app.launch_cursor = self.app.launch_cursor.saturating_sub(1),
                KeyCode::Down => self.app.launch_cursor = (self.app.launch_cursor + 1).min(7),
                KeyCode::Char('c') => {
                    self.app.launch_cursor = 3;
                    self.app.context_input = Some(String::new());
                }
                KeyCode::Left | KeyCode::Right if (5..=6).contains(&self.app.launch_cursor) => self
                    .change_launch_preference(self.app.launch_cursor, key.code == KeyCode::Left)?,
                KeyCode::Enter if self.app.launch_cursor >= 5 => {
                    self.change_launch_preference(self.app.launch_cursor, false)?
                }
                KeyCode::Left | KeyCode::Right if (3..=4).contains(&self.app.launch_cursor) => {
                    let current = if self.app.launch_cursor == 4 {
                        self.app.config.default_context
                    } else {
                        self.app
                            .config
                            .effective_profile(self.app.launch_menu.as_ref().unwrap())
                            .engine
                            .context_size
                    };
                    let choices = [
                        None,
                        Some(8192),
                        Some(16384),
                        Some(32768),
                        Some(65536),
                        Some(131072),
                        Some(262144),
                    ];
                    let i = choices.iter().position(|n| *n == current).unwrap_or(0);
                    let n = choices[if key.code == KeyCode::Right {
                        (i + 1) % choices.len()
                    } else {
                        (i + choices.len() - 1) % choices.len()
                    }];
                    self.save_context(
                        &n.map(|v| v.to_string()).unwrap_or_else(|| "auto".into()),
                        self.app.launch_cursor == 4,
                    )?;
                }
                KeyCode::Enter if self.app.launch_cursor >= 3 => {
                    self.app.context_input = Some(String::new());
                }
                KeyCode::Enter => {
                    let selected = self.app.launch_menu.clone();
                    let result = self.confirm_launch();
                    if result.is_err() && self.app.tab == 0 {
                        self.app.launch_menu = selected;
                    }
                    return result;
                }
                _ => {}
            }
            return Ok(Effect::None);
        }
        if self.app.setup_open {
            let max = crate::setup::LABELS.len() - 1;
            match key.code {
                KeyCode::Esc => self.app.setup_open = false,
                KeyCode::Up => self.app.setup_cursor = self.app.setup_cursor.saturating_sub(1),
                KeyCode::Down => self.app.setup_cursor = (self.app.setup_cursor + 1).min(max),
                KeyCode::Enter => {
                    self.app.setup_open = false;
                    self.app.tab = 4;
                    match self.app.setup_cursor {
                        1 => {
                            return self
                                .handle_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::NONE))
                        }
                        2 => {
                            self.app.tab = 3;
                            return self.server_key(KeyCode::Char('i'));
                        }
                        i => {
                            let field = match i {
                                0 => Field::AddRoot,
                                3 => Field::Agent,
                                _ => Field::WorkspaceRoot,
                            };
                            self.app.setting =
                                Field::ALL.iter().position(|&f| f == field).unwrap_or(0);
                            self.app.edit = Some((field, field.initial(&self.app.config)));
                        }
                    }
                    self.app.notice = "Complete this step; press G to return to setup".into();
                }
                _ => {}
            }
            return Ok(Effect::None);
        }
        if key.code == KeyCode::Char('G') && !self.modal() {
            self.app.setup_open = true;
            self.app.notice.clear();
            return Ok(Effect::None);
        }
        if self.app.fit_report.is_some() {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('m')) {
                self.app.fit_report = None;
            }
            return Ok(Effect::None);
        }
        if let Some(choice) = &mut self.app.download_destination {
            match key.code {
                KeyCode::Esc => self.app.download_destination = None,
                KeyCode::Up => choice.cursor = choice.cursor.saturating_sub(1),
                KeyCode::Down => {
                    choice.cursor = (choice.cursor + 1).min(choice.roots.len().saturating_sub(1))
                }
                KeyCode::Enter => {
                    let root = choice
                        .roots
                        .get(choice.cursor)
                        .cloned()
                        .ok_or_else(|| io::Error::other("Select a root"))?;
                    crate::downloads::check_destination(&root, choice.required)?;
                    self.app
                        .downloads
                        .as_mut()
                        .ok_or_else(|| io::Error::other("Queue unavailable"))?
                        .enqueue_and_start(choice.files.clone(), root)?;
                    self.app.download_destination = None;
                    self.app.notice = "Download added to queue; starts automatically".into();
                    self.app.tab = 2;
                }
                _ => {}
            }
            return Ok(Effect::None);
        }
        if let Some(projects) = &self.app.projects {
            match key.code {
                KeyCode::Esc => self.app.projects = None,
                KeyCode::Up => self.app.project_cursor = self.app.project_cursor.saturating_sub(1),
                KeyCode::Down => {
                    self.app.project_cursor =
                        (self.app.project_cursor + 1).min(projects.len().saturating_sub(1))
                }
                KeyCode::Enter => {
                    let root = self
                        .app
                        .config
                        .workspace_root
                        .as_ref()
                        .ok_or_else(|| io::Error::other("Set workspace root first"))?;
                    let candidate = projects
                        .get(self.app.project_cursor)
                        .ok_or_else(|| io::Error::other("No project selected"))?;
                    let mut next = self.app.config.clone();
                    next.project_dir = Some(crate::workspace::select(root, candidate)?);
                    self.persist(next)?;
                    self.app.projects = None;
                    self.app.notice = "Active project selected".into();
                }
                _ => {}
            }
            return Ok(Effect::None);
        }
        if key.code == KeyCode::Char('I') && self.app.tab == 4 && !self.modal() {
            let mut next = self.app.config.clone();
            next.icon_mode = next.icon_mode.next();
            self.persist(next)?;
            self.app.notice = format!(
                "Icons: {} ({})",
                self.app.config.icon_mode.label(),
                if self.app.nerd_icons() {
                    "Nerd Font"
                } else {
                    "portable"
                }
            );
            return Ok(Effect::None);
        }
        if key.code == KeyCode::Char('T') && self.app.tab == 4 && !self.modal() {
            let mut next = self.app.config.clone();
            next.theme = Theme::next_builtin(&next.theme).into();
            let theme = Theme::load(&self.root, &next.theme)?;
            self.persist(next)?;
            self.theme = theme;
            self.app.notice = format!("Theme: {}", self.app.config.theme);
            return Ok(Effect::None);
        }
        if key.code == KeyCode::Char('P') && matches!(self.app.tab, 3 | 4) && !self.modal() {
            let root = self.app.config.workspace_root.as_ref().ok_or_else(|| {
                io::Error::other("Set workspace parent directory in Settings first")
            })?;
            let projects = crate::workspace::projects(root)?;
            self.app.project_cursor = projects
                .iter()
                .position(|p| Some(p) == self.app.config.project_dir.as_ref())
                .unwrap_or(0);
            self.app.projects = Some(projects);
            self.app.notice.clear();
            return Ok(Effect::None);
        }
        if self.app.backend_open {
            self.backend_key(key.code)?;
            return Ok(Effect::None);
        }
        if self.app.show_invalid {
            if key.code == KeyCode::Esc {
                self.app.show_invalid = false;
            } else {
                self.library_key(key.code)?;
            }
            return Ok(Effect::None);
        }
        if key.code == KeyCode::Char('B') && matches!(self.app.tab, 3 | 4) && !self.modal() {
            self.app.notice.clear();
            self.app.backend_open = true;
            let root = backends::root()?;
            for (i, release) in backends::RELEASES.iter().enumerate() {
                for (j, kind) in Kind::ALL.into_iter().enumerate() {
                    if backends::catalog_version(kind, release).is_some_and(|asset| {
                        asset.binary(&root).to_string_lossy() == self.app.config.server_binary
                    }) {
                        self.app.backend_version = i;
                        self.app.backend_cursor = j;
                    }
                }
            }
            self.app.backend_root = Some(root);
            self.app.backend_hardware = backends::hardware();
            self.app.backend_status =
                "Select a backend, install or probe it. No system packages are changed.".into();
            return Ok(Effect::None);
        }
        if let Some(restore) = self.app.quarantine_confirmation {
            match key.code {
                KeyCode::Esc | KeyCode::Char('n') => self.app.quarantine_confirmation = None,
                KeyCode::Char('y') => {
                    self.ensure_quarantine_idle()?;
                    let downloads = self
                        .app
                        .downloads
                        .as_mut()
                        .ok_or_else(|| io::Error::other("Queue unavailable"))?;
                    downloads.quarantine(restore, &self.app.config.endpoint)?;
                    self.app.quarantine_confirmation = None;
                    self.app.candidates.clear();
                    self.app.model_cursor = 0;
                    self.app.details = false;
                }
                _ => {}
            }
            return Ok(Effect::None);
        }
        if let Some(id) = self.app.hide_confirmation.clone() {
            match key.code {
                KeyCode::Esc | KeyCode::Char('n') => self.app.hide_confirmation = None,
                KeyCode::Char('y') => {
                    let mut next = self.app.config.clone();
                    next.hidden_models.insert(id.clone());
                    self.persist(next)?;
                    self.app
                        .candidates
                        .retain(|g| models::library::identity(g) != id);
                    self.app.clamp_cursor();
                    self.app.hide_confirmation = None;
                    self.app.details = false;
                    self.app.notice =
                        "Hidden from library; files retained. Restore in Settings.".into();
                }
                _ => {}
            }
            return Ok(Effect::None);
        }
        if self.app.discovery.editing {
            if key.code == KeyCode::Enter {
                if self.app.discovery.repo.is_some() {
                    self.app.discovery.rebuild_variants();
                    self.app.discovery.editing = false;
                } else {
                    self.app.discovery.search()?;
                }
            } else if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Tab | KeyCode::BackTab | KeyCode::Down | KeyCode::Up
            ) {
                if self.app.discovery.repo.is_some() {
                    self.app.discovery.rebuild_variants();
                }
                self.app.discovery.editing = false;
            } else {
                if self.app.discovery.repo.is_some() {
                    edit_text(&mut self.app.discovery.variant_filter, key, 512);
                    self.app.discovery.rebuild_variants();
                } else {
                    if !(key.code == KeyCode::Char('/') && self.app.discovery.query.is_empty()) {
                        edit_text(&mut self.app.discovery.query, key, 512);
                    }
                }
            }
            return Ok(Effect::None);
        }
        if self.app.profile_editor.is_some() {
            self.profile_key(key)?;
            return Ok(Effect::None);
        }
        if self.app.searching {
            if matches!(
                key.code,
                KeyCode::Enter
                    | KeyCode::Esc
                    | KeyCode::Tab
                    | KeyCode::BackTab
                    | KeyCode::Down
                    | KeyCode::Up
            ) {
                self.app.searching = false;
            } else {
                edit_text(&mut self.app.query, key, 512);
                self.app.model_cursor = 0;
            }
            return Ok(Effect::None);
        }
        if self.app.edit.is_some() {
            self.settings_edit(key)?;
            return Ok(Effect::None);
        }
        if !self.modal()
            && (key.code == KeyCode::F(3)
                || (key.code == KeyCode::Char('f')
                    && key.modifiers.contains(KeyModifiers::CONTROL)))
            && self.app.tab <= 1
        {
            self.focus_search();
            return Ok(Effect::None);
        }
        let global = match key.code {
            KeyCode::Char('q') => Some(Action::Quit),
            KeyCode::Esc => Some(Action::Escape),
            KeyCode::Char('?') => Some(Action::ToggleHelp),
            KeyCode::Tab | KeyCode::Right => Some(Action::Next),
            KeyCode::BackTab | KeyCode::Left => Some(Action::Previous),
            KeyCode::Char(c @ '1'..='5') => Some(Action::Select(c as usize - '1' as usize)),
            _ => None,
        };
        if let Some(action) = global {
            self.app.dispatch(action);
            return Ok(Effect::None);
        }
        if self.app.help {
            return Ok(Effect::None);
        }
        self.app.notice.clear();
        match self.app.tab {
            0 => self.library_key(key.code)?,
            1 => self.discovery_key(key.code)?,
            2 => self.download_key(key.code)?,
            3 => return self.server_key(key.code),
            4 => self.settings_key(key.code),
            _ => {}
        }
        Ok(Effect::None)
    }
    fn backend_key(&mut self, key: KeyCode) -> io::Result<()> {
        self.app.notice.clear();
        if self.app.benchmark_open {
            if matches!(key, KeyCode::Esc | KeyCode::Char('h')) {
                self.app.benchmark_open = false;
            }
            return Ok(());
        }
        if let Some((job, _, _)) = &self.backend {
            if matches!(key, KeyCode::Char('x') | KeyCode::Esc) {
                job.cancel();
                self.app.backend_status = "Cancelling…".into();
            }
            return Ok(());
        }
        let action = if let Some(confirm) = self.app.backend_confirmation {
            match key {
                KeyCode::Char('y') => {
                    self.app.backend_confirmation = None;
                    confirm
                }
                KeyCode::Char('n') | KeyCode::Esc => {
                    self.app.backend_confirmation = None;
                    self.app.backend_status = "Cancelled".into();
                    return Ok(());
                }
                _ => return Ok(()),
            }
        } else {
            match key {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.app.backend_open = false;
                    return Ok(());
                }
                KeyCode::Up => {
                    self.app.backend_cursor =
                        (self.app.backend_cursor + Kind::ALL.len() - 1) % Kind::ALL.len();
                    return Ok(());
                }
                KeyCode::Down => {
                    self.app.backend_cursor = (self.app.backend_cursor + 1) % Kind::ALL.len();
                    return Ok(());
                }
                KeyCode::Char(action @ ('i' | 'd' | 'b')) => {
                    self.app.backend_confirmation = Some(action);
                    return Ok(());
                }
                KeyCode::Char('v' | 'u') => {
                    self.app.backend_version =
                        (self.app.backend_version + 1) % backends::RELEASES.len();
                    self.app.backend_status =
                        "Version selected for browsing; install and explicitly use it to switch."
                            .into();
                    return Ok(());
                }
                KeyCode::Char('c') => {
                    let job = Job::start(|cancel| {
                        Ok(BackendResult::Message(backends::discovery::check(&cancel)?))
                    })?;
                    self.backend = Some((job, Arc::new(AtomicU64::new(0)), 0));
                    self.app.backend_busy = true;
                    self.app.backend_status = "Checking upstream releases…".into();
                    return Ok(());
                }
                KeyCode::Char('h') => {
                    self.app.benchmark_open = true;
                    return Ok(());
                }
                KeyCode::Char('p') => 'p',
                KeyCode::Enter => 's',
                _ => return Ok(()),
            }
        };
        let asset = backends::catalog_version(
            Kind::ALL[self.app.backend_cursor],
            backends::RELEASES[self.app.backend_version],
        )
        .ok_or_else(|| io::Error::other("No official asset for this architecture"))?;
        let root = self
            .app
            .backend_root
            .clone()
            .ok_or_else(|| io::Error::other("No backend storage directory"))?;
        if matches!(action, 's' | 'd' | 'b')
            && (self.app.server.polling()
                || self.app.server.snapshot.is_some()
                || self.mutation_active())
        {
            return Err(io::Error::other(
                "Stop/detach inference before changing or benchmarking backends",
            ));
        }
        if action != 'i' && !asset.installed(&root) {
            return Err(io::Error::other("Install selected backend first"));
        }
        let model = if action == 'b' {
            let group = self
                .app
                .selected()
                .filter(|g| g.complete && !g.projector && g.files.len() == 1)
                .ok_or_else(|| {
                    io::Error::other("Select a small complete single-file model in Library first")
                })?;
            Some(group.files[0].path.clone())
        } else {
            None
        };
        let selected = self.app.config.server_binary.clone();
        let progress = Arc::new(AtomicU64::new(0));
        let counter = progress.clone();
        let job = Job::start(move |cancel| {
            let binary = asset.binary(&root);
            match action {
                'i' => {
                    backends::install(&root, asset, &cancel, &counter)?;
                    Ok(BackendResult::Message(
                        "Verified archive installed. Probe and select before use.".into(),
                    ))
                }
                'p' => {
                    let output = backends::probe(&binary)?;
                    let message = match backends::detected_device(asset.kind, &output) {
                        Ok(_) => output,
                        Err(e) => format!("{e}\n{output}"),
                    };
                    Ok(BackendResult::Message(message))
                }
                's' => {
                    let output = backends::probe(&binary)?;
                    let device = backends::detected_device(asset.kind, &output)?;
                    Ok(BackendResult::Select(binary, device))
                }
                'd' => {
                    backends::remove(&root, asset, &selected)?;
                    Ok(BackendResult::Message("Managed backend removed".into()))
                }
                'b' => Ok(BackendResult::Benchmark(backends::benchmark(
                    &root,
                    asset,
                    &model.expect("benchmark model"),
                    &cancel,
                )?)),
                _ => Err(io::Error::other("Unknown backend action")),
            }
        })?;
        self.backend = Some((
            job,
            progress,
            if action == 'i' { asset.total_size() } else { 0 },
        ));
        self.app.backend_busy = true;
        self.app.backend_status = "Working…".into();
        Ok(())
    }
    fn persist(&mut self, next: Config) -> io::Result<()> {
        next.save(&self.path)?;
        if next.server_binary != self.app.config.server_binary {
            self.app.server.devices.clear();
        }
        self.app.config = next;
        let _ = self
            .diagnostics
            .record(crate::diagnostics::Event::SettingsSaved, None);
        Ok(())
    }
    fn profile_key(&mut self, key: KeyEvent) -> io::Result<()> {
        let editor = self.app.profile_editor.as_mut().expect("editor active");
        let field = editor.selected();
        if let Some(input) = editor.input.as_mut() {
            match key.code {
                KeyCode::Esc => editor.input = None,
                KeyCode::Enter => {
                    field.set(&mut editor.draft, input)?;
                    editor.input = None;
                    self.app.notice = "Draft updated; press s to save".into();
                }
                _ => edit_text(input, key, 4096),
            }
            return Ok(());
        }
        match key.code {
            KeyCode::Esc => {
                self.app.profile_editor = None;
                self.app.notice = "Profile edit cancelled".into();
            }
            KeyCode::Up => {
                editor.cursor = (editor.cursor + editor.fields().len() - 1) % editor.fields().len()
            }
            KeyCode::Down => editor.cursor = (editor.cursor + 1) % editor.fields().len(),
            KeyCode::Char('m') => {
                let group = self
                    .app
                    .candidates
                    .iter()
                    .find(|g| models::library::identity(g) == editor.id)
                    .ok_or_else(|| io::Error::other("Rescan model library first"))?;
                let choices = models::group::projector_candidates(group, &self.app.candidates);
                let [projector] = choices.as_slice() else {
                    return Err(io::Error::other(if choices.is_empty() {
                        "No complete same-directory projector found; enter the path manually"
                    } else {
                        "Multiple projector candidates; choose the correct path manually"
                    }));
                };
                let path = projector.files[0]
                    .path
                    .to_str()
                    .ok_or_else(|| io::Error::other("Projector path is not UTF-8"))?;
                editor.draft.engine.projector = Some(path.into());
                self.app.notice="Projector candidate selected in draft; verify compatibility and press s to save".into();
            }
            KeyCode::Char('a') => {
                editor.advanced = !editor.advanced;
                editor.cursor = 0;
            }
            KeyCode::Enter => editor.input = Some(field.value(&editor.draft)),
            KeyCode::Char('s') => {
                let mut next = self.app.config.clone();
                next.profiles
                    .insert(editor.id.clone(), editor.draft.clone());
                self.persist(next)?;
                self.app.profile_editor = None;
                self.app.notice =
                    "Inference profile saved; restart an owned router to apply".into();
            }
            _ => {}
        }
        Ok(())
    }
    fn settings_edit(&mut self, key: KeyEvent) -> io::Result<()> {
        if key.code == KeyCode::Enter && self.mutation_active() {
            return Err(io::Error::other("Wait for model quarantine to finish"));
        }
        let (field, input) = self.app.edit.as_mut().expect("settings edit active");
        match key.code {
            KeyCode::Esc => {
                self.app.edit = None;
                self.app.notice = "Edit cancelled".into();
            }
            KeyCode::Enter => {
                let (next, theme) = settings::apply(&self.app.config, *field, input, &self.root)?;
                let credential_cleared = self.app.config.api_key_file.is_some()
                    && next.api_key_file.is_none()
                    && next.endpoint != self.app.config.endpoint;
                let endpoint_changed = next.endpoint != self.app.config.endpoint
                    || next.server_binary != self.app.config.server_binary
                    || next.device != self.app.config.device
                    || next.api_key_file != self.app.config.api_key_file;
                if self.app.server.active() && endpoint_changed {
                    return Err(io::Error::other(
                        "Stop server before changing endpoint or binary",
                    ));
                }
                let roots_changed = next.scan_roots() != self.app.config.scan_roots()
                    || next.hidden_models != self.app.config.hidden_models;
                self.persist(next)?;
                self.theme = theme;
                if roots_changed {
                    self.rescan_pending = true;
                    self.app.candidates.clear();
                    self.app.invalid.clear();
                    self.app.invalid_total = 0;
                    self.app.invalid_cursor = 0;
                    self.app.details = false;
                    self.app.model_cursor = 0;
                    if let Some(job) = &self.scan {
                        job.cancel();
                    }
                }
                if endpoint_changed {
                    self.app.server.disconnect()?;
                    self.app.server.status = "Endpoint changed; refresh server".into();
                }
                self.app.edit = None;
                self.app.notice = if credential_cleared {
                    "Endpoint changed; previous API key cleared. Set a new key if needed."
                } else {
                    "Settings saved"
                }
                .into();
            }
            _ => edit_text(input, key, 4096),
        }
        Ok(())
    }
    fn settings_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Up => {
                self.app.setting = (self.app.setting + Field::ALL.len() - 1) % Field::ALL.len()
            }
            KeyCode::Down => self.app.setting = (self.app.setting + 1) % Field::ALL.len(),
            KeyCode::Enter => {
                let f = Field::ALL[self.app.setting];
                self.app.edit = Some((f, f.initial(&self.app.config)));
            }
            _ => {}
        }
    }
    fn mutation_active(&self) -> bool {
        self.app.downloads.as_ref().is_some_and(|d| d.mutating())
    }
    fn ensure_quarantine_idle(&self) -> io::Result<()> {
        if self.scan.is_some()
            || self.app.server.polling()
            || self.app.server.snapshot.is_some()
            || self.app.downloads.as_ref().is_some_and(|d| d.busy())
        {
            return Err(io::Error::other(
                "Stop/detach inference and wait for scans/downloads before moving files",
            ));
        }
        Ok(())
    }
    pub fn scan(&mut self) -> io::Result<()> {
        if self.scan.is_some() || self.mutation_active() {
            return Err(io::Error::other("Scan or model mutation already running"));
        }
        let roots = self.app.config.scan_roots();
        self.scan = Some(Job::start(move |cancel| models::scan(roots, &cancel))?);
        let _ = self
            .diagnostics
            .record(crate::diagnostics::Event::ScanStarted, None);
        self.app.scanning = true;
        self.app.notice = "Scanning model roots…".into();
        Ok(())
    }
    fn library_key(&mut self, key: KeyCode) -> io::Result<()> {
        if self.app.show_invalid {
            match key {
                KeyCode::Char('i') | KeyCode::Enter => self.app.show_invalid = false,
                KeyCode::Up => self.app.invalid_cursor = self.app.invalid_cursor.saturating_sub(1),
                KeyCode::Down => {
                    self.app.invalid_cursor =
                        (self.app.invalid_cursor + 1).min(self.app.invalid.len().saturating_sub(1))
                }
                KeyCode::Home => self.app.invalid_cursor = 0,
                KeyCode::End => self.app.invalid_cursor = self.app.invalid.len().saturating_sub(1),
                _ => {}
            }
            return Ok(());
        }
        if self.app.details && matches!(key, KeyCode::PageUp | KeyCode::PageDown) {
            self.app.inspector_scroll = if key == KeyCode::PageUp {
                self.app.inspector_scroll.saturating_sub(6)
            } else {
                self.app.inspector_scroll.saturating_add(6)
            };
            return Ok(());
        }
        self.app.inspector_scroll = 0;
        match key {
            KeyCode::Char('i') => self.app.show_invalid = true,
            KeyCode::Char('m') => {
                let group = self
                    .app
                    .selected()
                    .ok_or_else(|| io::Error::other("Select a model first"))?;
                let profile = self.app.config.effective_profile(group);
                self.app.fit_report = Some(models::fit::report(
                    group,
                    &profile,
                    &self.app.config,
                    &self.app.server.devices,
                    models::fit::available_ram(),
                ));
            }
            KeyCode::Char('t') => {
                self.ensure_quarantine_idle()?;
                let group = self
                    .app
                    .selected()
                    .cloned()
                    .ok_or_else(|| io::Error::other("Select a model"))?;
                let d = self
                    .app
                    .downloads
                    .as_mut()
                    .ok_or_else(|| io::Error::other("Queue unavailable"))?;
                d.select_managed_group(&group)?;
                self.app.tab = 2;
                self.app.quarantine_confirmation = Some(false);
            }
            KeyCode::Char('h') => {
                if self.app.server.polling() || self.app.server.snapshot.is_some() {
                    return Err(io::Error::other(
                        "Stop or disconnect inference before hiding a model",
                    ));
                }
                if let Some(group) = self.app.selected() {
                    self.app.hide_confirmation = Some(models::library::identity(group));
                }
            }

            KeyCode::Up => self.app.model_cursor = self.app.model_cursor.saturating_sub(1),
            KeyCode::Down => {
                self.app.model_cursor =
                    (self.app.model_cursor + 1).min(self.app.visible().len().saturating_sub(1))
            }
            KeyCode::Enter if self.app.details => self.app.details = false,
            KeyCode::Enter if self.app.selected().is_some() => self.open_launch()?,
            KeyCode::Char('j') if self.app.selected().is_some() => {
                self.app.details = !self.app.details
            }
            KeyCode::Char('/') => {
                self.app.searching = true;
                self.app.details = false;
            }
            KeyCode::Char('s') => {
                self.app.sort = self.app.sort.next();
                self.app.model_cursor = 0;
                self.app.details = false;
            }
            KeyCode::Char('v') => {
                self.app.only_favorites = !self.app.only_favorites;
                self.app.model_cursor = 0;
                self.app.details = false;
            }
            KeyCode::Char('f') => {
                if let Some(g) = self.app.selected() {
                    let id = models::library::identity(g);
                    let mut next = self.app.config.clone();
                    if !next.favorites.remove(&id) {
                        next.favorites.insert(id);
                    }
                    self.persist(next)?;
                    self.app.clamp_cursor();
                    self.app.details = false;
                    self.app.notice = "Favorites saved".into();
                }
            }
            KeyCode::Char('p') => {
                if let Some(g) = self.app.selected() {
                    if g.projector || !g.complete {
                        return Err(io::Error::other(
                            "Select a complete model group, not a projector",
                        ));
                    }
                    let id = models::library::identity(g);
                    let draft = self
                        .app
                        .config
                        .profiles
                        .get(&id)
                        .cloned()
                        .unwrap_or_default();
                    self.app.profile_editor = Some(models::profile::Editor::new(id, draft));
                }
            }
            KeyCode::Char('l' | 'L') => {
                if self.mutation_active() {
                    return Err(io::Error::other("Wait for model quarantine to finish"));
                }
                if let Some(group) = self.app.selected().cloned() {
                    if !group.complete || group.projector {
                        return Err(io::Error::other("Select a complete model, not a projector"));
                    }
                    if !self.app.server.owns_server()
                        && !crate::doctor::executable(
                            &self.app.config.server_binary,
                            std::env::var_os("PATH").as_deref(),
                        )
                    {
                        self.app.tab = 3;
                        self.handle_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::NONE))?;
                        self.app.notice="Choose/install a backend first. Then return to Library and press l to run the model.".into();
                        return Ok(());
                    }
                    if self.app.server.owns_server()
                        && self.app.server.snapshot.as_ref().is_some_and(|s| s.router)
                    {
                        let id = crate::llama::router::id(&group);
                        self.app.server.select_id(&id)?;
                        let scope = AgentScope {
                            generation: self.app.server.generation,
                            model: Some(id),
                        };
                        self.auto_release.remove(&scope);
                        self.release_pending.remove(&scope);
                        self.app.server.set_loaded(&self.app.config, true)?;
                    } else {
                        self.app
                            .server
                            .start(&self.app.config, Vec::new(), Some(group))?;
                    }
                    self.app.tab = 3;
                }
            }
            KeyCode::Char('r') => self.scan()?,
            KeyCode::Char('x') => {
                if let Some(job) = &self.scan {
                    job.cancel();
                    self.app.notice = "Cancelling scan…".into();
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn discovery_key(&mut self, key: KeyCode) -> io::Result<()> {
        match key {
            KeyCode::Char('s') if !self.app.discovery.busy() => {
                let d = &mut self.app.discovery;
                if d.repo.is_some() {
                    d.variant_sort = d.variant_sort.next();
                    d.rebuild_variants();
                } else {
                    d.ranking = d.ranking.next();
                    d.search()?;
                }
            }
            KeyCode::Char('n') => self.app.discovery.more()?,
            KeyCode::Char('/') if !self.app.discovery.busy() => self.app.discovery.editing = true,
            KeyCode::Up => self.app.discovery.move_cursor(false),
            KeyCode::Down => self.app.discovery.move_cursor(true),
            KeyCode::Backspace => self.app.discovery.back(),
            KeyCode::Enter if self.app.discovery.repo.is_none() => {
                if self.app.discovery.repos.is_empty() {
                    self.app.discovery.search()?;
                } else {
                    self.app.discovery.open()?;
                }
            }
            KeyCode::Char('d' | 'D') | KeyCode::Enter if !self.app.discovery.busy() => {
                let files = self.app.discovery.selected_bundle();
                if files.is_empty() {
                    return Err(io::Error::other("Select a file or complete shard bundle"));
                }
                let roots = &self.app.config.model_roots;
                if roots.is_empty() {
                    return Err(io::Error::other(
                        "Add a download/model root in Settings first",
                    ));
                }
                let preferred = self.app.config.download_root.as_deref();
                if key == KeyCode::Enter
                    || key == KeyCode::Char('D')
                    || (roots.len() > 1 && preferred.is_none())
                {
                    self.app.download_destination = Some(crate::downloads::DestinationChoice::new(
                        files,
                        roots.clone(),
                        preferred,
                    )?);
                    return Ok(());
                }
                let root = preferred
                    .filter(|p| roots.iter().any(|r| r == p))
                    .unwrap_or(roots[0].as_path())
                    .to_owned();
                crate::downloads::check_destination(
                    &root,
                    crate::downloads::bundle_bytes(&files)?,
                )?;
                let downloads = self
                    .app
                    .downloads
                    .as_mut()
                    .ok_or_else(|| io::Error::other("Queue unavailable"))?;
                downloads.enqueue_and_start(files, root)?;
                self.app.notice = "Download added to queue; starts automatically".into();
                self.app.tab = 2;
            }
            _ => {}
        }
        Ok(())
    }
    fn download_key(&mut self, key: KeyCode) -> io::Result<()> {
        if matches!(key, KeyCode::Char('t' | 'T')) {
            self.ensure_quarantine_idle()?;
            let d = self
                .app
                .downloads
                .as_ref()
                .ok_or_else(|| io::Error::other("Queue unavailable"))?;
            d.quarantine_targets()?;
            self.app.quarantine_confirmation = Some(key == KeyCode::Char('T'));
            return Ok(());
        }
        let Some(d) = self.app.downloads.as_mut() else {
            return Ok(());
        };
        match key {
            KeyCode::Up => d.move_bundle(false),
            KeyCode::Down => d.move_bundle(true),
            KeyCode::Char('r') | KeyCode::Enter => d.resume_bundle()?,
            KeyCode::Char('R') => d.start_queue()?,
            KeyCode::Char('x') => d.pause(),
            KeyCode::Char('z') => d.remove_bundle()?,
            _ => {}
        }
        Ok(())
    }
    fn server_key(&mut self, key: KeyCode) -> io::Result<Effect> {
        if self.mutation_active() {
            return Err(io::Error::other("Wait for model quarantine to finish"));
        }
        if matches!(key, KeyCode::Char('l' | 'u' | 'A'))
            || (key == KeyCode::Char('a')
                && self.app.config.agent_terminal != crate::agents::TerminalMode::NewWindow)
        {
            if let Some(scope) = self.agent_scope() {
                self.auto_release.remove(&scope);
                self.release_pending.remove(&scope);
            }
        }
        let c = &self.app.config;
        let s = &mut self.app.server;
        match key {
            KeyCode::Up => s.move_cursor(false),
            KeyCode::Down => s.move_cursor(true),
            KeyCode::Char('r') => s.refresh(c)?,
            KeyCode::Char('D') => s.disconnect()?,
            KeyCode::Char('i') => s.probe(c)?,
            KeyCode::Char('s') => s.start(c, self.app.candidates.clone(), None)?,
            KeyCode::Char('R') => s.restart(c, self.app.candidates.clone())?,
            KeyCode::Char('x') => s.stop()?,
            KeyCode::Char('l' | 'u') => s.set_loaded(c, key == KeyCode::Char('l'))?,
            KeyCode::Char('g') => s.show_logs = !s.show_logs,
            KeyCode::Char('w') => {
                return Ok(Effect::Open(
                    crate::llama::client::endpoint(&c.endpoint)?.to_string(),
                ))
            }
            KeyCode::Char('b') => return Ok(Effect::Monitor),
            KeyCode::Enter | KeyCode::Char('a' | 'A') => {
                return self.launch_agent(if key == KeyCode::Enter {
                    crate::agents::TerminalMode::NewWindow
                } else if key == KeyCode::Char('A') {
                    crate::agents::TerminalMode::Current
                } else {
                    c.agent_terminal
                })
            }
            _ => {}
        }
        Ok(Effect::None)
    }
    fn launch_agent(&mut self, mode: crate::agents::TerminalMode) -> io::Result<Effect> {
        let c = &self.app.config;
        let s = &self.app.server;
        if s.busy() {
            return Err(io::Error::other("Wait for server operation"));
        }
        let snapshot = s
            .snapshot
            .as_ref()
            .ok_or_else(|| io::Error::other("Refresh server first"))?;
        let model = s
            .selected()
            .ok_or_else(|| io::Error::other("Select a model first"))?;
        if snapshot.router && !matches!(model.state(), "loaded" | "sleeping") {
            return Err(io::Error::other("Load the model first"));
        }
        if c.default_agent == "pi-native" && !snapshot.router {
            return Err(io::Error::other(
                "Pi native integration requires router mode",
            ));
        }
        let cwd = match &c.project_dir {
            Some(p) if p.is_dir() => p.clone(),
            Some(_) => {
                return Err(io::Error::other(
                    "Active project no longer exists; select P",
                ))
            }
            None if c.workspace_root.is_some() => {
                return Err(io::Error::other("Select an active project with P"))
            }
            None => std::env::current_dir()?,
        };
        let mut plan = match c.default_agent.as_str() {
            "omp" => Native("omp").prepare(&c.endpoint, &model.id, cwd)?,
            "pi" => {
                let context=model.context_limit().or_else(||self.app.candidates.iter().find(|group|crate::llama::router::id(group)==model.id).and_then(|group|c.profiles.get(&models::library::identity(group)).and_then(|p|p.engine.context_size).map(u64::from))).ok_or_else(||io::Error::other("Server did not report an active context size; use a configured model profile or pi-native"))?;
                Pi {
                    root: &self.root,
                    context,
                }
                .prepare(&c.endpoint, &model.id, cwd)?
            }
            "pi-native" => Native("pi").prepare(&c.endpoint, &model.id, cwd)?,
            "custom" => Custom {
                executable: &c.custom_agent,
                args: &c.custom_agent_args,
            }
            .prepare(&c.endpoint, &model.id, cwd)?,
            _ => return Err(io::Error::other("Unknown agent")),
        };
        if c.default_agent == "omp" {
            if let Some(t) = self.active_profile(&model.id).agent_thinking {
                plan.args.extend(["--thinking".into(), t.label().into()]);
            }
        }
        if let Some(key) = crate::llama::auth::Key::for_endpoint(
            c.api_key_file.as_deref(),
            &crate::llama::client::endpoint(&c.endpoint)?,
        )? {
            if mode != crate::agents::TerminalMode::Current {
                return Err(io::Error::other("Authenticated agent launch requires current terminal (A); no secrets in Kitty argv"));
            }
            plan.with_key(&key, &c.default_agent)?;
        }
        plan.validate(mode)?;
        Ok(Effect::Launch(plan, mode))
    }
}
fn edit_text(input: &mut String, key: KeyEvent, limit: usize) {
    match key.code {
        KeyCode::Backspace => {
            input.pop();
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => input.clear(),
        KeyCode::Char(c)
            if !c.is_control()
                && input.len() + c.len_utf8() <= limit
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            input.push(c)
        }
        _ => {}
    }
}
