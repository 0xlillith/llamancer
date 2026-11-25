//! Responsive library table and inspector.
use crate::{
    app::App,
    models::{
        group::Group,
        library::{identity, quantization},
    },
    ui::{safe, Palette},
};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, BorderType, Cell, Paragraph, Row, Table, Wrap},
    Frame,
};
pub fn details(group: &Group, config: &crate::config::Config) -> String {
    let m = &group.files[0].metadata;
    let text = |v: &Option<String>| v.as_deref().map(safe).unwrap_or_else(|| "Unknown".into());
    let num = |v: Option<u64>| v.map(|n| n.to_string()).unwrap_or_else(|| "Unknown".into());
    let mut lines = vec![
        safe(&group.name),
        String::new(),
        format!("Name          {}", text(&m.name)),
        format!("Architecture  {}", text(&m.architecture)),
        format!("Quantization  {}", quantization(m.file_type)),
        format!("Context       {}", num(m.context)),
        format!("Layers        {}", num(m.layers)),
        format!("Tokenizer     {}", text(&m.tokenizer)),
        format!("Vocabulary    {}", num(m.vocabulary)),
        format!("Elements      {}", group.elements()),
        format!(
            "Disk size     {:.2} GiB",
            group.bytes() as f64 / 1073741824.0
        ),
        format!("Shards        {}/{}", group.files.len(), group.expected),
        if group.complete {
            "Expected shards present".into()
        } else {
            "Incomplete or inconsistent shards".into()
        },
        String::new(),
        if group
            .files
            .iter()
            .all(|f| f.metadata.payload_bounds_checked)
        {
            "Known tensor extents verified; loadability untested.".into()
        } else {
            "Metadata read; some tensor extents unverified.".into()
        },
        "p profile | l load | Enter / Esc close".into(),
        String::new(),
    ];
    let profile = config
        .profiles
        .get(&identity(group))
        .cloned()
        .unwrap_or_default();
    if let Some(estimate) = crate::models::estimate::estimate(group, &profile) {
        lines.push(format!(
            "Weights ~{:.2} GiB + KV ~{:.2} GiB at {} tokens",
            estimate.weights as f64 / 1073741824.0,
            estimate.kv as f64 / 1073741824.0,
            estimate.context
        ));
        lines.push("Approximate (auto cache assumes F16); excludes compute buffers, projector and overhead.".into());
    } else {
        lines.push("KV estimate unavailable for this architecture/metadata.".into());
    }
    lines.extend(group.files.iter().map(|f| safe(&f.path.to_string_lossy())));
    lines.join("\n")
}
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
    let summary_text = format!(
        "{} / {} models  ·  {} roots/imports  ·  {} invalid  ·  {}\nSearch: {}{}   Sort: {}   {}",
        indices.len(),
        app.candidates.len(),
        app.config.model_roots.len() + app.config.model_imports.len(),
        app.invalid_total,
        if app.scanning { "Scanning" } else { "Idle" },
        safe(&app.query),
        if app.searching { "▏" } else { "" },
        app.sort.label(),
        if app.only_favorites {
            "Favorites"
        } else {
            "All"
        }
    );
    frame.render_widget(
        Paragraph::new(summary_text).style(p.base).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(p.muted)
                .title(" Library "),
        ),
        summary,
    );
    if app.details && area.width < 100 {
        let text = app
            .selected()
            .map(|g| details(g, &app.config))
            .unwrap_or_else(|| "No model selected".into());
        frame.render_widget(
            Paragraph::new(text)
                .wrap(Wrap { trim: false })
                .style(p.base)
                .block(p.panel(" Model inspector ")),
            table_area,
        );
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
            "No models indexed.\n\nAdd a model root in Settings (5), then press r to scan.\nDiscover (2) can queue Hugging Face GGUF files."
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
                        Cell::from(state).style(Style::default().fg(if group.complete {
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
                Constraint::Length(10),
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
        let text = app
            .selected()
            .map(|g| details(g, &app.config))
            .unwrap_or_else(|| "Select a model to inspect metadata.".into());
        frame.render_widget(
            Paragraph::new(text)
                .style(p.base)
                .wrap(Wrap { trim: false })
                .block(p.panel(" Inspector ")),
            inspector,
        );
    }
}
