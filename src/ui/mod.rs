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
    let (title, content) = if let Some((field, draft)) = &app.edit {
        (
            "Edit settings",
            format!(
                "{}\n\n{}▏\n\nEnter: save   Esc: cancel",
                field.label(),
                draft
            ),
        )
    } else if app.help {
        ("Help", "Tab / arrows / 1–5: navigate\nLibrary: r scan, x cancel, Up/Down select\nSettings: Up/Down select, Enter edit\n?: toggle help\nEsc: close help or quit\nq / Ctrl+C: quit".into())
    } else {
        (
            TABS[app.tab],
            match app.tab {
                0 => {
                    let mut lines = vec![
                        format!(
                            "{} GGUF candidates | {} roots | {}",
                            app.candidates.len(),
                            app.config.model_roots.len(),
                            if app.scanning { "Scanning" } else { "Idle" }
                        ),
                        "r: scan   x: cancel".into(),
                    ];
                    if app.candidates.is_empty() {
                        lines.push("Add a model directory in Settings, then press r.".into());
                    }
                    let cursor = app.model_cursor.min(app.candidates.len().saturating_sub(1));
                    let visible = usize::from(body.height.saturating_sub(5)).max(1);
                    let offset = cursor.saturating_sub(visible.saturating_sub(1));
                    for (index, candidate) in
                        app.candidates.iter().enumerate().skip(offset).take(visible)
                    {
                        let name = candidate
                            .path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy();
                        lines.push(format!(
                            "{} {} | {} MiB | GGUF v{}",
                            if index == cursor { ">" } else { " " },
                            name,
                            candidate.bytes / 1048576,
                            candidate.version
                        ));
                    }
                    lines.join("\n")
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
    fn renders_small_terminal() {
        for (w, h) in [(1, 1), (20, 5), (80, 24)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|frame| draw(frame, &App::default())).unwrap();
        }
    }
}
