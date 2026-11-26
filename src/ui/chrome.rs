//! Shared visual chrome and semantic text styles; all geometry stays in ui/mod.rs.
use super::{icons::Icon, safe, Palette};
use crate::app::{App, TABS};
use ratatui::{
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Gauge, Paragraph, Sparkline, Wrap},
    Frame,
};

pub fn busy(app: &App) -> bool {
    app.scanning
        || app.folder_browsing
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
    let interior = Rect::new(
        area.x,
        area.y.saturating_add(1),
        area.width,
        area.height.min(2).saturating_sub(1),
    );
    let style = if selected { p.brand } else { p.surface };
    frame.render_widget(Paragraph::new(label).centered().style(style), interior);
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
pub fn transfer_size(n: u64, total: u64) -> String {
    let (unit, divisor) = if total >= 1024_u64.pow(3) {
        ("GiB", 1024_f64.powi(3))
    } else if total >= 1024_u64.pow(2) {
        ("MiB", 1024_f64.powi(2))
    } else {
        ("KiB", 1024.)
    };
    format!(
        "{:.1}/{:.1} {unit}",
        n as f64 / divisor,
        total as f64 / divisor
    )
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
            safe(if app.notice.starts_with("Checking tool arguments") {
                &app.notice
            } else {
                &app.server.status
            })
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
fn transfer_dashboard(frame: &mut Frame, area: Rect, app: &App, p: &Palette) {
    frame.render_widget(p.panel(" Transfer dashboard ").style(p.base), area);
    if area.width < 4 || area.height < 5 {
        return;
    }
    let inner = Rect::new(area.x + 1, area.y + 1, area.width - 2, area.height - 2);
    let Some(d) = &app.downloads else { return };
    if let Some(a) = d.audit_progress() {
        let lines = vec![
            Line::styled("Integrity audit", p.heading),
            Line::raw(format!("Part {} / {}", a.part, a.parts)),
            Line::raw(safe(&a.name)),
            Line::styled(a.stage, p.heading),
        ];
        frame.render_widget(
            Paragraph::new(lines)
                .style(p.base)
                .wrap(Wrap { trim: true }),
            Rect::new(inner.x, inner.y, inner.width, inner.height.min(5)),
        );
        if inner.height >= 7 {
            let ratio = (a.bytes as f64 / a.total.max(1) as f64).clamp(0., 1.);
            frame.render_widget(
                Gauge::default()
                    .ratio(ratio)
                    .label(transfer_size(a.bytes, a.total))
                    .gauge_style(p.base.fg(p.green)),
                Rect::new(inner.x, inner.y + 5, inner.width, 1),
            );
        }
        return;
    }
    let groups = d.bundles();
    let Some(g) = groups.get(d.bundle_cursor(&groups)) else {
        frame.render_widget(
            Paragraph::new("No downloads yet. Open Discover.")
                .wrap(Wrap { trim: true })
                .style(p.dim),
            inner,
        );
        return;
    };
    let state_color = match g.state {
        "Complete" => p.green,
        "Failed" | "Integrity failed" => p.orange,
        _ => p.heading.fg.unwrap_or(p.green),
    };
    let summary = vec![
        Line::styled(super::clip(&g.name, usize::from(inner.width)), p.heading),
        Line::from(vec![
            Span::styled(
                g.label(),
                p.base.fg(state_color).add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(" · {}", transfer_size(g.received, g.bytes))),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(summary).style(p.base),
        Rect::new(inner.x, inner.y, inner.width, 2),
    );
    let ratio = if g.bytes == 0 {
        0.0
    } else {
        (g.received as f64 / g.bytes as f64).clamp(0.0, 1.0)
    };
    let label = if g.state == "Hub cache" {
        "Hub cache · transfer size unknown".into()
    } else if g.state == "Verifying" {
        if g.active
            .is_some_and(|i| d.queue.items[i].file.sha256.is_some())
        {
            "Verifying SHA-256".into()
        } else {
            "Checking file size".into()
        }
    } else if g.state == "Failed" && ratio >= 1.0 {
        "Transfer failed · inspect parts".into()
    } else {
        format!("{:.1}% transferred", ratio * 100.0)
    };
    frame.render_widget(
        Gauge::default()
            .ratio(ratio)
            .label(label)
            .gauge_style(p.base.fg(state_color).add_modifier(Modifier::BOLD)),
        Rect::new(inner.x, inner.y + 2, inner.width, 1),
    );
    let mut y = inner.y + 4;
    if let Some(i) = g.active {
        let samples = d.speed_samples(i);
        if y + 5 < inner.bottom() {
            let text = if let Some((speed, _)) = d.speed_eta(i) {
                let secs = (g.bytes.saturating_sub(g.received) as f64 / speed).ceil() as u64;
                format!(
                    "{:.1} MiB/s  •  ETA {}m {}s",
                    speed / 1048576.0,
                    secs / 60,
                    secs % 60
                )
            } else if g.state == "Hub cache" {
                "Fetching / validating Hub cache…".into()
            } else if g.state == "Verifying" {
                "Checking downloaded bytes…".into()
            } else {
                "Measuring transfer speed…".into()
            };
            frame.render_widget(
                Paragraph::new(text).style(p.heading),
                Rect::new(inner.x, y, inner.width, 1),
            );
            y += 1;
            if !samples.is_empty() {
                frame.render_widget(
                    Sparkline::default()
                        .data(&samples)
                        .style(p.base.fg(state_color)),
                    Rect::new(inner.x, y, inner.width, 3),
                );
                y += 4;
            }
        }
    }
    if y >= inner.bottom() {
        return;
    }
    let mut detail = vec![Line::styled("PARTS", p.muted)];
    for &i in &g.indices {
        let item = &d.queue.items[i];
        let part = crate::models::group::shard(std::path::Path::new(&item.file.name))
            .map(|(_, n, t)| format!("{n}/{t}"))
            .unwrap_or_else(|| "1/1".into());
        let (symbol, style) = match item.state {
            crate::downloads::State::Complete => ("✓", p.base.fg(p.green)),
            crate::downloads::State::Failed => ("!", p.base.fg(p.orange)),
            _ => ("·", p.base),
        };
        detail.push(Line::styled(
            format!("{symbol} Part {part} · {:?}", item.state),
            style,
        ));
        if let Some(e) = item.integrity_error.as_ref().or(item.error.as_ref()) {
            detail.push(Line::styled(safe(e), p.base.fg(p.orange)));
        }
    }
    detail.push(Line::raw(""));
    detail.push(Line::styled("DESTINATION", p.muted));
    if let Some(&i) = g.indices.first() {
        detail.push(Line::raw(safe(&d.queue.items[i].root.to_string_lossy())));
    }
    frame.render_widget(
        Paragraph::new(detail)
            .style(p.base)
            .wrap(Wrap { trim: false }),
        Rect::new(inner.x, y, inner.width, inner.bottom() - y),
    );
}
pub fn aside(frame: &mut Frame, area: Rect, app: &App, p: &Palette) {
    if app.tab == 2
        && !app.setup_open
        && !app.backend_open
        && app.download_destination.is_none()
        && app.projects.is_none()
        && !app.benchmark_open
    {
        transfer_dashboard(frame, area, app, p);
        return;
    }
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
                        if v.draft() {
                            lines.push(Line::raw("MTP draft: attachment not supported"));
                        } else if v.projector() {
                            lines.push(Line::raw("Optional vision projector"));
                        }
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
                title = "Transfers";
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
                if let Some(n) = crate::models::fit::available_ram() {
                    lines.push(Line::raw(""));
                    lines.push(Line::styled("System memory", p.muted));
                    lines.push(Line::raw(format!(
                        "{:.1} GiB available",
                        n as f64 / 1073741824.0
                    )));
                }
                if let Some(n) = s.active_context {
                    lines.push(Line::raw(format!("Requested context: {n}")));
                }
                if let Some(props) = s
                    .selected()
                    .and_then(|model| s.snapshot.as_ref()?.properties.get(&model.id))
                {
                    if let Some(n) = props.context {
                        lines.push(Line::styled(
                            format!("Actual context: {n} tokens"),
                            p.heading,
                        ));
                    }
                    lines.push(Line::raw(match props.tools {
                        Some(true) => "Tools: supported",
                        Some(false) => "Tools: not supported",
                        None => "Tools: unknown",
                    }));
                    if !props.levels.is_empty() {
                        lines.push(Line::raw(format!(
                            "Reasoning: {}",
                            props
                                .levels
                                .iter()
                                .map(|t| t.label())
                                .collect::<Vec<_>>()
                                .join(" / ")
                        )));
                    }
                    use crate::models::profile::{Field, Profile};
                    let profile = Profile {
                        generation: props.generation.clone(),
                        ..Default::default()
                    };
                    lines.push(Line::styled("Actual server sampling", p.heading));
                    for field in [
                        Field::Temperature,
                        Field::TopP,
                        Field::TopK,
                        Field::MinP,
                        Field::RepeatPenalty,
                        Field::PresencePenalty,
                    ] {
                        let value = field.value(&profile);
                        if value != "auto" {
                            lines.push(Line::raw(format!(
                                "{}: {value}",
                                field.label().trim_start_matches("Generation / ")
                            )));
                        }
                    }
                }
                for device in crate::models::fit::parse_devices(&s.devices) {
                    lines.push(Line::raw(format!(
                        "{}: {:.1}/{:.1} GiB free (last probe)",
                        device.id,
                        device.free as f64 / 1073741824.0,
                        device.total as f64 / 1073741824.0
                    )));
                }
                lines.push(Line::raw(
                    "Agent window close releases agent-owned inference.",
                ));
            }
            _ => {
                title = "Preferences";
                if let Some(field) = crate::settings::Field::ALL.get(app.setting) {
                    lines.push(Line::styled(field.label(), p.heading));
                    lines.push(Line::raw(field.hint()));
                    lines.push(Line::raw(""));
                    lines.push(Line::raw("Double-click or Enter to edit."));
                    lines.push(Line::raw(""));
                }
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
