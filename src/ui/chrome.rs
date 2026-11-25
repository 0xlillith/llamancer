//! Shared visual chrome and semantic text styles; all geometry stays in ui/mod.rs.
use super::{icons::Icon, safe, Palette};
use crate::app::{App, TABS};
use ratatui::{
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, BorderType, Paragraph, Wrap},
    Frame,
};

pub fn busy(app: &App) -> bool {
    app.scanning
        || app.backend_busy
        || app.discovery.busy()
        || app.server.busy()
        || app.downloads.as_ref().is_some_and(|d| d.busy())
}
fn chip<'a>(p: &Palette, icon: Icon, label: &str, value: impl Into<String>) -> Vec<Span<'a>> {
    vec![
        Span::styled(format!(" {} {label} ", p.icon(icon)), p.label),
        Span::styled(
            format!(" {} ", value.into()),
            p.surface.add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ]
}
pub fn header(frame: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let mut first = vec![
        Span::styled(format!(" {} LLAMANCER ", p.icon(Icon::Brand)), p.brand),
        Span::styled(format!(" v{} ", env!("CARGO_PKG_VERSION")), p.label),
    ];
    let status = if busy(app) {
        format!(
            "{} working",
            ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"][app.activity_frame % 10]
        )
    } else if app.server.snapshot.is_some() {
        "● connected".into()
    } else {
        "○ local / idle".into()
    };
    first.push(Span::styled(
        format!("  {status}  "),
        if busy(app) {
            p.heading
        } else if app.server.snapshot.is_some() {
            p.base.fg(p.green)
        } else {
            p.dim
        },
    ));
    if area.width >= 84 {
        first.push(Span::styled("Summon your models.", p.dim));
    }
    let project = app
        .config
        .project_dir
        .as_ref()
        .and_then(|path| path.file_name())
        .map(|n| safe(&n.to_string_lossy()))
        .unwrap_or_else(|| "none".into());
    let mut second = chip(p, Icon::Library, "MODELS", app.candidates.len().to_string());
    second.extend(chip(
        p,
        Icon::Agent,
        "AGENT",
        safe(&app.config.default_agent),
    ));
    second.extend(chip(p, Icon::Project, "ACTIVE PROJECT", project));
    if area.width >= 120 {
        second.push(Span::styled(
            if app.nerd_icons() {
                " NF "
            } else {
                " Unicode "
            },
            p.dim,
        ));
    }
    frame.render_widget(
        Paragraph::new(vec![Line::from(first), Line::from(second)]).style(p.base),
        area,
    );
}
pub fn tab(frame: &mut Frame, area: Rect, index: usize, app: &App, p: &Palette) {
    let icons = [
        Icon::Library,
        Icon::Discover,
        Icon::Downloads,
        Icon::Server,
        Icon::Settings,
    ];
    let selected = index == app.tab;
    let label = if area.width >= 16 {
        format!(
            "{} {} {} {}",
            if selected { ">" } else { " " },
            p.icon(icons[index]),
            index + 1,
            TABS[index]
        )
    } else if area.width >= 9 {
        format!(
            "{} {} {}",
            if selected { ">" } else { " " },
            index + 1,
            ["Lib", "Find", "DL", "Srv", "Cfg"][index]
        )
    } else {
        format!("{}{}", if selected { ">" } else { " " }, index + 1)
    };
    let style = if selected {
        p.selected.add_modifier(Modifier::BOLD)
    } else {
        p.base
    };
    let border = if selected { p.selected } else { p.border };
    frame.render_widget(
        Paragraph::new(label).centered().style(style).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(border),
        ),
        area,
    );
}
/// Color structured rows without changing their text or hit-testing geometry.
pub fn row(line: &str, p: &Palette, logs: bool) -> Line<'static> {
    let mut style = p.base;
    if line.starts_with("> ") {
        style = p.selected.add_modifier(Modifier::BOLD);
    } else if !logs {
        let lower = line.to_ascii_lowercase();
        if lower.contains("[ok]")
            || lower.ends_with(" | loaded")
            || lower.ends_with(" | complete")
            || lower.ends_with(" | selected")
        {
            style = p.base.fg(p.green);
        } else if lower.contains("insufficient")
            || lower.starts_with("warning:")
            || lower.contains("read-only")
            || lower.ends_with(" | failed")
        {
            style = p.base.fg(p.orange);
        } else if lower.starts_with("backend ")
            || lower.starts_with("endpoint:")
            || lower.starts_with("mode:")
            || lower.starts_with("bundle:")
            || lower.starts_with("latest model sha-256:")
        {
            style = p.heading;
        }
    }
    let line = safe(line);
    if !logs && !line.starts_with("> ") {
        if let Some((label, value)) = line.split_once(": ") {
            if label.len() <= 35
                && !label.contains('/')
                && !label.contains('[')
                && !label.starts_with(' ')
            {
                return Line::from(vec![
                    Span::styled(format!("{label}: "), p.muted),
                    Span::styled(value.to_owned(), style),
                ]);
            }
        }
    }
    Line::styled(line, style)
}
pub fn meter(value: u64, total: u64, width: usize) -> String {
    if total == 0 {
        return "─".repeat(width);
    }
    let filled = ((value.min(total) as u128 * width as u128) / total as u128) as usize;
    format!("{}{}", "━".repeat(filled), "─".repeat(width - filled))
}
fn key_lines(hints: &str, p: &Palette, width: u16, max: usize) -> Vec<Line<'static>> {
    let mut result = Vec::new();
    let mut spans = Vec::new();
    let mut used = 0usize;
    let width = usize::from(width);
    for part in hints.split("  ").filter(|s| !s.is_empty()) {
        let (key, action) = part.split_once(' ').unwrap_or((part, ""));
        let mut pair = vec![Span::styled(
            format!(" {key} "),
            p.label.add_modifier(Modifier::BOLD),
        )];
        if !action.is_empty() {
            pair.push(Span::styled(format!(" {action} "), p.base));
        }
        let n: usize = pair.iter().map(Span::width).sum();
        if used + n > width && !spans.is_empty() {
            result.push(Line::from(std::mem::take(&mut spans)));
            used = 0;
        }
        if result.len() == max {
            break;
        }
        used += n;
        spans.extend(pair);
    }
    if result.len() < max && !spans.is_empty() {
        result.push(Line::from(spans));
    }
    result
}
pub fn footer(frame: &mut Frame, area: Rect, hints: &str, app: &App, p: &Palette) {
    let notice = if app.launching_agent {
        format!(
            "{} · waiting to open agent; Esc cancels",
            safe(&app.server.status)
        )
    } else if app.tab == 2 && app.downloads.as_ref().is_some_and(|d| d.busy()) {
        app.downloads
            .as_ref()
            .map(|d| safe(&d.status))
            .unwrap_or_default()
    } else if !app.notice.is_empty() {
        safe(&app.notice)
    } else if let Some((n, t)) = app.backend_progress {
        format!(
            "{}  {:.1}/{:.1} MiB",
            meter(n, t, 16),
            n as f64 / 1048576.0,
            t as f64 / 1048576.0
        )
    } else if busy(app) {
        "Working…  •  operations remain cancellable where indicated".into()
    } else if app.backend_open {
        safe(
            app.backend_status
                .lines()
                .next()
                .unwrap_or("Backend manager"),
        )
    } else if app.tab == 2 {
        app.downloads
            .as_ref()
            .map(|d| safe(&d.status))
            .unwrap_or_default()
    } else if app.tab == 3 {
        safe(&app.server.status)
    } else {
        "Ready  •  ? help  •  G setup  •  q quit".into()
    };
    let notice = if notice.is_empty() {
        "Ready".into()
    } else {
        notice
    };
    let status = Line::from(vec![
        Span::styled(" STATUS ", p.brand),
        Span::styled(format!(" {notice}"), p.surface),
    ]);
    let mut lines = vec![status];
    lines.extend(key_lines(
        hints,
        p,
        area.width,
        usize::from(area.height.saturating_sub(1)),
    ));
    frame.render_widget(Paragraph::new(lines).style(p.base), area);
}
pub fn aside(frame: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let mut lines = Vec::new();
    let title;
    if app.setup_open {
        title = "Setup progress";
        let states = crate::setup::statuses(app);
        let done = states.iter().filter(|&&s| s).count();
        lines.push(Line::styled(
            format!("{done} / {} checks ready", states.len()),
            p.heading,
        ));
        lines.push(Line::styled(
            meter(done as u64, states.len() as u64, 20),
            p.base.fg(p.green),
        ));
        lines.push(Line::raw(""));
        lines.push(Line::styled("Local-first setup", p.muted));
        lines.push(Line::raw("Select a step to open the corresponding setting or manager. Nothing is installed automatically."));
        lines.push(Line::raw(""));
        lines.push(Line::raw("Esc skips. G reopens this checklist."));
    } else if let Some(projects) = &app.projects {
        title = "Workspace";
        lines.push(Line::styled(
            format!("{} folders", projects.len()),
            p.heading,
        ));
        lines.push(Line::raw(""));
        lines.push(Line::styled("Parent directory", p.muted));
        lines.push(Line::raw(
            app.config
                .workspace_root
                .as_ref()
                .map(|x| safe(&x.to_string_lossy()))
                .unwrap_or_else(|| "not configured".into()),
        ));
        lines.push(Line::raw(""));
        lines.push(Line::styled("Selected folder", p.muted));
        lines.push(Line::raw(
            projects
                .get(app.project_cursor)
                .map(|x| safe(&x.to_string_lossy()))
                .unwrap_or_else(|| "none".into()),
        ));
        lines.push(Line::raw(""));
        lines.push(Line::raw(
            "The chosen folder becomes the agent working directory.",
        ));
    } else if let Some(choice) = &app.download_destination {
        title = "Storage plan";
        lines.push(Line::styled(
            format!("{} files", choice.files.len()),
            p.heading,
        ));
        lines.push(Line::raw(format!(
            "{:.2} GiB required",
            choice.required as f64 / 1073741824.0
        )));
        if let Some(Some(free)) = choice.available.get(choice.cursor) {
            lines.push(Line::raw(format!(
                "{:.2} GiB free",
                *free as f64 / 1073741824.0
            )));
            lines.push(Line::styled(
                meter(choice.required, *free, 20),
                p.base.fg(if *free < choice.required {
                    p.orange
                } else {
                    p.green
                }),
            ));
        }
        lines.push(Line::raw(""));
        lines.push(Line::raw(
            "Queued files keep this destination, even if your default changes.",
        ));
    } else if app.benchmark_open {
        title = "Method";
        lines.push(Line::styled("Same model, comparable runs", p.heading));
        lines.push(Line::raw(""));
        for line in [
            "PP 128  ·  prompt processing",
            "TG 32   ·  token generation",
            "2 CPU threads / 2 repetitions",
            "0 CPU or 8 GPU layers",
            "Latest run per backend",
        ] {
            lines.push(Line::raw(line));
        }
        lines.push(Line::raw(""));
        lines.push(Line::raw(
            "Unmeasured means no result, not zero throughput.",
        ));
    } else if app.backend_open {
        title = "Runtime";
        lines.push(Line::styled("Managed backends", p.heading));
        lines.push(Line::raw(safe(&app.backend_hardware)));
        lines.push(Line::raw(""));
        lines.push(Line::styled("Safety", p.muted));
        lines.push(Line::raw(
            "Pinned archives, isolated installs. Device probe is required before selection.",
        ));
        if let Some((n, t)) = app.backend_progress {
            lines.push(Line::styled(meter(n, t, 20), p.heading));
        }
    } else {
        match app.tab {
            1 => {
                title = "Repository";
                let d = &app.discovery;
                if let Some(repo) = &d.repo {
                    lines.push(Line::styled(safe(repo), p.heading));
                    if let Some(v) = d.variants.get(d.cursor) {
                        let f = &v.files[0];
                        lines.push(Line::raw(""));
                        lines.push(Line::styled("Selected variant", p.muted));
                        lines.push(Line::raw(safe(&v.name)));
                        lines.push(Line::raw(format!(
                            "{:.2} GiB total · {}/{} files",
                            v.bytes() as f64 / 1073741824.0,
                            v.files.len(),
                            v.expected
                        )));
                        lines.push(Line::raw("Per-variant popularity unavailable"));
                        lines.push(Line::raw(if f.sha256.is_some() {
                            "SHA-256 available"
                        } else {
                            "No SHA-256 reported"
                        }));
                    }
                } else if let Some(r) = d.repos.get(d.cursor) {
                    lines.push(Line::styled(safe(&r.id), p.heading));
                    lines.push(Line::raw(format!(
                        "{} downloads · {} likes",
                        r.downloads, r.likes
                    )));
                    if let Some(score) = r.trending_score {
                        lines.push(Line::raw(format!("Trending score: {score:.1}")));
                    }
                    if let Some(date) = r.created_at.as_deref() {
                        lines.push(Line::raw(format!(
                            "Created: {}",
                            safe(date.split('T').next().unwrap_or(date))
                        )));
                    }
                    if let Some(date) = r.updated_at.as_deref() {
                        lines.push(Line::raw(format!(
                            "Updated: {}",
                            safe(date.split('T').next().unwrap_or(date))
                        )));
                    }
                } else {
                    lines.push(Line::styled("Discover GGUF models", p.heading));
                    lines.push(Line::raw(
                        "Search names, @owner/repo, or filters such as quant:Q4_K_M.",
                    ));
                }
            }
            2 => {
                title = "Transfer";
                if let Some(d) = &app.downloads {
                    let groups = d.bundles();
                    lines.push(Line::styled(
                        format!("{} model downloads", groups.len()),
                        p.heading,
                    ));
                    if let Some(g) = groups.get(d.bundle_cursor(&groups)) {
                        lines.push(Line::raw(""));
                        lines.push(Line::styled(safe(&g.name), p.heading));
                        lines.push(Line::raw(format!(
                            "{} · {:.2} GiB",
                            g.label(),
                            g.bytes as f64 / 1073741824.0
                        )));
                        lines.push(Line::styled(meter(g.received, g.bytes, 28), p.heading));
                        lines.push(Line::raw(format!(
                            "{:.2}/{:.2} GiB",
                            g.received as f64 / 1073741824.0,
                            g.bytes as f64 / 1073741824.0
                        )));
                        if let Some(i) = g.active {
                            if let Some((graph, peak)) = d.speed_history(i) {
                                lines.push(Line::raw("Recent transfer rate"));
                                lines.push(Line::styled(graph, p.heading));
                                lines
                                    .push(Line::raw(format!("Peak {:.1} MiB/s", peak / 1048576.0)));
                            }
                            if let Some((speed, _)) = d.speed_eta(i) {
                                lines.push(Line::raw(format!(
                                    "{:.1} MiB/s (current part)",
                                    speed / 1048576.0
                                )));
                            }
                        }
                        lines.push(Line::raw(""));
                        lines.push(Line::styled("Parts", p.muted));
                        for &i in &g.indices {
                            let item = &d.queue.items[i];
                            let part =
                                crate::models::group::shard(std::path::Path::new(&item.file.name))
                                    .map(|(_, n, t)| format!("{n}/{t}"))
                                    .unwrap_or_else(|| "1/1".into());
                            lines.push(Line::raw(format!("{part} · {:?}", item.state)));
                            if let Some(err) = &item.error {
                                lines.push(Line::styled(safe(err), p.base.fg(p.orange)));
                            }
                        }
                        lines.push(Line::raw(""));
                        lines.push(Line::styled("Destination", p.muted));
                        lines.push(Line::raw(safe(
                            &d.queue.items[g.indices[0]].root.to_string_lossy(),
                        )));
                    }
                }
            }
            3 => {
                title = "Inference";
                let s = &app.server;
                lines.push(Line::styled(
                    if s.owns_server() {
                        "Managed session"
                    } else {
                        "External / stopped"
                    },
                    p.heading,
                ));
                lines.push(Line::raw(safe(&s.status)));
                lines.push(Line::raw(""));
                lines.push(Line::styled("Device", p.muted));
                lines.push(Line::raw(safe(&app.config.device)));
                if let Some(model) = s.selected() {
                    lines.push(Line::raw(""));
                    lines.push(Line::styled("Selected model", p.muted));
                    lines.push(Line::raw(safe(&model.id)));
                    lines.push(Line::raw(safe(model.state())));
                    if let Some(n) = model.context_limit() {
                        lines.push(Line::raw(format!("Context {n}")));
                    }
                }
                lines.push(Line::raw(""));
                lines.push(Line::styled("Agent", p.muted));
                lines.push(Line::raw(safe(&app.config.default_agent)));
                lines.push(Line::raw("P project · a launch"));
            }
            _ => {
                title = "Preferences";
                lines.push(Line::styled("Appearance", p.heading));
                lines.push(Line::raw(format!("Theme: {}", safe(&app.config.theme))));
                lines.push(Line::raw(format!(
                    "Icons: {} ({})",
                    app.config.icon_mode.label(),
                    if app.nerd_icons() {
                        "Nerd Font"
                    } else {
                        "portable"
                    }
                )));
                lines.push(Line::from(vec![
                    Span::styled("  ●  ", p.brand),
                    Span::styled("  ●  ", p.label),
                    Span::styled("  ●  ", p.heading),
                    Span::styled("  ●  ", p.base.fg(p.green)),
                ]));
                lines.push(Line::raw("T cycle theme · I cycle icons"));
                lines.push(Line::raw(""));
                lines.push(Line::styled("Model storage", p.muted));
                lines.push(Line::raw(format!(
                    "{} roots · {} imported",
                    app.config.model_roots.len(),
                    app.config.model_imports.len()
                )));
                lines.push(Line::raw(""));
                lines.push(Line::raw("G setup · B backends · P projects"));
                if let Some(field) = crate::settings::Field::ALL.get(app.setting) {
                    lines.push(Line::raw(""));
                    lines.push(Line::styled("Selected setting", p.heading));
                    lines.push(Line::raw(field.label()));
                    lines.push(Line::raw(safe(&field.initial(&app.config))));
                }
            }
        }
    }
    frame.render_widget(
        Paragraph::new(lines)
            .style(p.base)
            .wrap(Wrap { trim: false })
            .block(p.panel(format!(" {title} "))),
        area,
    );
}
