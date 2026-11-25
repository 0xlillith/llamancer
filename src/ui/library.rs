//! Responsive library table and inspector.
use crate::{
    app::App,
    models::library::{identity, quantization},
    ui::{safe, Palette},
};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table, Wrap},
    Frame,
};
pub(super) fn list_area(area: Rect) -> Rect {
    let [_, table] = Layout::vertical([Constraint::Length(4), Constraint::Min(1)]).areas(area);
    if area.width >= 100 {
        let [list, _] =
            Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)])
                .areas(table);
        list
    } else {
        table
    }
}
pub fn draw(frame: &mut Frame, area: Rect, app: &App, p: &Palette) {
    if let Some(id) = &app.hide_confirmation {
        frame.render_widget(Paragraph::new(format!("Hide this model from the library?\n{}\n\nFiles and profiles will be retained.\ny confirm | n/Esc cancel",safe(&id.to_string_lossy()))).wrap(Wrap{trim:false}).style(p.base).block(p.panel(" Confirm hide ")),area);
        return;
    }
    if app.show_invalid {
        let (offset, capacity) = super::viewport(app.invalid_cursor, area.height, 4);
        let mut lines = vec![format!(
            "Invalid GGUF: {} total, {} retained | ↑↓ / Home End | i/Esc back",
            app.invalid_total,
            app.invalid.len()
        )];
        lines.extend(
            app.invalid
                .iter()
                .enumerate()
                .skip(offset)
                .take(capacity)
                .map(|(i, (path, kind))| {
                    format!(
                        "{} {} | {}",
                        if i == app.invalid_cursor { ">" } else { " " },
                        safe(&path.to_string_lossy()),
                        kind
                    )
                }),
        );
        if let Some((path, kind)) = app.invalid.get(app.invalid_cursor) {
            lines.push(format!(
                "Selected: {} | {}",
                safe(&path.to_string_lossy()),
                kind
            ));
        }
        frame.render_widget(
            Paragraph::new(lines.join("\n"))
                .style(p.base)
                .block(p.panel(" Invalid files ")),
            area,
        );
        return;
    }
    let indices = app.visible();
    let [summary, table_area] =
        Layout::vertical([Constraint::Length(4), Constraint::Min(1)]).areas(area);
    use super::icons::Icon;
    let total_bytes = app
        .candidates
        .iter()
        .fold(0u64, |sum, g| sum.saturating_add(g.bytes()));
    let stats = Line::from(vec![
        Span::styled(format!(" {} MODELS ", p.icon(Icon::Library)), p.label),
        Span::styled(
            format!(" {}/{} ", indices.len(), app.candidates.len()),
            p.surface.add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(" STORAGE ", p.label),
        Span::styled(
            format!(" {:.2} GiB ", total_bytes as f64 / 1073741824.0),
            p.surface,
        ),
        Span::raw("  "),
        Span::styled(" ROOTS/IMPORTS ", p.label),
        Span::styled(
            format!(
                " {} ",
                app.config.model_roots.len() + app.config.model_imports.len()
            ),
            p.surface,
        ),
        Span::raw("  "),
        Span::styled(format!(" {} INVALID ", p.icon(Icon::Warning)), p.label),
        Span::styled(
            format!(" {} ", app.invalid_total),
            if app.invalid_total > 0 {
                p.surface.fg(p.orange)
            } else {
                p.surface
            },
        ),
    ]);
    let search = Line::from(vec![
        Span::styled(format!(" {} Search ", p.icon(Icon::Discover)), p.muted),
        Span::styled(
            if app.query.is_empty() && !app.searching {
                "[click to filter]".into()
            } else {
                format!(
                    "{}{}",
                    super::search_value(
                        &app.query,
                        app.searching,
                        usize::from(area.width.saturating_sub(45))
                    ),
                    if app.searching { "▏" } else { "" }
                )
            },
            if app.searching { p.selected } else { p.surface },
        ),
        Span::styled("   Sort ", p.muted),
        Span::styled(app.sort.label(), p.base),
        Span::styled("   Filter ", p.muted),
        Span::styled(
            if app.only_favorites {
                "Favorites"
            } else {
                "All"
            },
            p.base,
        ),
    ]);
    let stats = if area.width < 75 {
        Line::from(vec![
            Span::styled(
                format!(" {}/{} models ", indices.len(), app.candidates.len()),
                p.heading,
            ),
            Span::styled(
                format!(
                    " • {:.1} GiB • {} invalid",
                    total_bytes as f64 / 1073741824.0,
                    app.invalid_total
                ),
                p.base,
            ),
        ])
    } else {
        stats
    };
    frame.render_widget(
        Paragraph::new(vec![stats, search])
            .style(p.base)
            .block(p.panel(" Library ")),
        summary,
    );
    if app.details {
        draw_inspector(frame, table_area, app, p, true);
        return;
    }
    let (list, inspector) = if area.width >= 100 {
        let [a, b] = Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)])
            .areas(table_area);
        (a, Some(b))
    } else {
        (table_area, None)
    };
    if indices.is_empty() {
        let message = if app.candidates.is_empty() {
            "        /\\\n       /  \\\n      / *  \\\n     /______\\\n    /________\\\n     LLAMANCER\n\nNo models indexed.\n\n1. G  Guided setup (model folder and backend)\n2. 2  Discover and download a GGUF\n3. 1  Select a complete model; Enter twice to launch\n4. 4  Server: w opens Web UI; a launches agent"
        } else {
            "No matching models. Edit search with / or toggle favorites with v."
        };
        frame.render_widget(
            Paragraph::new(message)
                .style(p.base)
                .wrap(Wrap { trim: true })
                .block(p.panel(" Models ")),
            list,
        );
    } else {
        let cursor = app.model_cursor.min(indices.len().saturating_sub(1));
        let (offset, capacity) = super::viewport(cursor, list.height, 3);
        let rows = indices
            .iter()
            .enumerate()
            .skip(offset)
            .take(capacity)
            .map(|(i, &index)| {
                let group = &app.candidates[index];
                let m = &group.files[0].metadata;
                let fav = app.config.favorites.contains(&identity(group));
                let state = if group.projector {
                    "Projector"
                } else if !group.complete {
                    "Incomplete"
                } else {
                    "Indexed"
                };
                let style = if i == cursor {
                    p.selected.add_modifier(Modifier::BOLD)
                } else {
                    p.base
                };
                let name = format!(
                    "{}{} {}",
                    if i == cursor { ">" } else { " " },
                    if fav { "★" } else { " " },
                    safe(&group.name)
                );
                let cells = if list.width >= 55 {
                    vec![
                        Cell::from(name),
                        Cell::from(quantization(m.file_type)),
                        Cell::from(format!("{:.1}", group.bytes() as f64 / 1073741824.0)),
                        Cell::from(format!(
                            "{} {state}",
                            p.icon(if group.complete {
                                Icon::Check
                            } else {
                                Icon::Warning
                            })
                        ))
                        .style(Style::default().fg(if group.complete {
                            p.green
                        } else {
                            p.orange
                        })),
                    ]
                } else {
                    vec![Cell::from(name), Cell::from(quantization(m.file_type))]
                };
                Row::new(cells).style(style)
            });
        let widths = if list.width >= 55 {
            vec![
                Constraint::Min(12),
                Constraint::Length(11),
                Constraint::Length(6),
                Constraint::Length(12),
            ]
        } else {
            vec![Constraint::Min(6), Constraint::Length(9)]
        };
        let headings = if list.width >= 55 {
            vec!["Model", "Quant", "GiB", "Status"]
        } else {
            vec!["Model", "Quant"]
        };
        let table = Table::new(rows, widths)
            .header(Row::new(headings).style(p.heading))
            .column_spacing(1)
            .block(p.panel(" Models "));
        frame.render_widget(table, list);
    }
    if let Some(inspector) = inspector {
        draw_inspector(frame, inspector, app, p, false);
    }
}

