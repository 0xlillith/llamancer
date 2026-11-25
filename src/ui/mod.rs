mod chrome;
pub mod icons;
mod library;
pub mod theme;
mod views;
use crate::app::App;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, BorderType, Paragraph, Wrap},
    Frame,
};
use theme::{color, Theme};

pub(crate) fn safe(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '�' } else { c })
        .collect()
}
// Keep the end of an active query visible, including the editing cursor.
fn search_value(value: &str, editing: bool, width: usize) -> String {
    if value.is_empty() {
        return if editing {
            String::new()
        } else {
            "click to type".into()
        };
    }
    let value = safe(value);
    if !editing {
        return value;
    }
    let mut used = 0;
    let mut suffix = Vec::new();
    for c in value.chars().rev() {
        used += ratatui::text::Span::raw(c.to_string()).width();
        if used > width.saturating_sub(1) {
            break;
        }
        suffix.push(c);
    }
    let truncated = suffix.len() != value.chars().count();
    let tail: String = suffix.into_iter().rev().collect();
    if truncated {
        format!("…{tail}")
    } else {
        tail
    }
}
pub(super) struct Palette {
    pub base: Style,
    pub selected: Style,
    pub heading: Style,
    pub muted: Style,
    pub green: Color,
    pub orange: Color,
    pub dim: Style,
    pub border: Style,
    pub surface: Style,
    pub label: Style,
    pub brand: Style,
    nerd: bool,
}
impl Palette {
    fn new(theme: &Theme, nerd: bool) -> Self {
        let c = |v: &str| color(v).unwrap_or(Color::Reset);
        let base = Style::default()
            .bg(c(&theme.background))
            .fg(c(&theme.foreground));
        let mix = |a: Color, b: Color, t: u16| match (a, b) {
            (Color::Rgb(r, g, bl), Color::Rgb(r2, g2, b2)) => {
                let blend =
                    |x: u8, y: u8| ((u16::from(x) * (100 - t) + u16::from(y) * t) / 100) as u8;
                Color::Rgb(blend(r, r2), blend(g, g2), blend(bl, b2))
            }
            _ => b,
        };
        let bg = c(&theme.background);
        let fg = c(&theme.foreground);
        let accent = c(&theme.purple);
        let surface = base.bg(mix(bg, fg, 8));
        Self {
            base,
            dim: base.fg(mix(bg, fg, 70)),
            border: base.fg(mix(bg, fg, 40)),
            surface,
            label: surface.fg(accent),
            brand: Style::default()
                .bg(accent)
                .fg(bg)
                .add_modifier(Modifier::BOLD),
            nerd,
            selected: base.bg(c(&theme.selection)).fg(c(&theme.pink)),
            heading: base.fg(c(&theme.cyan)).add_modifier(Modifier::BOLD),
            muted: base.fg(c(&theme.purple)),
            green: c(&theme.green),
            orange: Color::Rgb(255, 184, 108),
        }
    }
    pub fn icon(&self, icon: icons::Icon) -> &'static str {
        icon.glyph(self.nerd)
    }
    pub fn panel<'a>(&self, title: impl Into<Line<'a>>) -> Block<'a> {
        Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(self.border)
            .title_style(self.heading)
            .title(title)
    }
}
fn regions(area: Rect) -> [Rect; 4] {
    Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(if area.height >= 24 {
            3
        } else if area.height >= 12 {
            2
        } else {
            1
        }),
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
fn content_regions(body: Rect, app: &App) -> (Rect, Option<Rect>) {
    let ordinary = app.tab != 0
        && !app.help
        && app.edit.is_none()
        && app.profile_editor.is_none()
        && app.fit_report.is_none()
        && app.quarantine_confirmation.is_none()
        && !app.server.show_logs;
    if body.width >= 120
        && body.height >= 15
        && app.backend_confirmation.is_none()
        && (ordinary
            || app.setup_open
            || app.projects.is_some()
            || app.download_destination.is_some()
            || app.benchmark_open
            || app.backend_open)
    {
        let [main, side] =
            Layout::horizontal([Constraint::Min(82), Constraint::Length(34)]).areas(body);
        (main, Some(side))
    } else {
        (body, None)
    }
}
pub fn search_at(area: Rect, app: &App, column: u16, row: u16) -> bool {
    if app.launch_menu.is_some()
        || app.launching_agent
        || app.backend_open
        || app.setup_open
        || app.projects.is_some()
        || app.download_destination.is_some()
        || app.fit_report.is_some()
        || app.help
        || app.show_invalid
        || app.hide_confirmation.is_some()
        || app.quarantine_confirmation.is_some()
        || app.profile_editor.is_some()
        || app.edit.is_some()
    {
        return false;
    }
    let (body, _) = content_regions(regions(area)[2], app);
    let y = match app.tab {
        0 => body.y.saturating_add(2),
        1 if !app.discovery.busy() => body.y.saturating_add(1),
        _ => return false,
    };
    row == y
        && row < body.bottom().saturating_sub(1)
        && column > body.x
        && column < body.right().saturating_sub(1)
}
pub fn server_actions(app: &App) -> String {
    let server = &app.server;
    let active = server.owns_server() || server.snapshot.is_some();
    let router = server.snapshot.as_ref().is_some_and(|s| s.router);
    let first = if server.owns_server() {
        "[ Stop server ]"
    } else if active {
        "[ Disconnect ]"
    } else {
        "[ Connect ]"
    };
    let second = if router {
        "[ Unload model ]"
    } else if active {
        "[ Library ]"
    } else {
        "[ Start router ]"
    };
    let third = if active {
        "[ Open agent ]"
    } else {
        "[ Backends ]"
    };
    let fourth = if active { "[ Web UI ]" } else { "[ Setup ]" };
    format!("{first:<16}{second:<17}{third:<15}{fourth}")
}
pub fn server_action_at(
    area: Rect,
    app: &App,
    column: u16,
    row: u16,
) -> Option<crossterm::event::KeyCode> {
    if app.tab != 3
        || app.server.show_logs
        || app.backend_open
        || app.setup_open
        || app.projects.is_some()
        || app.help
        || app.edit.is_some()
    {
        return None;
    }
    let (body, _) = content_regions(regions(area)[2], app);
    if row != body.y + 3 || column <= body.x || column >= body.right().saturating_sub(1) {
        return None;
    }
    use crossterm::event::KeyCode::Char;
    let x = column - body.x - 1;
    let active = app.server.owns_server() || app.server.snapshot.is_some();
    match x {
        0..=14 => Some(Char(if app.server.owns_server() {
            'x'
        } else if active {
            'D'
        } else {
            'r'
        })),
        16..=31 => Some(Char(
            if app.server.snapshot.as_ref().is_some_and(|s| s.router) {
                'u'
            } else if active {
                '1'
            } else {
                's'
            },
        )),
        33..=46 => Some(if active {
            crossterm::event::KeyCode::Enter
        } else {
            Char('B')
        }),
        48..=59 => Some(Char(if active { 'w' } else { 'G' })),
        _ => None,
    }
}
pub fn row_at(area: Rect, app: &App, column: u16, row: u16) -> Option<usize> {
    let (body, _) = content_regions(regions(area)[2], app);
    if app.launch_menu.is_some() {
        let index = usize::from(row.checked_sub(body.y.checked_add(4)?)?);
        return (index < 5
            && app.context_input.is_none()
            && row < body.bottom().saturating_sub(1)
            && column > body.x
            && column < body.right().saturating_sub(1))
        .then_some(index);
    }
    if app.setup_open {
        let row = row.checked_sub(body.y.checked_add(2)?)?;
        let (offset, capacity) = viewport(app.setup_cursor, body.height, 4);
        let index = offset.checked_add(usize::from(row))?;
        return (usize::from(row) < capacity
            && index < crate::setup::LABELS.len()
            && column > body.x
            && column < body.right().saturating_sub(1)
            && body.y + 2 + row < body.bottom().saturating_sub(1))
        .then_some(index);
    }
    if app.benchmark_open {
        return None;
    }
    if let Some(choice) = &app.download_destination {
        let row = row.checked_sub(body.y.checked_add(2)?)?;
        let (offset, capacity) = viewport(choice.cursor, body.height, 4);
        let index = offset.checked_add(usize::from(row))?;
        return (usize::from(row) < capacity
            && index < choice.roots.len()
            && column > body.x
            && column < body.right().saturating_sub(1)
            && body.y + 2 + row < body.bottom().saturating_sub(1))
        .then_some(index);
    }
    if let Some(projects) = &app.projects {
        let row = row.checked_sub(body.y.checked_add(2)?)?;
        let (offset, capacity) = viewport(app.project_cursor, body.height, 4);
        let index = offset.checked_add(usize::from(row))?;
        return (usize::from(row) < capacity
            && index < projects.len()
            && column > body.x
            && column < body.right().saturating_sub(1)
            && body.y + 2 + row < body.bottom().saturating_sub(1))
        .then_some(index);
    }
    if app.backend_open {
        if app.backend_busy || app.backend_confirmation.is_some() {
            return None;
        }
        let row = row.checked_sub(body.y.checked_add(3)?)?;
        return (usize::from(row) < crate::backends::Kind::ALL.len()
            && body.width >= 3
            && column > body.x
            && column < body.right().saturating_sub(1)
            && body.y + 3 + row < body.bottom().saturating_sub(1))
        .then_some(usize::from(row));
    }
    if app.help
        || app.fit_report.is_some()
        || app.hide_confirmation.is_some()
        || app.quarantine_confirmation.is_some()
        || app.profile_editor.is_some()
        || app.edit.is_some()
    {
        return None;
    }
    let (rect, header, reserved, cursor, len) = match app.tab {
        0 if app.show_invalid => (body, 2, 4, app.invalid_cursor, app.invalid.len()),
        0 => {
            if app.details {
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
            let groups = d.bundles();
            (body, 3, 5, d.bundle_cursor(&groups), groups.len())
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
pub fn clamp_inspector_scroll(area: Rect, app: &mut App, theme: &Theme) {
    if app.tab != 0 || !app.details || app.inspector_scroll == 0 {
        return;
    }
    let body = regions(area)[2];
    let [_, table] = Layout::vertical([Constraint::Length(4), Constraint::Min(1)]).areas(body);
    let palette = Palette::new(theme, app.nerd_icons());
    app.inspector_scroll = app
        .inspector_scroll
        .min(library::scroll_limit(table, app, &palette));
}
pub fn draw(frame: &mut Frame, app: &App) {
    draw_themed(frame, app, &Theme::default());
}

pub fn draw_themed(frame: &mut Frame, app: &App, theme: &Theme) {
    let [header, _, body, footer] = regions(frame.area());
    let palette = Palette::new(theme, app.nerd_icons());
    let base = palette.base;
    frame.render_widget(Block::default().style(base), frame.area());
    chrome::header(frame, header, app, &palette);
    for (index, rect) in tab_regions(frame.area()).into_iter().enumerate() {
        chrome::tab(frame, rect, index, app, &palette);
    }
    let (body, aside) = content_regions(body, app);
    if let Some(action) = app.backend_confirmation.filter(|_| app.backend_open) {
        use crate::backends::{Kind, RELEASES};
        let kind = Kind::ALL[app.backend_cursor].name();
        let release = RELEASES[app.backend_version];
        let verb = match action {
            'i' => "Install",
            'd' => "Remove",
            'b' => "Benchmark",
            _ => "Confirm",
        };
        let message = format!("{verb} {kind} {release}?\ny confirm / n cancel");
        let detail = match action {
            'i' => "Download and verify the official archive.",
            'd' => "Delete only this managed backend. Stop all inference first.",
            'b' => "Run the selected local GGUF (maximum 512 MiB).",
            _ => "",
        };
        let content = format!("{message}\n{detail}");
        frame.render_widget(
            Paragraph::new(content)
                .style(base)
                .wrap(Wrap { trim: false })
                .block(palette.panel(" Backend confirmation ")),
            body,
        );
    } else if app.tab == 0
        && app.launch_menu.is_none()
        && !app.backend_open
        && app.projects.is_none()
        && app.fit_report.is_none()
        && !app.setup_open
        && app.download_destination.is_none()
        && !app.help
        && app.edit.is_none()
        && app.profile_editor.is_none()
    {
        library::draw(frame, body, app, &palette);
    } else {
        let (title, content) = views::content(app, body);
        let lines: Vec<Line> = content
            .lines()
            .enumerate()
            .map(|(index, line)| {
                if title == "Discover" && index == 0 {
                    let style = if app.discovery.editing {
                        palette.selected
                    } else {
                        palette.surface
                    };
                    let padding = usize::from(body.width.saturating_sub(2))
                        .saturating_sub(ratatui::text::Line::raw(line).width());
                    return Line::styled(format!("{line}{}", " ".repeat(padding)), style);
                }
                let mut styled = chrome::row(line, &palette, app.tab == 3 && app.server.show_logs);
                if line.starts_with("> ") {
                    let padding =
                        usize::from(body.width.saturating_sub(2)).saturating_sub(styled.width());
                    styled.spans.push(ratatui::text::Span::styled(
                        " ".repeat(padding),
                        palette.selected,
                    ));
                }
                styled
            })
            .collect();
        let icon = match title {
            "Model memory advisory" => icons::Icon::Memory,
            "Comparable benchmark history" => icons::Icon::Chart,
            "Project workspace" => icons::Icon::Project,
            "Guided setup" => icons::Icon::Check,
            "Backend Manager" => icons::Icon::Server,
            "Discover" => icons::Icon::Discover,
            "Downloads" | "Download destination" => icons::Icon::Downloads,
            "Server" => icons::Icon::Server,
            "Settings" | "Edit settings" | "Inference profile" => icons::Icon::Settings,
            _ => icons::Icon::Brand,
        };
        let paragraph = Paragraph::new(lines)
            .style(base)
            .block(palette.panel(format!(" {} {title} ", palette.icon(icon))));
        // Keep list rows on one terminal line, so cursor and pointer geometry agree.
        let paragraph = if app.help
            || app.profile_editor.is_some()
            || app.fit_report.is_some()
            || app.edit.is_some()
            || app.quarantine_confirmation.is_some()
        {
            paragraph.wrap(Wrap { trim: false })
        } else {
            paragraph
        };
        frame.render_widget(paragraph, body);
    }
    if let Some(side) = aside {
        chrome::aside(frame, side, app, &palette);
    }
    let footer_text = if app.launch_menu.is_some() {
        "Enter action  ↑↓ choose  ←→ context presets  c custom context  Esc back"
    } else if app.launching_agent {
        "Starting inference → agent window  Esc cancel agent launch"
    } else if app.setup_open {
        "↑↓ choose step  Enter open  Esc skip  G reopen"
    } else if app.benchmark_open {
        "Esc back  h back"
    } else if app.fit_report.is_some() {
        "Esc close  Enter close  p profile (after closing)"
    } else if app.download_destination.is_some() {
        "↑↓ choose storage  Enter confirm & download  Esc cancel"
    } else if app.projects.is_some() {
        "↑↓ choose project  Enter select  Esc cancel"
    } else if app.backend_open && app.backend_confirmation.is_some() {
        "y confirm  n cancel  Esc back"
    } else if app.edit.is_some() {
        "Enter save  Esc cancel  Ctrl+U clear"
    } else if app.profile_editor.is_some() {
        "↑↓ select  Enter edit  a mode  s save  Esc back"
    } else if app.help {
        "? close help  Esc close"
    } else if app.searching || app.discovery.editing {
        "Enter search  Tab/Esc results  Ctrl+U clear"
    } else if app.details && app.tab == 0 {
        "PgUp/PgDn scroll  Enter close  Esc close  p profile  m fit  l Run model"
    } else if app.backend_open {
        "↑↓ select  i install  Enter use  v version  c check  p probe  b benchmark  h compare  d remove  x cancel  Esc back"
    } else {
        match app.tab {
            0 => "Enter open / double-click  j inspect  p profile  m fit  F3 search  r refresh  ? help",
            1 => "F3 focus search  s sort  Enter open/download  d download  n next page  Backspace back  ? help",
            2 => "r resume  R run queue  x pause  t quarantine  T restore  z remove entry  ? help",
            3 => "r refresh  D detach  s Start router  R restart  i probe  x stop  l/u load/unload  a Launch agent  A current  w web  b btop  g logs  B backends  P project",
            _ => "↑↓ select  Enter edit  T theme  I icons  B backends  P project  G setup  ? help  q quit",
        }
    };
    let footer_text = if footer.width < 60
        && app.backend_confirmation.is_none()
        && app.projects.is_none()
        && app.download_destination.is_none()
        && app.fit_report.is_none()
        && !app.setup_open
        && !app.benchmark_open
        && app.edit.is_none()
        && app.profile_editor.is_none()
        && !app.help
        && !app.searching
        && !app.discovery.editing
        && !app.details
    {
        match (app.backend_open, app.tab) {
            (true, _) => "↑↓ select  i install  Enter use  v version  ? help",
            (false, 0) => "r scan  / find  p profile  m fit  ? help",
            (false, 1) => "/ find  Enter open  d queue  ? help",
            (false, 2) => "r run  x pause  T restore  ? help",
            (false, 3) => "s start  x stop  a agent  B backends  ? help",
            _ => "↑↓ select  Enter edit  T theme  I icons  ? help",
        }
    } else {
        footer_text
    };
    chrome::footer(frame, footer, footer_text, app, &palette);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::TABS;
    use ratatui::{backend::TestBackend, Terminal};
    #[test]
    fn icon_modes_and_wide_sidebar_are_adaptive() {
        let theme = Theme::default();
        let mut app = App {
            tab: 4,
            nerd_available: true,
            ..App::default()
        };
        let area = Rect::new(0, 0, 140, 36);
        let (main, side) = content_regions(regions(area)[2], &app);
        assert!(side.is_some());
        assert_eq!(row_at(area, &app, 5, main.y + 1), Some(0));
        assert_eq!(row_at(area, &app, side.unwrap().x + 2, main.y + 1), None);
        for (mode, expected) in [
            (icons::Mode::Auto, true),
            (icons::Mode::Nerd, true),
            (icons::Mode::Plain, false),
        ] {
            app.config.icon_mode = mode;
            for (w, h) in [(20, 5), (60, 14), (140, 36)] {
                let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
                term.draw(|f| draw_themed(f, &app, &theme)).unwrap();
                let buffer = term.backend().buffer();
                let seen = (0..h).any(|y| {
                    (0..w).any(|x| buffer[(x, y)].symbol() == icons::Icon::Brand.glyph(true))
                });
                assert_eq!(seen, expected);
            }
        }
        let (main, side) = content_regions(regions(Rect::new(0, 0, 80, 24))[2], &app);
        assert_eq!(main.width, 80);
        assert!(side.is_none());
    }
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
    fn search_field_hit_testing_respects_tabs_and_dialogs() {
        let area = Rect::new(0, 0, 120, 36);
        let body = regions(area)[2];
        let mut app = App::default();
        assert!(search_at(area, &app, 5, body.y + 2));
        app.dispatch(crate::app::Action::Select(1));
        assert!(search_at(area, &app, 5, body.y + 1));
        assert!(!search_at(area, &app, 5, body.y + 2));
        app.help = true;
        assert!(!search_at(area, &app, 5, body.y + 1));
        assert_eq!(search_value("abcdefghijkl", true, 5), "…ijkl");
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
    fn narrow_backend_confirmation_exposes_target_and_controls() {
        let app = App {
            tab: 4,
            backend_open: true,
            backend_cursor: 1,
            backend_confirmation: Some('d'),
            ..App::default()
        };
        for (w, h) in [(40, 12), (60, 14), (80, 24)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|f| draw(f, &app)).unwrap();
            let buffer = terminal.backend().buffer();
            let screen = (0..h)
                .map(|y| (0..w).map(|x| buffer[(x, y)].symbol()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(screen.contains("Remove vulkan b12721?"), "{screen}");
            assert!(screen.contains("y confirm / n cancel"), "{screen}");
        }
    }
    #[test]
    fn workspace_mouse_rows_match_viewport() {
        let area = Rect::new(0, 0, 80, 24);
        let app = App {
            projects: Some((0..30).map(|i| format!("/tmp/{i}").into()).collect()),
            project_cursor: 17,
            ..App::default()
        };
        let body = regions(area)[2];
        let (offset, _) = viewport(17, body.height, 4);
        assert_eq!(row_at(area, &app, 5, body.y + 2), Some(offset));
        assert_eq!(row_at(area, &app, 0, body.y + 2), None);
        for (w, h) in [(1, 1), (40, 12), (80, 24)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|f| draw(f, &app)).unwrap();
        }
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
