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
    let (title, content) = if app.help {
        ("Help", "Tab / Right: next view\nShift+Tab / Left: previous view\n1–5: select view\n?: toggle help\nEsc: close help or quit\nq: quit")
    } else {
        (
            TABS[app.tab],
            match app.tab {
                0 => "Model library — indexing not implemented yet.",
                1 => "Hugging Face discovery — planned.",
                2 => "Download queue — planned.",
                3 => "Server management — planned.",
                _ => "Settings — configuration file supported; editor planned.",
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
    frame.render_widget(
        Paragraph::new("Tab / arrows / 1–5: navigate   ?: help   q: quit").style(base),
        footer,
    );
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