fn inspector_widget(app: &App, p: &Palette, expanded: bool) -> Paragraph<'static> {
    use super::icons::Icon;
    let mut lines = Vec::new();
    if let Some(group) = app.selected() {
        let m = &group.files[0].metadata;
        let value = |v: &Option<String>| v.as_deref().map(safe).unwrap_or_else(|| "unknown".into());
        let n = |v: Option<u64>| v.map(|x| x.to_string()).unwrap_or_else(|| "unknown".into());
        let field = |label: &str, value: String| {
            Line::from(vec![
                Span::styled(format!("{label:<14}"), p.muted),
                Span::styled(value, p.base),
            ])
        };
        let section = |label: &str| Line::styled(format!("─ {label}"), p.heading);
        lines.push(Line::styled(
            safe(&group.name),
            p.heading.add_modifier(Modifier::BOLD),
        ));
        lines.push(Line::from(vec![
            Span::styled(
                format!(
                    " {} {} ",
                    p.icon(if group.complete {
                        Icon::Check
                    } else {
                        Icon::Warning
                    }),
                    if group.complete {
                        "COMPLETE"
                    } else {
                        "INCOMPLETE"
                    }
                ),
                if group.complete {
                    p.surface.fg(p.green)
                } else {
                    p.surface.fg(p.orange)
                },
            ),
            Span::raw(" "),
            Span::styled(format!(" {} ", quantization(m.file_type)), p.label),
            Span::raw(" "),
            Span::styled(
                format!(" {:.2} GiB ", group.bytes() as f64 / 1073741824.0),
                p.surface,
            ),
        ]));
        lines.push(Line::raw(""));
        lines.push(section("MODEL"));
        lines.push(field("Name", value(&m.name)));
        lines.push(field("Architecture", value(&m.architecture)));
        lines.push(field("Context", n(m.context)));
        lines.push(field("Layers", n(m.layers)));
        lines.push(field("Tokenizer", value(&m.tokenizer)));
        lines.push(field("Vocabulary", n(m.vocabulary)));
        lines.push(field("Elements", group.elements().to_string()));
        lines.push(field(
            "Shards",
            format!("{}/{}", group.files.len(), group.expected),
        ));
        lines.push(Line::raw(""));
        lines.push(section("INFERENCE"));
        let profile = app
            .config
            .profiles
            .get(&identity(group))
            .cloned()
            .unwrap_or_default();
        lines.push(field(
            "GPU layers",
            profile
                .engine
                .gpu_layers
                .map(|x| x.to_string())
                .unwrap_or_else(|| "auto".into()),
        ));
        if let Some(e) = crate::models::estimate::estimate(group, &profile) {
            lines.push(field(
                "Weights",
                format!("~{:.2} GiB", e.weights as f64 / 1073741824.0),
            ));
            lines.push(field(
                "KV cache",
                format!(
                    "~{:.2} GiB / {} tokens",
                    e.kv as f64 / 1073741824.0,
                    e.context
                ),
            ));
        } else {
            lines.push(Line::raw("KV estimate unavailable."));
        }
        lines.push(Line::styled(
            "Estimate only; excludes runtime overhead.",
            p.dim,
        ));
        lines.push(Line::raw(""));
        lines.push(section("VALIDATION"));
        lines.push(Line::styled(
            if group
                .files
                .iter()
                .all(|f| f.metadata.payload_bounds_checked)
            {
                "Known tensor extents verified; loadability untested."
            } else {
                "Metadata read; some tensor extents unverified."
            },
            p.dim,
        ));
        if expanded {
            lines.push(Line::raw(""));
            lines.push(section("FILES"));
            lines.extend(
                group
                    .files
                    .iter()
                    .map(|f| Line::raw(safe(&f.path.to_string_lossy()))),
            );
        } else {
            lines.push(Line::raw(""));
            lines.push(Line::styled(
                "Enter launch · j inspect · p profile",
                p.heading,
            ));
        }
    } else {
        lines.push(Line::raw("Select a model to inspect metadata."));
    }
    Paragraph::new(lines)
        .style(p.base)
        .wrap(Wrap { trim: false })
        .block(p.panel(format!(
            " {} {} ",
            p.icon(Icon::Library),
            if expanded {
                "Model inspector"
            } else {
                "Inspector"
            }
        )))
}
pub(super) fn scroll_limit(area: Rect, app: &App, p: &Palette) -> u16 {
    inspector_widget(app, p, true)
        .line_count(area.width)
        .saturating_sub(usize::from(area.height))
        .min(u16::MAX as usize) as u16
}
fn draw_inspector(frame: &mut Frame, area: Rect, app: &App, p: &Palette, expanded: bool) {
    let widget = inspector_widget(app, p, expanded);
    let scroll = if expanded {
        app.inspector_scroll.min(
            widget
                .line_count(area.width)
                .saturating_sub(usize::from(area.height))
                .min(u16::MAX as usize) as u16,
        )
    } else {
        0
    };
    frame.render_widget(widget.scroll((scroll, 0)), area);
}
