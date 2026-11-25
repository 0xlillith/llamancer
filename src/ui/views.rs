use crate::app::{App, TABS};
use ratatui::layout::Rect;

pub fn content(app: &App, body: Rect) -> (&'static str, String) {
    if let Some(editor) = &app.profile_editor {
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
                "Up/Down select | Enter edit | a mode | s save | Esc cancel".into(),
            ]);
        }
        ("Inference profile", lines.join("\n"))
    } else if let Some((field, draft)) = &app.edit {
        (
            "Edit settings",
            format!(
                "{}\n\n{}▏\n\nEnter: save   Esc: cancel",
                field.label(),
                draft
            ),
        )
    } else if app.help {
        ("Help", "Tab / arrows / 1–5: navigate\nLibrary: r scan, x cancel, Up/Down select, Enter details\n/: search, s: sort, f: favorite, v: favorites only, p: inference profile, l: start single model, i: invalid files, h: hide model\nDiscover: / search, Enter open, d queue, n next page\nDownloads: r resume, R run queue, x pause, z remove entry\nServer: r refresh, D detach, s router  R restart, i probe, x stop, l/u load/unload, a agent, A current terminal, g logs\nSettings: Up/Down select, Enter edit\n?: toggle help\nEsc: close help or quit\nq / Ctrl+C: quit".into())
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
                            "Hugging Face | Search: {}{}",
                            safe(&d.query),
                            if d.editing { "▏" } else { "" }
                        ),
                        safe(&d.status),
                        "/ search | Enter repository | Backspace back | d queue file/bundle | n next page".into(),
                    ];
                    let capacity = usize::from(body.height.saturating_sub(6)).max(1);
                    let offset = d.cursor.saturating_sub(capacity.saturating_sub(1));
                    if let Some(repo) = &d.repo {
                        lines.push(safe(repo));
                        for (i, f) in d.files.iter().enumerate().skip(offset).take(capacity) {
                            lines.push(format!(
                                "{} {} | {:.2} GiB",
                                if i == d.cursor { ">" } else { " " },
                                safe(&f.name),
                                f.size as f64 / 1073741824.0
                            ));
                        }
                    } else {
                        for (i, r) in d.repos.iter().enumerate().skip(offset).take(capacity) {
                            lines.push(format!(
                                "{} {} | {} downloads{}",
                                if i == d.cursor { ">" } else { " " },
                                safe(&r.id),
                                r.downloads,
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
                            "r resume | R run queue | x pause | z remove entry".into(),
                        ];
                        let capacity = usize::from(body.height.saturating_sub(5)).max(1);
                        let offset = d.cursor.saturating_sub(capacity.saturating_sub(1));
                        for (i, item) in
                            d.queue.items.iter().enumerate().skip(offset).take(capacity)
                        {
                            let status = d
                                .progress(i)
                                .map(|n| {
                                    format!(
                                        "{:.1}% ({:.2}/{:.2} GiB)",
                                        n as f64 / item.file.size as f64 * 100.0,
                                        n as f64 / 1073741824.0,
                                        item.file.size as f64 / 1073741824.0
                                    )
                                })
                                .unwrap_or_else(|| format!("{:?}", item.state));
                            let status = if let Some((speed, eta)) = d.speed_eta(i) {
                                format!(
                                    "{status} | {:.1} MiB/s | ETA {}m {:02}s",
                                    speed / 1048576.0,
                                    eta / 60,
                                    eta % 60
                                )
                            } else {
                                status
                            };
                            lines.push(format!(
                                "{} {} | {}",
                                if i == d.cursor { ">" } else { " " },
                                safe(&item.file.name),
                                status
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
                    "r refresh | D detach | s router  R restart | i probe | x stop | l/u load/unload | a agent | g logs".into()];
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
                        let capacity = usize::from(body.height.saturating_sub(7)).max(1);
                        let offset = server.cursor.saturating_sub(capacity.saturating_sub(1));
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
                    let capacity = usize::from(body.height.saturating_sub(5)).max(1);
                    let offset = app.setting.saturating_sub(capacity.saturating_sub(1));
                    for (i, field) in crate::settings::Field::ALL
                        .into_iter()
                        .enumerate()
                        .skip(offset)
                        .take(capacity)
                    {
                        lines.push(format!(
                            "{} {}: {}",
                            if i == app.setting { ">" } else { " " },
                            field.label(),
                            field.initial(&app.config)
                        ));
                    }
                    lines.push("Up/Down: select   Enter: edit".into());
                    lines.push("Model roots:".into());
                    lines.extend(
                        app.config
                            .model_roots
                            .iter()
                            .map(|p| format!("  {}", p.display())),
                    );
                    lines.join("\n")
                }
            },
        )
    }
}
