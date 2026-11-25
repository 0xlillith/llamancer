//! Shared visible button geometry for ordinary views.
use super::{content_regions, regions, Palette};
use crate::app::App;
use crossterm::event::KeyCode;
use ratatui::{layout::Rect, style::Modifier, text::Span, widgets::Paragraph, Frame};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Key(KeyCode),
    SubmitSearch,
}
#[derive(Clone, Debug)]
struct Button {
    rect: Rect,
    label: String,
    command: Command,
    primary: bool,
}
fn visible(app: &App) -> bool {
    !app.help
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
        && !(app.tab == 3 && app.server.show_logs)
}
fn buttons(area: Rect, app: &App) -> Vec<Button> {
    use Command::{Key, SubmitSearch};
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
                ("Favorites".into(), Key(Char('v'))),
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
                (
                    format!("Sort: {}", app.discovery.variant_sort.label()),
                    Key(Char('s')),
                ),
                ("Back".into(), Key(Backspace)),
            ],
        ),
        1 => (
            3,
            vec![
                ("Search".into(), SubmitSearch),
                ("Open".into(), Key(Enter)),
                (
                    format!("Sort: {}", app.discovery.ranking.label()),
                    Key(Char('s')),
                ),
                ("Next page".into(), Key(Char('n'))),
            ],
        ),
        2 if app.downloads.is_some() => (
            2,
            vec![
                ("Resume model".into(), Key(Enter)),
                ("Pause".into(), Key(Char('x'))),
                ("Run queue".into(), Key(Char('R'))),
                ("Discover".into(), Key(Char('2'))),
            ],
        ),
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
                    v.push(("Unload model".into(), Key(Char('u'))));
                }
                v.extend([
                    ("Open agent".into(), Key(Enter)),
                    ("Web UI".into(), Key(Char('w'))),
                    ("Library".into(), Key(Char('1'))),
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
        _ => return vec![],
    };
    let y = body.y.saturating_add(dy);
    if body.width < 3 || y >= body.bottom().saturating_sub(1) {
        return vec![];
    }
    let mut x = body.x + 1;
    let end = body.right().saturating_sub(1);
    let mut out = vec![];
    for (i, (label, command)) in items.into_iter().enumerate() {
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
            primary: i == 0,
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
