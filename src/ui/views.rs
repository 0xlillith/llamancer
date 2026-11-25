use crate::app::{App, TABS};
use ratatui::layout::Rect;

pub fn content(app: &App, body: Rect) -> (&'static str, String) {
    if let Some(g) = &app.launch_menu {
        let mut lines = vec![
            format!(
                "{} · {:.2} GiB",
                super::safe(&g.name),
                g.bytes() as f64 / 1073741824.0
            ),
            format!(
                "Agent: {} · separate window",
                super::safe(&app.config.default_agent)
            ),
            String::new(),
        ];
        for (i, label) in [
            "Run model + open coding agent",
            "Run model only",
            "Inspect model details",
        ]
        .iter()
        .enumerate()
        {
            lines.push(format!(
                "{} {label}",
                if i == app.launch_cursor { ">" } else { " " }
            ));
        }
        lines.push(String::new());
        if app.server.owns_server()
            && app.server.snapshot.as_ref().is_some_and(|s| !s.router)
            && !app.server.is_single_model(g)
        {
            lines.push(
                "Starting another model replaces the current managed single-model server.".into(),
            );
        }
        if app.config.api_key_file.is_some() {
            lines.push("Authenticated agents: Run model only, then explicit A in Server.".into());
        }
        lines.push(if g.complete && !g.projector {
            "Enter or click an action. Esc goes back.".into()
        } else {
            "Incomplete model or projector: inspection only.".into()
        });
        ("Launch model", lines.join("\n"))
    } else if app.setup_open {
        let (offset, capacity) = super::viewport(app.setup_cursor, body.height, 4);
        let statuses = crate::setup::statuses(app);
        let mut lines =
            vec!["Configure only what you need. No automatic downloads or driver changes.".into()];
        for (i, label) in crate::setup::LABELS
            .iter()
            .enumerate()
            .skip(offset)
            .take(capacity)
        {
            lines.push(format!(
                "{} [{}] {label}",
                if i == app.setup_cursor { ">" } else { " " },
                if statuses[i] { "OK" } else { "  " }
            ));
        }
        ("Guided setup", lines.join("\n"))
    } else if app.benchmark_open {
        (
            "Comparable benchmark history",
            crate::benchmark::comparison(&app.config.benchmarks),
        )
    } else if let Some(report) = &app.fit_report {
        ("Model memory advisory", report.clone())
    } else if let Some(choice) = &app.download_destination {
        let (offset, capacity) = super::viewport(choice.cursor, body.height, 4);
        let mut lines = vec![format!(
            "Bundle: {} files | {:.2} GiB required",
            choice.files.len(),
            choice.required as f64 / 1073741824.0
        )];
        for (i, root) in choice.roots.iter().enumerate().skip(offset).take(capacity) {
            let space = choice.available[i]
                .map(|n| {
                    format!(
                        "{:.2} GiB free{}",
                        n as f64 / 1073741824.0,
                        if n < choice.required {
                            " (insufficient)"
                        } else {
                            ""
                        }
                    )
                })
                .unwrap_or_else(|| "unavailable / read-only".into());
            lines.push(format!(
                "{} {} | {}",
                if i == choice.cursor { ">" } else { " " },
                space,
                super::safe(&root.to_string_lossy())
            ));
        }
        ("Download destination", lines.join("\n"))
    } else if let Some(projects) = &app.projects {
        let (offset, capacity) = super::viewport(app.project_cursor, body.height, 4);
        let mut lines = vec!["Choose a direct project folder (up to 512).".into()];
        if projects.is_empty() {
            lines.push("No visible project folders found.".into());
        }
        for (i, p) in projects.iter().enumerate().skip(offset).take(capacity) {
            lines.push(format!(
                "{} {}{}",
                if i == app.project_cursor { ">" } else { " " },
                super::safe(&p.file_name().unwrap_or_default().to_string_lossy()),
                if Some(p) == app.config.project_dir.as_ref() {
                    " [active]"
                } else {
                    ""
                }
            ));
        }
        ("Project workspace", lines.join("\n"))
    } else if app.backend_open {
        use crate::backends::{self, Kind};
        let mut lines = vec![
            format!(
                "Official llama.cpp {} | {}",
                backends::RELEASES[app.backend_version],
                super::safe(&app.backend_hardware)
            ),
            String::new(),
        ];
        for (i, kind) in Kind::ALL.into_iter().enumerate() {
            let status = if let Some(asset) =
                backends::catalog_version(kind, backends::RELEASES[app.backend_version])
            {
                let installed = app
                    .backend_root
                    .as_ref()
                    .is_some_and(|root| asset.installed(root));
                let selected = app.backend_root.as_ref().is_some_and(|root| {
                    asset.binary(root).to_string_lossy() == app.config.server_binary
                });
                format!(
                    "{} | {:.1} MiB{}",
                    if installed {
                        "installed"
                    } else {
                        "not installed"
                    },
                    asset.total_size() as f64 / 1048576.0,
                    if selected { " | SELECTED" } else { "" }
                )
            } else {
                "unavailable on this architecture".into()
            };
            lines.push(format!(
                "{} {:7} {status}",
                if i == app.backend_cursor { ">" } else { " " },
                kind.name()
            ));
        }
        lines.push(format!(
            "v/u: switch catalog version ({}/{}) | c: check upstream",
            app.backend_version + 1,
            backends::RELEASES.len()
        ));
        lines.push(String::new());
        if let Some(action) = app.backend_confirmation {
            let label = match action {
                'i' => "Download and install checksum-pinned archive",
                'd' => "Delete selected managed backend (stop all inference first)",
                _ => "Benchmark selected Library model (max 512 MiB)",
            };
            lines.push(format!("{label}? y confirm / n cancel"));
        } else if let Some((bytes, total)) = app.backend_progress {
            lines.push(format!(
                "Download: {} {:.1}/{:.1} MiB ({:.0}%)",
                super::chrome::meter(bytes, total, 12),
                bytes as f64 / 1048576.0,
                total as f64 / 1048576.0,
                100.0 * bytes as f64 / total as f64
            ));
            lines.push("Verifying and extracting after download; x cancel".into());
        }
        lines.extend(app.backend_status.lines().map(super::safe));
        ("Backend Manager", lines.join("\n"))
    } else if let Some(restore) = app.quarantine_confirmation {
        let mut lines=vec![if restore{"Restore all selected managed shards?"}else{"Quarantine all selected managed shards?"}.into(),"Stop all external inference first. Files are renamed, never deleted; disk space is retained.".into(),"y confirm | n/Esc cancel".into()];
        if let Some(d) = &app.downloads {
            if let Ok(indices) = d.quarantine_targets() {
                for index in indices {
                    lines.push(super::safe(&d.queue.items[index].file.name));
                }
            }
        }
        ("Confirm model movement", lines.join("\n"))
    } else if let Some(editor) = &app.profile_editor {
        let selected = editor.selected();
        let filename = editor
            .id
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .chars()
            .map(|c| if c.is_control() { '�' } else { c })
            .collect::<String>();
        let mut lines = vec![
            filename,
            format!(
                "{} mode | auto = inherit backend default",
                if editor.advanced { "Advanced" } else { "Basic" }
            ),
        ];
        if let Some(input) = &editor.input {
            lines.extend([
                selected.label().into(),
                format!("{input}▏"),
                selected.hint().into(),
                "Enter: apply to draft | Esc: back | Ctrl+U: clear".into(),
            ]);
        } else {
            let capacity = usize::from(body.height.saturating_sub(7)).max(1);
            let offset = editor.cursor.saturating_sub(capacity.saturating_sub(1));
            for (index, field) in editor
                .fields()
                .iter()
                .enumerate()
                .skip(offset)
                .take(capacity)
            {
                lines.push(format!(
                    "{} {}: {}",
                    if index == editor.cursor { ">" } else { " " },
                    field.label(),
                    field.value(&editor.draft)
                ));
            }
            lines.extend([
                selected.hint().into(),
                "Up/Down select | Enter edit | a mode | m projector candidate | s save | Esc cancel".into(),
            ]);
        }
        ("Inference profile", lines.join("\n"))
    } else if let Some((field, draft)) = &app.edit {
        (
            "Edit settings",
            format!(
                "{}\n\n{}▏\n\n{}\n\nEnter: save   Esc: cancel",
                field.label(),
                draft,
                if *field == crate::settings::Field::AddRoot {
                    format!(
                        "{} Suggested: {}",
                        field.hint(),
                        crate::settings::default_model_root()
                            .map(|p| super::safe(&p.to_string_lossy()))
                            .unwrap_or_else(|_| "unavailable".into())
                    )
                } else {
                    field.hint().into()
                }
            ),
        )
    } else if app.help {
        ("Help", "Tab / arrows / 1–5: navigate; click tab or row, scroll to select\nLibrary: r scan, x cancel, Up/Down select, Enter launch menu, j details (PgUp/PgDn scroll)\n/: search, s: sort, f: favorite, v: favorites only, p: inference profile, m: memory advisory, l: start single model, i: invalid files, h: hide model, t: quarantine\nDiscover: / search, Enter open, d queue, n next page\nDownloads: r resume, R run queue, x pause, t quarantine, T restore, z remove entry\nServer: r refresh, D detach, s router  R restart, i probe, x stop, l/u load/unload, a agent, A current terminal, g logs\nG guided setup (any view). Settings: Up/Down select, Enter edit; T next theme; I icon mode; B backend manager, P project picker (also Server)\n?: toggle help\nEsc: close help or quit\nq / Ctrl+C: quit".into())
    } else {
        (
            TABS[app.tab],
            match app.tab {
                0 => "Library".into(),
                1 => {
                    let d = &app.discovery;
                    let safe = |s: &str| {
                        s.chars()
                            .map(|c| if c.is_control() { '�' } else { c })
                            .collect::<String>()
                    };
                    let mut lines = vec![
                        format!(
                            "{} [{}{}]",
                            if d.repo.is_some() {
                                "Filter variants"
                            } else {
                                "Search models"
                            },
                            super::search_value(
                                if d.repo.is_some() {
                                    &d.variant_filter
                                } else {
                                    &d.query
                                },
                                d.editing,
                                usize::from(body.width.saturating_sub(23))
                            ),
                            if d.editing { "▏" } else { "" }
                        ),
                        format!(
                            "{} | {}",
                            if d.repo.is_some() {
                                d.variant_sort.label()
                            } else {
                                d.ranking.label()
                            },
                            safe(&d.status)
                        ),
                        if d.repo.is_some() {
                            "Click filter | s sort | Enter/d download | Backspace back".into()
                        } else {
                            "Click search | s ranking | author:owner | license:mit | quant:Q4_K_M | n next".into()
                        },
                    ];
                    let (offset, capacity) = super::viewport(d.cursor, body.height, 6);
                    if let Some(repo) = &d.repo {
                        lines.push(safe(repo));
                        for (i, v) in d.variants.iter().enumerate().skip(offset).take(capacity) {
                            lines.push(format!(
                                "{} {:>8.2} GiB | {}/{} | {}{}",
                                if i == d.cursor { ">" } else { " " },
                                v.bytes() as f64 / 1073741824.0,
                                v.files.len(),
                                v.expected,
                                safe(&v.name),
                                if v.complete() { "" } else { " [incomplete]" }
                            ));
                        }
                    } else {
                        for (i, r) in d.repos.iter().enumerate().skip(offset).take(capacity) {
                            lines.push(format!(
                                "{} {:>9} DL | {:>6} likes | {}{}",
                                if i == d.cursor { ">" } else { " " },
                                r.downloads,
                                r.likes,
                                safe(&r.id),
                                if r.gated == serde_json::Value::Bool(false) || r.gated.is_null() {
                                    ""
                                } else {
                                    " | Gated"
                                }
                            ));
                        }
                    }
                    lines.join("\n")
                }
                2 => {
                    if let Some(d) = &app.downloads {
                        let safe = |s: &str| {
                            s.chars()
                                .map(|c| if c.is_control() { '�' } else { c })
                                .collect::<String>()
                        };
                        let mut lines = vec![
                            safe(&d.status),
                            "Enter/r resume model | R all queued | x pause | t quarantine | T restore | z remove model".into(),
                        ];
                        let groups = d.bundles();
                        let cursor = d.bundle_cursor(&groups);
                        let (offset, capacity) = super::viewport(cursor, body.height, 5);
                        for (i, g) in groups.iter().enumerate().skip(offset).take(capacity) {
                            lines.push(format!(
                                "{} {:<21} {} | {:>6.1}% | {:.2}/{:.2} GiB | {}",
                                if i == cursor { ">" } else { " " },
                                g.label(),
                                super::chrome::meter(g.received, g.bytes, 12),
                                100.0 * g.received as f64 / g.bytes.max(1) as f64,
                                g.received as f64 / 1073741824.0,
                                g.bytes as f64 / 1073741824.0,
                                safe(&g.name)
                            ));
                        }
                        if d.queue.items.is_empty() {
                            lines.push("Discover models and queue files with d.".into());
                        }
                        lines.join("\n")
                    } else {
                        "Queue unavailable".into()
                    }
                }
                3 => {
                    let server = &app.server;
                    let safe = |s: &str| {
                        s.chars()
                            .map(|c| if c.is_control() { '�' } else { c })
                            .collect::<String>()
                    };
                    let mut lines=vec![format!("Endpoint: {}",safe(&app.config.endpoint)),format!("{} | {}",if server.owns_server(){"Managed session"}else{"External / stopped"},safe(&server.status)),
                    "s Start router | B choose backend | x Stop | a Launch agent | w Web UI | g logs".into()];
                    if server.show_logs {
                        lines.extend(
                            server
                                .logs
                                .iter()
                                .rev()
                                .take(usize::from(body.height.saturating_sub(5)))
                                .rev()
                                .map(|s| safe(s)),
                        );
                    } else if let Some(snapshot) = &server.snapshot {
                        lines.push(format!(
                            "Mode: {}",
                            if snapshot.router {
                                "Router"
                            } else {
                                "Single model"
                            }
                        ));
                        let (offset, capacity) = super::viewport(server.cursor, body.height, 7);
                        for (i, model) in snapshot
                            .models
                            .iter()
                            .enumerate()
                            .skip(offset)
                            .take(capacity)
                        {
                            lines.push(format!(
                                "{} {} | {}",
                                if i == server.cursor { ">" } else { " " },
                                safe(&model.id),
                                safe(model.state())
                            ));
                        }
                    }
                    if !server.owns_server() && server.snapshot.is_none() {
                        lines.push(
                            "To run a single model: Library (1) → select model → Enter → Run model."
                                .into(),
                        );
                        lines
                            .push("For multiple models: B choose backend → s Start router.".into());
                    }
                    if let Some(project) = &app.config.project_dir {
                        lines.push(format!(
                            "Project: {} | P switch",
                            safe(&project.to_string_lossy())
                        ));
                    } else if app.config.workspace_root.is_some() {
                        lines.push("Project: select with P before launching an agent".into());
                    }
                    if !server.version.is_empty() {
                        lines.push(format!("Build: {}", safe(&server.version)));
                    }
                    if !server.devices.is_empty() {
                        lines.push(format!("Device preference: {}", safe(&app.config.device)));
                        lines.extend(server.devices.lines().map(safe));
                    }
                    lines.join("\n")
                }
                _ => {
                    let mut lines = Vec::new();
                    let (offset, capacity) = super::viewport(app.setting, body.height, 5);
                    for (i, field) in crate::settings::Field::ALL
                        .into_iter()
                        .enumerate()
                        .skip(offset)
                        .take(capacity)
                    {
                        lines.push(format!(
                            "{} {:<10} | {}: {}",
                            if i == app.setting { ">" } else { " " },
                            field.category(),
                            field.label(),
                            super::safe(&field.initial(&app.config))
                        ));
                    }
                    lines.push(format!(
                        "Hint: {}",
                        crate::settings::Field::ALL[app.setting].hint()
                    ));
                    lines.push("Model roots:".into());
                    lines.extend(
                        app.config
                            .model_roots
                            .iter()
                            .map(|p| format!("  {}", super::safe(&p.to_string_lossy()))),
                    );
                    lines.push(format!(
                        "Imported file references: {}",
                        app.config.model_imports.len()
                    ));
                    lines.extend(
                        app.config
                            .model_imports
                            .iter()
                            .map(|p| format!("  {}", super::safe(&p.to_string_lossy()))),
                    );
                    lines.push(format!("Hidden models: {}", app.config.hidden_models.len()));
                    lines.extend(
                        app.config
                            .hidden_models
                            .iter()
                            .map(|p| format!("  {}", super::safe(&p.to_string_lossy()))),
                    );
                    lines.join("\n")
                }
            },
        )
    }
}
