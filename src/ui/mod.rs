pub mod theme;
use crate::app::{App, TABS};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Paragraph, Wrap},
    Frame,
};
use theme::{color, Theme};

fn regions(area: Rect) -> [Rect; 4] {
    Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area)
}

fn tab_regions(area: Rect) -> [Rect; 5] {
    Layout::horizontal([Constraint::Ratio(1, 5); 5]).areas(regions(area)[1])
}

pub fn tab_at(area: Rect, column: u16, row: u16) -> Option<usize> {
    tab_regions(area)
        .iter()
        .position(|r| column >= r.x && column < r.right() && row >= r.y && row < r.bottom())
}

pub fn draw(frame: &mut Frame, app: &App) {
    draw_themed(frame, app, &Theme::default());
}

pub fn draw_themed(frame: &mut Frame, app: &App, theme: &Theme) {
    let [header, _, body, footer] = regions(frame.area());
    let base = Style::default()
        .bg(color(&theme.background).unwrap_or(Color::Reset))
        .fg(color(&theme.foreground).unwrap_or(Color::Reset));
    frame.render_widget(Block::default().style(base), frame.area());
    frame.render_widget(
        Paragraph::new("LLAMANCER / Summon your models.")
            .style(base.fg(color(&theme.purple).unwrap_or(Color::Reset))),
        header,
    );
    for (index, rect) in tab_regions(frame.area()).into_iter().enumerate() {
        let style = if index == app.tab {
            base.fg(color(&theme.pink).unwrap_or(Color::Reset))
                .bg(color(&theme.selection).unwrap_or(Color::Reset))
        } else {
            base
        };
        frame.render_widget(
            Paragraph::new(TABS[index])
                .style(style)
                .block(Block::bordered()),
            rect,
        );
    }
    let (title, content) = if let Some(editor) = &app.profile_editor {
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
        ("Help", "Tab / arrows / 1–5: navigate\nLibrary: r scan, x cancel, Up/Down select, Enter details\n/: search, s: sort, f: favorite, v: favorites only, p: inference profile\nSettings: Up/Down select, Enter edit\n?: toggle help\nEsc: close help or quit\nq / Ctrl+C: quit".into())
    } else {
        (
            TABS[app.tab],
            match app.tab {
                0 => {
                    if app.details {
                        if let Some(g) = app.selected() {
                            let m = &g.files[0].metadata;
                            let safe = |s: &str| {
                                s.chars()
                                    .map(|c| if c.is_control() { '�' } else { c })
                                    .collect::<String>()
                            };
                            let text = |v: &Option<String>| {
                                v.as_deref().map(safe).unwrap_or_else(|| "Unknown".into())
                            };
                            let num = |v: Option<u64>| {
                                v.map(|n| n.to_string()).unwrap_or_else(|| "Unknown".into())
                            };
                            let mut lines = vec![
                                safe(&g.name),
                                format!("Name: {}", text(&m.name)),
                                format!("Architecture: {}", text(&m.architecture)),
                                format!(
                                    "Quantization: {} | Type ID: {}",
                                    crate::models::library::quantization(m.file_type),
                                    num(m.file_type)
                                ),
                                format!("Context: {} | Layers: {}", num(m.context), num(m.layers)),
                                format!(
                                    "Tokenizer: {} | Vocabulary: {}",
                                    text(&m.tokenizer),
                                    num(m.vocabulary)
                                ),
                                format!(
                                    "Tensor elements: {} | Size: {:.2} GiB",
                                    g.elements(),
                                    g.bytes() as f64 / 1073741824.0
                                ),
                                format!(
                                    "Shards: {}/{} | {}",
                                    g.files.len(),
                                    g.expected,
                                    if g.complete {
                                        "Present"
                                    } else {
                                        "Incomplete or inconsistent"
                                    }
                                ),
                                "Metadata only; loadability not verified.".into(),
                                "p: inference profile | Enter / Esc: close".into(),
                            ];
                            lines.extend(g.files.iter().map(|f| safe(&f.path.to_string_lossy())));
                            lines.join("\n")
                        } else {
                            "No selection".into()
                        }
                    } else {
                        let indices = app.visible();
                        let mut lines = vec![
                            format!(
                                "{} / {} GGUF groups | {} roots | {}",
                                indices.len(),
                                app.candidates.len(),
                                app.config.model_roots.len(),
                                if app.scanning { "Scanning" } else { "Idle" }
                            ),
                            format!(
                                "Search: {}{} | Sort: {} | {}",
                                app.query,
                                if app.searching { "▏" } else { "" },
                                app.sort.label(),
                                if app.only_favorites {
                                    "Favorites"
                                } else {
                                    "All"
                                }
                            ),
                            "r scan / search s sort f favorite v filter p profile Enter details"
                                .into(),
                        ];
                        if app.candidates.is_empty() {
                            lines.push("Add a model directory in Settings, then press r.".into());
                        }
                        if indices.is_empty() && !app.candidates.is_empty() {
                            lines.push("No matches. Press / to edit the search.".into());
                        }
                        let cursor = app.model_cursor.min(indices.len().saturating_sub(1));
                        let visible = usize::from(body.height.saturating_sub(6)).max(1);
                        let offset = cursor.saturating_sub(visible.saturating_sub(1));
                        for (index, candidate) in indices
                            .iter()
                            .map(|&i| &app.candidates[i])
                            .enumerate()
                            .skip(offset)
                            .take(visible)
                        {
                            let name: String = candidate
                                .name
                                .chars()
                                .map(|c| if c.is_control() { '�' } else { c })
                                .collect();
                            lines.push(format!(
                                "{}{} {} | {} | {:.2} GiB | {}",
                                if index == cursor { ">" } else { " " },
                                if app
                                    .config
                                    .favorites
                                    .contains(&crate::models::library::identity(candidate))
                                {
                                    "*"
                                } else {
                                    " "
                                },
                                name,
                                crate::models::library::quantization(
                                    candidate.files[0].metadata.file_type
                                ),
                                candidate.bytes() as f64 / 1073741824.0,
                                if candidate.projector {
                                    "Projector"
                                } else if candidate.complete {
                                    "Metadata read"
                                } else {
                                    "Incomplete"
                                }
                            ));
                        }
                        lines.join("\n")
                    }
                }
                1 => "Hugging Face discovery — planned.".into(),
                2 => "Download queue — planned.".into(),
                3 => "Server management — planned.".into(),
                _ => {
                    let mut lines = Vec::new();
                    for (i, field) in crate::settings::Field::ALL.into_iter().enumerate() {
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
    };
    frame.render_widget(
        Paragraph::new(content)
            .style(base)
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title(title)),
        body,
    );
    let footer_text = if app.notice.is_empty() {
        "Tab / arrows / 1–5: navigate   ?: help   q: quit"
    } else {
        &app.notice
    };
    frame.render_widget(Paragraph::new(footer_text).style(base), footer);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};
    #[test]
    fn mouse_hit_testing() {
        let area = Rect::new(0, 0, 80, 24);
        assert_eq!(tab_at(area, 1, 3), Some(0));
        assert_eq!(tab_at(area, 79, 3), Some(4));
        assert_eq!(tab_at(area, 10, 10), None);
    }
    #[test]
    fn renders_profile_editor() {
        let mut editor =
            crate::models::profile::Editor::new("/models/example.gguf".into(), Default::default());
        editor.advanced = true;
        editor.cursor = editor.fields().len() - 1;
        let mut app = App {
            profile_editor: Some(editor),
            ..App::default()
        };
        for (w, h) in [(1, 1), (20, 5), (80, 24)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|frame| draw(frame, &app)).unwrap();
        }
        app.profile_editor.as_mut().unwrap().input = Some("42".into());
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| draw(frame, &app)).unwrap();
    }
    #[test]
    fn renders_small_terminal() {
        for (w, h) in [(1, 1), (20, 5), (80, 24)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|frame| draw(frame, &App::default())).unwrap();
        }
    }
}
