mod library;
pub mod theme;
mod views;
use crate::app::{App, TABS};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Paragraph, Wrap},
    Frame,
};
use theme::{color, Theme};

pub(super) fn safe(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '�' } else { c })
        .collect()
}
pub(super) struct Palette {
    pub base: Style,
    pub selected: Style,
    pub heading: Style,
    pub muted: Style,
    pub green: Color,
    pub orange: Color,
}
impl Palette {
    fn new(theme: &Theme) -> Self {
        let c = |v: &str| color(v).unwrap_or(Color::Reset);
        let base = Style::default()
            .bg(c(&theme.background))
            .fg(c(&theme.foreground));
        Self {
            base,
            selected: base.bg(c(&theme.selection)).fg(c(&theme.pink)),
            heading: base.fg(c(&theme.cyan)).add_modifier(Modifier::BOLD),
            muted: base.fg(c(&theme.purple)),
            green: c(&theme.green),
            orange: Color::Rgb(255, 184, 108),
        }
    }
    pub fn panel<'a>(&self, title: impl Into<Line<'a>>) -> Block<'a> {
        Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(self.muted)
            .title(title)
    }
}
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
    let palette = Palette::new(theme);
    let base = palette.base;
    frame.render_widget(Block::default().style(base), frame.area());
    let header_line = Line::from(vec![
        Span::styled(" ◈ LLAMANCER ", palette.muted.add_modifier(Modifier::BOLD)),
        Span::styled(" / Summon your models. ", base),
        Span::styled(concat!("v", env!("CARGO_PKG_VERSION")), palette.heading),
    ]);
    frame.render_widget(Paragraph::new(header_line), header);
    for (index, rect) in tab_regions(frame.area()).into_iter().enumerate() {
        let style = if index == app.tab {
            base.fg(color(&theme.pink).unwrap_or(Color::Reset))
                .bg(color(&theme.selection).unwrap_or(Color::Reset))
        } else {
            base
        };
        frame.render_widget(
            Paragraph::new(TABS[index]).style(style).block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(palette.muted),
            ),
            rect,
        );
    }
    if app.tab == 0 && !app.help && app.edit.is_none() && app.profile_editor.is_none() {
        library::draw(frame, body, app, &palette);
    } else {
        let (title, content) = views::content(app, body);
        frame.render_widget(
            Paragraph::new(content)
                .style(base)
                .wrap(Wrap { trim: false })
                .block(palette.panel(title)),
            body,
        );
    }
    let footer_text = if app.notice.is_empty() {
        match app.tab {
            0 => "r scan  / search  f favorite  s sort  p profile  l load  ? help",
            1 => "/ search  Enter open  d queue  n next page  Backspace back  ? help",
            2 => "r resume  R run queue  x pause  z remove entry  ? help",
            3 => "r refresh  s router  i probe  x stop  l/u load/unload  a agent  A current  w web  b btop  g logs",
            _ => "↑↓ select  Enter edit  ? help  q quit",
        }
    } else {
        &app.notice
    };
    frame.render_widget(
        Paragraph::new(safe(footer_text)).style(palette.heading),
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
            for tab in 0..TABS.len() {
                let app = App {
                    tab,
                    ..App::default()
                };
                terminal.draw(|frame| draw(frame, &app)).unwrap();
            }
        }
    }
}
