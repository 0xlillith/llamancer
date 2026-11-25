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
pub struct Runtime {
    pub app: App,
    pub theme: Theme,
    root: PathBuf,
    path: PathBuf,
    scan: Option<Job<Scan>>,
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
                downloads: Some(downloads),
                ..App::default()
            },
            theme,
            root,
            path,
            scan: None,
            backend: None,
            _lock: lock,
            diagnostics,
        })
    }
    pub fn active(&self) -> bool {
        self.scan.is_some()
            || self.backend.is_some()
            || self.app.server.polling()
            || self.app.discovery.busy()
            || self.app.downloads.as_ref().is_some_and(|d| d.busy())
    }
    pub fn tick(&mut self) {
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
        self.app.discovery.tick();
        if let Some(d) = self.app.downloads.as_mut() {
            d.tick();
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
                        self.app.candidates = self
                            .app
                            .config
                            .filter_groups(models::group::group(scan.candidates));
                        self.app.model_cursor = 0;
                        self.app.details = false;
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
    pub fn modal(&self) -> bool {
        self.app.backend_open
            || self.app.setup_open
            || self.app.projects.is_some()
            || self.app.fit_report.is_some()
            || self.app.download_destination.is_some()
            || self.app.show_invalid
            || self.app.profile_editor.is_some()
            || self.app.edit.is_some()
            || self.app.searching
            || self.app.discovery.editing
            || self.app.help
            || self.app.hide_confirmation.is_some()
            || self.app.quarantine_confirmation.is_some()
    }
    pub fn select_row(&mut self, index: usize) {
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
            0 if index < self.app.visible().len() => self.app.model_cursor = index,
            1 if index < self.app.discovery.len() => self.app.discovery.cursor = index,
            2 => {
                if let Some(d) = self.app.downloads.as_mut() {
                    if index < d.queue.items.len() {
                        d.cursor = index;
                    }
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
                        .add(choice.files.clone(), root)?;
                    self.app.download_destination = None;
                    self.app.notice = "Queued; press r to start".into();
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
                self.app.discovery.search()?;
            } else if key.code == KeyCode::Esc {
                self.app.discovery.editing = false;
            } else {
                edit_text(&mut self.app.discovery.query, key, 512);
            }
            return Ok(Effect::None);
        }
        if self.app.profile_editor.is_some() {
            self.profile_key(key)?;
            return Ok(Effect::None);
        }
        if self.app.searching {
            if matches!(key.code, KeyCode::Enter | KeyCode::Esc) {
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
        match key {
            KeyCode::Char('i') => self.app.show_invalid = true,
            KeyCode::Char('m') => {
                let group = self
                    .app
                    .selected()
                    .ok_or_else(|| io::Error::other("Select a model first"))?;
                let profile = self
                    .app
                    .config
                    .profiles
                    .get(&models::library::identity(group))
                    .cloned()
                    .unwrap_or_default();
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
            KeyCode::Enter if self.app.selected().is_some() => self.app.details = !self.app.details,
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
            KeyCode::Char('l') => {
                if self.mutation_active() {
                    return Err(io::Error::other("Wait for model quarantine to finish"));
                }
                if let Some(group) = self.app.selected().cloned() {
                    if self.app.server.owns_server()
                        && self.app.server.snapshot.as_ref().is_some_and(|s| s.router)
                    {
                        let id = crate::llama::router::id(&group);
                        self.app.server.select_id(&id)?;
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
            KeyCode::Char('n') => self.app.discovery.more()?,
            KeyCode::Char('/') => self.app.discovery.editing = true,
            KeyCode::Up => self.app.discovery.move_cursor(false),
            KeyCode::Down => self.app.discovery.move_cursor(true),
            KeyCode::Backspace => self.app.discovery.back(),
            KeyCode::Enter if self.app.discovery.repo.is_none() => self.app.discovery.open()?,
            KeyCode::Char('d' | 'D') if !self.app.discovery.busy() => {
                let files = crate::huggingface::bundle(
                    &self.app.discovery.files,
                    self.app.discovery.cursor,
                );
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
                if key == KeyCode::Char('D') || (roots.len() > 1 && preferred.is_none()) {
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
                downloads.add(files, root)?;
                self.app.notice = "Queued; press r to start".into();
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
            KeyCode::Up => d.cursor = d.cursor.saturating_sub(1),
            KeyCode::Down => d.cursor = (d.cursor + 1).min(d.queue.items.len().saturating_sub(1)),
            KeyCode::Char('r') => d.start()?,
            KeyCode::Char('R') => d.start_queue()?,
            KeyCode::Char('x') => d.pause(),
            KeyCode::Char('z') => d.remove_selected()?,
            _ => {}
        }
        Ok(())
    }
    fn server_key(&mut self, key: KeyCode) -> io::Result<Effect> {
        if self.mutation_active() {
            return Err(io::Error::other("Wait for model quarantine to finish"));
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
            KeyCode::Char('a' | 'A') => {
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
                let mode = if key == KeyCode::Char('A') {
                    crate::agents::TerminalMode::Current
                } else {
                    c.agent_terminal
                };
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
                return Ok(Effect::Launch(plan, mode));
            }
            _ => {}
        }
        Ok(Effect::None)
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
