//! Shared visible button geometry for ordinary views.
use super::{content_regions, regions, Palette};
use crate::app::App;
use crossterm::event::KeyCode;
use ratatui::{layout::Rect, style::Modifier, text::Span, widgets::Paragraph, Frame};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Key(KeyCode),
    SubmitSearch,
    Pick(super::choices::Kind),
    SelectSetting(usize),
}
#[derive(Clone, Debug)]
struct Button {
    rect: Rect,
    label: String,
    command: Command,
    primary: bool,
}
fn visible(app: &App) -> bool {
    app.picker.is_none()
        && !app.help
        && !app.backend_open
        && !app.setup_open
        && !app.benchmark_open
        && app.projects.is_none()
        && app.download_destination.is_none()
        && app.fit_report.is_none()
        && app.edit.is_none()
        && app.profile_editor.is_none()
        && app.launch_menu.is_none()
        && !app.launching_agent
        && app.quarantine_confirmation.is_none()
        && app.hide_confirmation.is_none()
        && !app.show_invalid
}
fn buttons(area: Rect, app: &App) -> Vec<Button> {
    use Command::{Key, Pick, SubmitSearch};
    use KeyCode::{Backspace, Char, Enter};
    if !visible(app) {
        return vec![];
    }
    let (body, _) = content_regions(regions(area)[2], app);
    let (dy, items): (u16, Vec<(String, Command)>) = match app.tab {
        0 if app.selected().is_some() => (
            1,
            vec![
                (
                    if app.details {
                        "Back to models".into()
                    } else {
                        "Run model".into()
                    },
                    Key(Enter),
                ),
                ("Inspect".into(), Key(Char('j'))),
                ("Profile".into(), Key(Char('p'))),
                ("Refresh".into(), Key(Char('r'))),
                (format!("Sort: {}", app.sort.label()), Key(Char('s'))),
                (
                    if app.only_favorites {
                        "Show all".into()
                    } else {
                        "Favorites".into()
                    },
                    Key(Char('v')),
                ),
                ("Star model".into(), Key(Char('f'))),
            ],
        ),
        0 => (
            1,
            vec![
                ("Discover".into(), Key(Char('2'))),
                ("Setup".into(), Key(Char('G'))),
                ("Refresh".into(), Key(Char('r'))),
            ],
        ),
        1 if app.discovery.busy() => return vec![],
        1 if app.discovery.repo.is_some() => (
            3,
            vec![
                ("Download model".into(), Key(Enter)),
                ("Filter".into(), Pick(super::choices::Kind::Filters)),
                (
                    format!("Sort: {}", app.discovery.variant_sort.label()),
                    Pick(super::choices::Kind::Sort),
                ),
                ("Back".into(), Key(Backspace)),
            ],
        ),
        1 => (
            3,
            vec![
                ("Search".into(), SubmitSearch),
                ("Open".into(), Key(Enter)),
                ("Filters".into(), Pick(super::choices::Kind::Filters)),
                (
                    format!("Sort: {}", app.discovery.ranking.label()),
                    Pick(super::choices::Kind::Sort),
                ),
                ("Next page".into(), Key(Char('n'))),
            ],
        ),
        2 if app.downloads.is_some() => {
            let d = app.downloads.as_ref().unwrap();
            let groups = d.bundles();
            let selected = groups.get(d.bundle_cursor(&groups));
            let mut actions = Vec::new();
            if d.busy() {
                actions.push(("Pause".into(), Key(Char('x'))));
            } else if selected.is_some_and(|g| g.state == "Failed") {
                actions.push(("Retry / verify".into(), Key(Enter)));
                actions.push(("Retry fresh".into(), Key(Char('F'))));
            } else if selected.is_some_and(|g| matches!(g.state, "Queued" | "Paused")) {
                actions.push(("Resume model".into(), Key(Enter)));
            } else if selected.is_some_and(|g| g.state == "Complete") {
                actions.push(("Open Library".into(), Key(Char('1'))));
            } else if selected.is_some_and(|g| g.state == "Quarantined") {
                actions.push(("Restore model".into(), Key(Char('T'))));
            }
            actions.push(("Discover".into(), Key(Char('2'))));
            if !d.busy()
                && d.queue
                    .items
                    .iter()
                    .any(|i| i.state == crate::downloads::State::Queued)
            {
                actions.push(("Run queue".into(), Key(Char('R'))));
            }
            if !d.busy()
                && selected.is_some_and(|g| {
                    g.indices
                        .iter()
                        .all(|&i| d.queue.items[i].receipt.is_none())
                })
            {
                actions.push(("Remove job".into(), Key(Char('z'))));
            }
            (2, actions)
        }
        3 if app.server.show_logs => (3, vec![("Back to server".into(), Key(Char('g')))]),
        3 => {
            let s = &app.server;
            let active = s.owns_server() || s.snapshot.is_some();
            if active {
                let mut v = vec![(
                    if s.owns_server() {
                        "Stop server".into()
                    } else {
                        "Disconnect".into()
                    },
                    Key(Char(if s.owns_server() { 'x' } else { 'D' })),
                )];
                if s.snapshot.as_ref().is_some_and(|x| x.router) {
                    let loaded = s
                        .selected()
                        .is_some_and(|m| matches!(m.state(), "loaded" | "sleeping"));
                    v.push((
                        if loaded {
                            "Unload model".into()
                        } else {
                            "Load model".into()
                        },
                        Key(Char(if loaded { 'u' } else { 'l' })),
                    ));
                }
                v.extend([
                    ("Open agent".into(), Key(Enter)),
                    ("Web UI".into(), Key(Char('w'))),
                    ("Project".into(), Key(Char('P'))),
                    ("Logs".into(), Key(Char('g'))),
                ]);
                (3, v)
            } else {
                (
                    3,
                    vec![
                        ("Connect".into(), Key(Char('r'))),
                        ("Start router".into(), Key(Char('s'))),
                        ("Backends".into(), Key(Char('B'))),
                        ("Setup".into(), Key(Char('G'))),
                    ],
                )
            }
        }
        4 if body.width < 66 => (
            1,
            vec![
                (
                    format!(
                        "Category: {}",
                        crate::settings::Field::ALL[app.setting].category()
                    ),
                    Pick(super::choices::Kind::Category),
                ),
                ("Edit".into(), Key(Enter)),
            ],
        ),
        4 => (
            1,
            ["Storage", "Inference", "Agent", "Workspace", "Appearance"]
                .iter()
                .map(|category| {
                    let i = crate::settings::Field::ALL
                        .iter()
                        .position(|f| f.category() == *category)
                        .unwrap_or(0);
                    ((*category).to_string(), Command::SelectSetting(i))
                })
                .collect(),
        ),
        _ => return vec![],
    };
    let y = body.y.saturating_add(dy);
    if body.width < 3 || y >= body.bottom().saturating_sub(1) {
        return vec![];
    }
    let mut x = body.x + 1;
    let end = body.right().saturating_sub(1);
    let mut out = vec![];
    for (i, (mut label, command)) in items.into_iter().enumerate() {
        if body.width < 75 {
            label = match label.as_str() {
                "Download model" => "Download".into(),
                "Resume model" => "Resume".into(),
                "Start router" => "Router".into(),
                "Stop server" => "Stop".into(),
                "Disconnect" => "Detach".into(),
                "Open agent" => "Agent".into(),
                "Unload model" => "Unload".into(),
                "Load model" => "Load".into(),
                "Back to models" => "Back".into(),
                "Next page" => "Next".into(),
                _ if label.starts_with("Sort:") => "Sort".into(),
                _ => label,
            };
        }
        let width = Span::raw(&label)
            .width()
            .saturating_add(4)
            .min(u16::MAX as usize) as u16;
        if x.saturating_add(width) > end {
            break;
        }
        out.push(Button {
            rect: Rect::new(x, y, width, 1),
            label,
            command,
            primary: if let Command::SelectSetting(n) = command {
                crate::settings::Field::ALL[n].category()
                    == crate::settings::Field::ALL[app.setting].category()
            } else {
                i == 0
            },
        });
        x = x.saturating_add(width + 1);
    }
    out
}
pub fn hit(area: Rect, app: &App, x: u16, y: u16) -> Option<Command> {
    buttons(area, app)
        .iter()
        .find(|b| b.rect.contains(ratatui::layout::Position::new(x, y)))
        .map(|b| b.command)
}
pub(super) fn draw(f: &mut Frame, app: &App, p: &Palette) {
    let buttons = buttons(f.area(), app);
    let Some(first) = buttons.first() else {
        return;
    };
    let (body, _) = content_regions(regions(f.area())[2], app);
    let row = Rect::new(body.x + 1, first.rect.y, body.width.saturating_sub(2), 1);
    f.render_widget(
        Paragraph::new(" ".repeat(row.width as usize)).style(p.base),
        row,
    );
    for b in buttons {
        let style = if b.primary {
            p.brand
        } else {
            p.surface.add_modifier(Modifier::BOLD)
        };
        f.render_widget(
            Paragraph::new(format!("  {}  ", b.label)).style(style),
            b.rect,
        );
    }
}
