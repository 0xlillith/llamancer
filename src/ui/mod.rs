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

/// The viewport used by both row rendering and pointer hit testing.
pub(super) fn viewport(cursor: usize, height: u16, reserved: u16) -> (usize, usize) {
    let capacity = usize::from(height.saturating_sub(reserved)).max(1);
    (cursor.saturating_sub(capacity.saturating_sub(1)), capacity)
}
pub fn row_at(area: Rect, app: &App, column: u16, row: u16) -> Option<usize> {
    let body = regions(area)[2];
    if app.backend_open {
        if app.backend_busy || app.backend_confirmation.is_some() {
            return None;
        }
        let row = row.checked_sub(body.y.checked_add(3)?)?;
        return (row < 3
            && body.width >= 3
            && column > body.x
            && column < body.right().saturating_sub(1)
            && body.y + 3 + row < body.bottom().saturating_sub(1))
        .then_some(usize::from(row));
    }
    if app.help
        || app.hide_confirmation.is_some()
        || app.quarantine_confirmation.is_some()
        || app.profile_editor.is_some()
        || app.edit.is_some()
        || app.searching
        || app.discovery.editing
    {
        return None;
    }
    let (rect, header, reserved, cursor, len) = match app.tab {
        0 if app.show_invalid => (body, 2, 4, app.invalid_cursor, app.invalid.len()),
        0 => {
            if app.details && body.width < 100 {
                return None;
            }
            let list = library::list_area(body);
            (list, 2, 3, app.model_cursor, app.visible().len())
        }
        1 => (
            body,
            if app.discovery.repo.is_some() { 5 } else { 4 },
            6,
            app.discovery.cursor,
            app.discovery.len(),
        ),
        2 => {
            let d = app.downloads.as_ref()?;
            (body, 3, 5, d.cursor, d.queue.items.len())
        }
        3 => {
            if app.server.show_logs {
                return None;
            }
            let s = app.server.snapshot.as_ref()?;
            (body, 5, 7, app.server.cursor, s.models.len())
        }
        4 => (body, 1, 5, app.setting, crate::settings::Field::ALL.len()),
        _ => return None,
    };
    let inner_row = row.checked_sub(rect.y.checked_add(header)?)?;
    if rect.width < 3
        || column <= rect.x
        || column >= rect.right().saturating_sub(1)
        || row >= rect.bottom().saturating_sub(1)
    {
        return None;
    }
    let (offset, capacity) = viewport(cursor, rect.height, reserved);
    let index = offset.checked_add(usize::from(inner_row))?;
    (usize::from(inner_row) < capacity && index < len).then_some(index)
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
        let label = if rect.width >= 12 {
            TABS[index]
        } else if rect.width >= 6 {
            ["Lib", "Find", "DL", "Srv", "Cfg"][index]
        } else {
            ["1", "2", "3", "4", "5"][index]
        };
        frame.render_widget(
            Paragraph::new(if index == app.tab && rect.width >= 4 {
                format!(">{label}")
            } else {
                label.into()
            })
            .style(style)
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(palette.muted),
            ),
            rect,
        );
    }
    if app.tab == 0
        && !app.backend_open
        && !app.help
        && app.edit.is_none()
        && app.profile_editor.is_none()
    {
        library::draw(frame, body, app, &palette);
    } else {
        let (title, content) = views::content(app, body);
        let paragraph = Paragraph::new(content)
            .style(base)
            .block(palette.panel(title));
        // Keep list rows on one terminal line, so cursor and pointer geometry agree.
        let paragraph = if app.help
            || app.profile_editor.is_some()
            || app.edit.is_some()
            || app.quarantine_confirmation.is_some()
        {
            paragraph.wrap(Wrap { trim: false })
        } else {
            paragraph
        };
        frame.render_widget(paragraph, body);
    }
    let footer_text = if app.backend_open && app.notice.is_empty() {
        "↑↓ select  i install  Enter use  v version  p probe  b benchmark  d remove  x cancel  Esc back"
    } else if app.notice.is_empty() {
        match app.tab {
            0 => "r scan  / search  f favorite  s sort  p profile  l load  i invalid  h hide  t quarantine  ? help",
            1 => "/ search  Enter open  d queue  n next page  Backspace back  ? help",
            2 => "r resume  R run queue  x pause  t quarantine  T restore  z remove entry  ? help",
            3 => "r refresh  D detach  s router  R restart  i probe  x stop  l/u load/unload  a agent  A current  w web  b btop  g logs  B backends",
            _ => "↑↓ select  Enter edit  B backends  ? help  q quit",
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
    fn pointer_rows_match_rendered_viewports() {
        let area = Rect::new(0, 0, 80, 24);
        let mut app = App {
            tab: 4,
            setting: 12,
            ..App::default()
        };
        let body = regions(area)[2];
        let (offset, _) = viewport(app.setting, body.height, 5);
        assert_eq!(row_at(area, &app, 5, body.y + 1), Some(offset));
        assert_eq!(row_at(area, &app, body.x, body.y + 1), None);
        app.help = true;
        assert_eq!(row_at(area, &app, 5, body.y + 1), None);
        app.help = false;
        app.tab = 1;
        assert_eq!(row_at(area, &app, 5, body.y + 4), None);
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
    fn renders_backend_and_invalid_views_at_small_sizes_in_high_contrast() {
        let root = tempfile::tempdir().unwrap();
        let theme = Theme::load(root.path(), "high-contrast").unwrap();
        for (w, h) in [(1, 1), (20, 5), (40, 12), (80, 24), (120, 36)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            let app = App {
                backend_open: true,
                backend_confirmation: Some('d'),
                ..App::default()
            };
            terminal.draw(|f| draw_themed(f, &app, &theme)).unwrap();
            let app = App {
                show_invalid: true,
                invalid_cursor: 12,
                invalid_total: 20,
                invalid: (0..20)
                    .map(|i| {
                        (
                            format!("/tmp/{i}.gguf").into(),
                            std::io::ErrorKind::InvalidData,
                        )
                    })
                    .collect(),
                ..App::default()
            };
            terminal.draw(|f| draw_themed(f, &app, &theme)).unwrap();
        }
    }
    #[test]
    fn modal_row_hit_testing_matches_rendered_rows() {
        let area = Rect::new(0, 0, 80, 24);
        let body = regions(area)[2];
        let mut app = App {
            backend_open: true,
            ..App::default()
        };
        assert_eq!(row_at(area, &app, 5, body.y + 3), Some(0));
        assert_eq!(row_at(area, &app, 5, body.y + 5), Some(2));
        app.backend_confirmation = Some('d');
        assert_eq!(row_at(area, &app, 5, body.y + 5), None);
        app.backend_confirmation = None;
        app.backend_open = false;
        app.show_invalid = true;
        app.invalid = (0..30)
            .map(|i| {
                (
                    format!("/tmp/{i}.gguf").into(),
                    std::io::ErrorKind::InvalidData,
                )
            })
            .collect();
        app.invalid_cursor = 20;
        let (offset, _) = viewport(20, body.height, 4);
        assert_eq!(row_at(area, &app, 5, body.y + 2), Some(offset));
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
