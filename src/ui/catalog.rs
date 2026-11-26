//! Catalog and transfer tables. Row geometry is shared with ui::row_at.
use super::{chrome, safe, viewport, Palette};
use crate::app::App;
use ratatui::{
    layout::{Constraint, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table},
    Frame,
};

pub const DISCOVER_HEADER: u16 = 5;
pub const DISCOVER_RESERVED: u16 = 6;
pub const DOWNLOAD_HEADER: u16 = 4;
pub const DOWNLOAD_RESERVED: u16 = 7;

pub fn ordinary(app: &App) -> bool {
    !app.help
        && !app.backend_open
        && !app.setup_open
        && !app.benchmark_open
        && app.projects.is_none()
        && app.download_destination.is_none()
        && app.fit_report.is_none()
        && app.edit.is_none()
        && app.profile_editor.is_none()
        && app.quarantine_confirmation.is_none()
        && app.hide_confirmation.is_none()
        && !app.launching_agent
        && app.probe_warning.is_none()
}
fn interior(area: Rect, y: u16, height: u16) -> Rect {
    Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(y),
        area.width.saturating_sub(2),
        height.min(area.height.saturating_sub(y + 1)),
    )
}
fn state_style(selected: bool, p: &Palette) -> ratatui::style::Style {
    if selected {
        p.selected.add_modifier(Modifier::BOLD)
    } else {
        p.base
    }
}
fn abbreviated(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.)
    } else if n >= 1000 {
        format!("{:.1}K", n as f64 / 1000.)
    } else {
        n.to_string()
    }
}
pub fn discover(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let d = &app.discovery;
    f.render_widget(p.panel(" Discover ").style(p.base), area);
    let query = if d.repo.is_some() {
        &d.variant_filter
    } else {
        &d.query
    };
    let search = format!(
        " {}  [{}{}] ",
        if d.repo.is_some() {
            "Filter variants"
        } else {
            "Search models"
        },
        super::search_value(query, d.editing, usize::from(area.width.saturating_sub(24))),
        if d.editing { "▏" } else { "" }
    );
    f.render_widget(
        Paragraph::new(search).style(if d.editing { p.selected } else { p.surface }),
        interior(area, 1, 1),
    );
    let status = if let Some(repo) = &d.repo {
        format!(
            "{}  •  {} variants  •  {}",
            safe(repo),
            d.variants.len(),
            d.variant_sort.label()
        )
    } else {
        format!(
            "{} results  •  {}  •  {}",
            d.repos.len(),
            d.ranking.label(),
            safe(&d.status)
        )
    };
    f.render_widget(
        Paragraph::new(status).style(p.heading),
        interior(area, 2, 1),
    );
    if area.height < 7 || area.width < 12 {
        return;
    }
    let (offset, cap) = viewport(d.cursor, area.height, DISCOVER_RESERVED);
    let table_area = interior(area, DISCOVER_HEADER - 1, cap as u16 + 1);
    let wide = table_area.width >= 56;
    if d.repo.is_some() {
        let rows = d
            .variants
            .iter()
            .enumerate()
            .skip(offset)
            .take(cap)
            .map(|(i, v)| {
                let mut cells = vec![Cell::from(format!(
                    "{} {}",
                    if i == d.cursor { "›" } else { " " },
                    safe(&v.name)
                ))];
                if wide {
                    cells.extend([
                        Cell::from(format!("{:.2}", v.bytes() as f64 / 1073741824.)),
                        Cell::from(format!("{}/{}", v.files.len(), v.expected)),
                        Cell::from(if v.draft() {
                            "MTP draft"
                        } else if !v.complete() {
                            "Incomplete"
                        } else if v.projector() {
                            "Projector"
                        } else {
                            "Ready"
                        })
                        .style(p.base.fg(if v.downloadable() {
                            p.green
                        } else {
                            p.orange
                        })),
                    ]);
                } else {
                    cells.push(Cell::from(format!(
                        "{:.1} GiB",
                        v.bytes() as f64 / 1073741824.
                    )));
                }
                Row::new(cells).style(state_style(i == d.cursor, p))
            });
        let (widths, head) = if wide {
            (
                vec![
                    Constraint::Min(12),
                    Constraint::Length(9),
                    Constraint::Length(7),
                    Constraint::Length(12),
                ],
                vec!["Variant / quantization", "GiB", "Parts", "Status"],
            )
        } else {
            (
                vec![Constraint::Min(8), Constraint::Length(10)],
                vec!["Variant", "Size"],
            )
        };
        f.render_widget(
            Table::new(rows, widths)
                .header(Row::new(head).style(p.label.add_modifier(Modifier::BOLD)))
                .column_spacing(1),
            table_area,
        );
    } else {
        let rows = d
            .repos
            .iter()
            .enumerate()
            .skip(offset)
            .take(cap)
            .map(|(i, r)| {
                let gated = !(r.gated == serde_json::Value::Bool(false) || r.gated.is_null());
                let mut cells = vec![
                    Cell::from(format!(
                        "{} {}",
                        if i == d.cursor { "›" } else { " " },
                        safe(&r.id)
                    )),
                    Cell::from(abbreviated(r.downloads)),
                ];
                if wide {
                    cells.extend([
                        Cell::from(abbreviated(r.likes)),
                        Cell::from(if gated { "Gated" } else { "Public" })
                            .style(p.base.fg(if gated { p.orange } else { p.green })),
                    ]);
                }
                Row::new(cells).style(state_style(i == d.cursor, p))
            });
        let (widths, head) = if wide {
            (
                vec![
                    Constraint::Min(12),
                    Constraint::Length(11),
                    Constraint::Length(8),
                    Constraint::Length(10),
                ],
                vec!["Repository", "Downloads", "Likes", "Access"],
            )
        } else {
            (
                vec![Constraint::Min(8), Constraint::Length(8)],
                vec!["Repository", "DL"],
            )
        };
        f.render_widget(
            Table::new(rows, widths)
                .header(Row::new(head).style(p.label.add_modifier(Modifier::BOLD)))
                .column_spacing(1),
            table_area,
        );
    }
    if d.is_empty() {
        let message = if d.busy() {
            "Searching…"
        } else if d.repo.is_some() {
            "No matching variants"
        } else {
            "Type a model name, or choose Search to browse."
        };
        f.render_widget(
            Paragraph::new(message).style(p.dim),
            interior(area, DISCOVER_HEADER, 1),
        );
    }
}
pub fn downloads(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    f.render_widget(p.panel(" Downloads ").style(p.base), area);
    let Some(d) = &app.downloads else { return };
    let groups = d.bundles();
    let cursor = d.bundle_cursor(&groups);
    f.render_widget(
        Paragraph::new(format!(
            "{} models  •  {}  •  {}",
            groups.len(),
            d.transport.label(),
            safe(&d.status)
        ))
        .style(p.heading),
        interior(area, 1, 1),
    );
    if area.height < 8 || area.width < 12 {
        return;
    }
    let (offset, cap) = viewport(cursor, area.height, DOWNLOAD_RESERVED);
    let table_area = interior(area, DOWNLOAD_HEADER - 1, cap as u16 + 1);
    let wide = table_area.width >= 68;
    let rows = groups
        .iter()
        .enumerate()
        .skip(offset)
        .take(cap)
        .map(|(i, g)| {
            let color = match g.state {
                "Complete" => p.green,
                "Failed" | "Integrity failed" => p.orange,
                _ => p.heading.fg.unwrap_or(p.green),
            };
            let pct = (g.received as f64 / g.bytes.max(1) as f64 * 100.).clamp(0., 100.);
            let mut cells = vec![
                Cell::from(format!(
                    "{} {}",
                    if i == cursor { "›" } else { " " },
                    safe(&g.name)
                )),
                Cell::from(g.state).style(p.base.fg(color)),
            ];
            if wide {
                cells.push(Cell::from(Line::from(vec![
                    Span::styled(chrome::meter(g.received, g.bytes, 10), p.base.fg(color)),
                    Span::raw(format!(" {pct:>5.1}%")),
                ])));
                cells.push(Cell::from(chrome::transfer_size(g.received, g.bytes)));
            } else {
                cells.push(Cell::from(format!("{pct:.0}%")));
            }
            Row::new(cells).style(state_style(i == cursor, p))
        });
    let (widths, head) = if wide {
        (
            vec![
                Constraint::Min(12),
                Constraint::Length(17),
                Constraint::Length(18),
                Constraint::Length(20),
            ],
            vec!["Model", "Status", "Progress", "Transferred"],
        )
    } else {
        (
            vec![
                Constraint::Min(7),
                Constraint::Length(11),
                Constraint::Length(6),
            ],
            vec!["Model", "Status", "%"],
        )
    };
    f.render_widget(
        Table::new(rows, widths)
            .header(Row::new(head).style(p.label.add_modifier(Modifier::BOLD)))
            .column_spacing(1),
        table_area,
    );
    let summary = if let Some(g) = groups.get(cursor) {
        let speed = g
            .active
            .and_then(|i| d.speed_eta(i))
            .map(|(s, _)| format!("  •  {:.1} MiB/s", s / 1048576.))
            .unwrap_or_default();
        let detail = format!("{}  •  {}/{} parts{speed}", g.state, g.complete, g.expected);
        let sub = g
            .indices
            .iter()
            .find_map(|&i| {
                d.queue.items[i]
                    .integrity_error
                    .as_deref()
                    .or(d.queue.items[i].error.as_deref())
            })
            .map(safe)
            .or_else(|| {
                g.active
                    .and_then(|i| d.speed_history(i))
                    .map(|(graph, _)| format!("Speed  {graph}"))
            })
            .unwrap_or_default();
        vec![Line::styled(detail, p.heading), Line::styled(sub, p.dim)]
    } else {
        vec![Line::styled(
            "No downloads. Open Discover to choose a model.",
            p.dim,
        )]
    };
    f.render_widget(
        Paragraph::new(summary).style(p.base),
        interior(area, area.height.saturating_sub(3), 2),
    );
}
