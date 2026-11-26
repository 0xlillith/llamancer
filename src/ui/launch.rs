//! Launch controls and their hit regions are calculated together.
use super::{views, Palette};
use crate::app::App;
use crossterm::event::KeyCode;
use ratatui::{
    layout::{Position, Rect},
    style::Modifier,
    widgets::{Paragraph, Wrap},
    Frame,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub index: Option<usize>,
    pub key: KeyCode,
}
struct Layout {
    main: Rect,
    aside: Option<Rect>,
    rows: Vec<(usize, Rect)>,
    editor: Option<Rect>,
    help: Option<Rect>,
}
fn layout(area: Rect, app: &App) -> Layout {
    let (main, aside) = if area.width >= 116 && area.height >= 15 {
        let w = area.width.saturating_sub(39);
        (
            Rect::new(area.x, area.y, w, area.height),
            Some(Rect::new(area.x + w, area.y, 39, area.height)),
        )
    } else {
        (area, None)
    };
    let inner = Rect::new(
        main.x.saturating_add(1),
        main.y.saturating_add(1),
        main.width.saturating_sub(2),
        main.height.saturating_sub(2),
    );
    let header = if inner.height >= 12 {
        3
    } else if inner.height >= 5 {
        1
    } else {
        0
    };
    let tail = if inner.height >= 3 { 1 } else { 0 };
    let order = app.launch_order();
    let capacity = usize::from(inner.height.saturating_sub(header + tail)).min(order.len());
    let cursor = order
        .iter()
        .position(|&i| i == app.launch_cursor)
        .unwrap_or(0);
    let offset = if capacity == 0 {
        0
    } else {
        cursor
            .saturating_sub(capacity - 1)
            .min(order.len() - capacity)
    };
    let rows = order
        .iter()
        .copied()
        .skip(offset)
        .take(capacity)
        .enumerate()
        .map(|(n, i)| {
            (
                i,
                Rect::new(inner.x, inner.y + header + n as u16, inner.width, 1),
            )
        })
        .collect();
    let end = inner.bottom().saturating_sub(1);
    let editor = if app.context_input.is_some() && tail > 0 {
        Some(Rect::new(inner.x, end, inner.width, 1))
    } else {
        None
    };
    let help = if tail > 0 && editor.is_none() {
        Some(Rect::new(inner.x, end, inner.width, 1))
    } else {
        None
    };
    Layout {
        main,
        aside,
        rows,
        editor,
        help,
    }
}
fn stepper(rect: Rect, index: usize) -> Option<(Rect, Rect)> {
    if !(3..=5).contains(&index) || rect.width < 32 {
        return None;
    }
    let end = rect.right();
    Some((
        Rect::new(end - 9, rect.y, 4, 1),
        Rect::new(end - 4, rect.y, 4, 1),
    ))
}
pub fn hit(area: Rect, app: &App, x: u16, y: u16) -> Option<Hit> {
    app.launch_menu.as_ref()?;
    if app.context_input.is_some() {
        return None;
    }
    let pos = Position::new(x, y);
    let l = layout(area, app);
    for (i, r) in l.rows {
        if !r.contains(pos) {
            continue;
        }
        let key = if let Some((left, right)) = stepper(r, i) {
            if left.contains(pos) {
                KeyCode::Left
            } else if right.contains(pos) {
                KeyCode::Right
            } else {
                if (3..=4).contains(&i) && x >= r.right().saturating_sub(20) && r.width >= 54 {
                    KeyCode::Char('C')
                } else {
                    KeyCode::Enter
                }
            }
        } else {
            KeyCode::Enter
        };
        return Some(Hit {
            index: Some(i),
            key,
        });
    }
    if l.help.is_some_and(|r| r.contains(pos)) && x < l.main.x + 9 {
        return Some(Hit {
            index: None,
            key: KeyCode::Esc,
        });
    }
    None
}
pub fn draw(f: &mut Frame, area: Rect, app: &App, p: &Palette) {
    let Some(g) = app.launch_menu.as_ref() else {
        return;
    };
    let l = layout(area, app);
    f.render_widget(p.panel(" Launch model ").style(p.base), l.main);
    let (_, raw) = views::content(app, l.main);
    let lines: Vec<_> = raw.lines().collect();
    let first = l.rows.first().map(|(_, r)| r.y).unwrap_or(l.main.y + 1);
    let start = l.main.y.saturating_add(1);
    let width = l.main.width.saturating_sub(2);
    if first > start && width > 0 {
        let header = Rect::new(l.main.x + 1, start, width, first - start);
        let title = format!(
            "{}\n{} · {} GiB · {}",
            super::safe(&g.name),
            super::safe(&app.config.default_agent),
            (g.bytes() as f64 / 1073741824.0 * 10.).round() / 10.,
            if g.complete && !g.projector {
                if app.config.effective_profile(g).backend_defaults {
                    "Backend defaults"
                } else if crate::models::profile::recommendation(g).is_some() {
                    "Recommended defaults"
                } else {
                    "Ready to launch"
                }
            } else {
                "Inspection only"
            }
        );
        f.render_widget(Paragraph::new(title).style(p.heading), header);
    }
    for (index, r) in &l.rows {
        let text = lines.get(3 + *index).copied().unwrap_or_default();
        let label = if *index == 8 {
            if app.launch_advanced {
                "▾ Advanced settings"
            } else {
                "▸ Advanced settings"
            }
        } else {
            text.get(2..).unwrap_or(text)
        };
        let label = label
            .split("  ←/→")
            .next()
            .unwrap_or(label)
            .split(" (Enter")
            .next()
            .unwrap_or(label);
        let selected = *index == app.launch_cursor;
        let style = if selected {
            p.selected.add_modifier(Modifier::BOLD)
        } else if *index == 0 {
            p.brand
        } else if *index < 3 || *index == 7 || *index == 8 {
            p.surface
        } else {
            p.base
        };
        let mut text_area = *r;
        if stepper(*r, *index).is_some() {
            text_area.width =
                text_area
                    .width
                    .saturating_sub(if (3..=4).contains(index) && r.width >= 54 {
                        20
                    } else {
                        10
                    });
        }
        let marker = if selected { "›" } else { " " };
        f.render_widget(
            Paragraph::new(format!("{marker} {label}")).style(style),
            text_area,
        );
        if let Some((prev, next)) = stepper(*r, *index) {
            if (3..=4).contains(index) && r.width >= 54 {
                f.render_widget(
                    Paragraph::new(" Presets ").centered().style(p.label),
                    Rect::new(r.right() - 20, r.y, 10, 1),
                );
            }
            f.render_widget(Paragraph::new(" ‹ ").centered().style(p.label), prev);
            f.render_widget(Paragraph::new(" › ").centered().style(p.label), next);
        }
    }
    if let Some(r) = l.editor {
        let value = app.context_input.as_deref().unwrap_or("");
        f.render_widget(
            Paragraph::new(format!(
                "Tokens or auto: {}▏  Enter save / Esc cancel",
                super::safe(value)
            ))
            .style(p.selected),
            r,
        );
    }
    if let Some(r) = l.help {
        f.render_widget(
            Paragraph::new(" [Back]  ↑↓ select · Enter action · C presets").style(p.dim),
            r,
        );
    }
    let details = lines
        .iter()
        .skip(12)
        .filter(|line| !line.starts_with("Enter tokens"))
        .copied()
        .collect::<Vec<_>>()
        .join("\n\n");
    if let Some(side) = l.aside {
        let profile = app.config.effective_profile(g);
        let source = if profile.backend_defaults {
            "Backend defaults + custom overrides"
        } else {
            profile
                .recommendation_source(g, app.config.default_agent == "omp")
                .map(|r| r.publisher())
                .unwrap_or("Backend defaults")
        };
        use crate::models::profile::Field;
        let sampler = [
            Field::Temperature,
            Field::TopP,
            Field::TopK,
            Field::MinP,
            Field::RepeatPenalty,
            Field::PresencePenalty,
        ]
        .map(|field| {
            format!(
                "{}: {}",
                field.label().trim_start_matches("Generation / "),
                field.value(&profile)
            )
        })
        .join("\n");
        let context = profile.engine.context_size.map(u64::from);
        let max = g.files.first().and_then(|f| f.metadata.context);
        let budget = if let (Some(n), Some(t)) = (context, max) {
            format!("{}  {n}/{t} tokens", super::chrome::meter(n, t, 18))
        } else {
            "Auto context (backend decides)".into()
        };
        let text = format!("EFFECTIVE PROFILE\n{source}\n\nCONTEXT\n{budget}\n\nSAMPLER\n{sampler}\n\nReasoning: {}\nPreflight: {}\n\n{details}", profile.agent_thinking.map(|t|t.label()).unwrap_or("inherit"), if profile.tool_preflight {"ON"} else {"OFF"});
        f.render_widget(
            Paragraph::new(text)
                .wrap(Wrap { trim: true })
                .style(p.base)
                .block(p.panel(" Configuration preview ")),
            side,
        );
    } else if let Some((_, last)) = l.rows.last() {
        let end = l
            .help
            .or(l.editor)
            .map(|r| r.y)
            .unwrap_or(l.main.bottom().saturating_sub(1));
        let y = last.bottom();
        if end > y {
            f.render_widget(
                Paragraph::new(details)
                    .wrap(Wrap { trim: true })
                    .style(p.dim),
                Rect::new(l.main.x + 1, y, width, end - y),
            );
        }
    }
}
