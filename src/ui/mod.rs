mod catalog;
pub mod choices;
mod chrome;
pub mod controls;
pub mod icons;
mod launch;
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
pub(super) fn clip(value: &str, width: usize) -> String {
    let value = safe(value);
    if ratatui::text::Span::raw(&value).width() <= width {
        return value;
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in value.chars() {
        let n = ratatui::text::Span::raw(ch.to_string()).width();
        if used + n > width.saturating_sub(1) {
            break;
        }
        used += n;
        out.push(ch);
    }
    out.push('…');
    out
}
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
    if app.launch_menu.is_some() {
        return (body, None);
    }
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
    if app.folder_browsing
        || app.launch_menu.is_some()
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
pub fn launch_hit(
    area: Rect,
    app: &App,
    x: u16,
    y: u16,
) -> Option<(Option<usize>, crossterm::event::KeyCode)> {
    if app.folder_browsing {
        return None;
    }
    let (body, _) = content_regions(regions(area)[2], app);
    launch::hit(body, app, x, y).map(|h| (h.index, h.key))
}
pub fn row_at(area: Rect, app: &App, column: u16, row: u16) -> Option<usize> {
    if app.folder_browsing {
        return None;
    }
    let (body, _) = content_regions(regions(area)[2], app);
    if app.launch_menu.is_some() {
        return launch::hit(body, app, column, row).and_then(|h| h.index);
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
        1 if body.height < 7 || body.width < 12 => return None,
        1 => (
            body,
            catalog::DISCOVER_HEADER,
            catalog::DISCOVER_RESERVED,
            app.discovery.cursor,
            app.discovery.len(),
        ),
        2 if body.height < 8 || body.width < 12 => return None,
        2 => {
            let d = app.downloads.as_ref()?;
            let groups = d.bundles();
            (
                body,
                catalog::DOWNLOAD_HEADER,
                catalog::DOWNLOAD_RESERVED,
                d.bundle_cursor(&groups),
                groups.len(),
            )
        }
        3 => {
            if app.server.show_logs {
                return None;
            }
            let s = app.server.snapshot.as_ref()?;
            (body, 5, 7, app.server.cursor, s.models.len())
        }
        4 => {
            let indices = crate::settings::Field::category_indices(app.setting);
            let cursor = indices.iter().position(|&i| i == app.setting).unwrap_or(0);
            let (offset, capacity) = viewport(cursor, body.height, 6);
            let inner_row = usize::from(row.checked_sub(body.y.checked_add(2)?)?);
            if column <= body.x
                || column >= body.right().saturating_sub(1)
                || row >= body.bottom().saturating_sub(1)
                || inner_row >= capacity
            {
                return None;
            }
            return indices.get(offset + inner_row).copied();
        }
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
    if app.folder_browsing {
        frame.render_widget(
            Paragraph::new("Choose a folder in the desktop dialog.\n\nEsc cancels.")
                .style(palette.heading)
                .wrap(Wrap { trim: true })
                .block(palette.panel(" Folder selection ")),
            body,
        );
    } else if let Some(action) = app.backend_confirmation.filter(|_| app.backend_open) {
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
    } else if app.launch_menu.is_some() {
        launch::draw(frame, body, app, &palette);
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
    } else if app.tab == 1 && catalog::ordinary(app) {
        catalog::discover(frame, body, app, &palette);
    } else if app.tab == 2 && catalog::ordinary(app) {
        catalog::downloads(frame, body, app, &palette);
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
    controls::draw(frame, app, &palette);
    choices::draw(frame, app, &palette);
    let footer_text = if app.folder_browsing {
        "Esc cancel folder selection"
    } else if app.picker.is_some() {
        "↑↓ select  Enter choose  Esc cancel"
    } else if app.launch_menu.is_some() {
        "Enter action  ↑↓ choose  C presets  c custom context  Esc back"
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
        "Enter select  o open folder  O open root  Esc back"
    } else if app.backend_open && app.backend_confirmation.is_some() {
        "y confirm  n cancel  Esc back"
    } else if app.edit.is_some() {
        "Enter save  F4 browse workspace folder  Esc cancel"
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
            1 => "F3 search  s sort  F filters  Enter open/download  ? help",
            2 => "Enter resume  x pause  ? help",
            3 => "Enter agent  P project  g logs  ? help",
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
        && app.launch_menu.is_none()
        && !app.launching_agent
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
        assert_eq!(row_at(area, &app, 5, main.y + 2), Some(0));
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
    fn launch_layout_keeps_focused_row_visible_and_steppers_clickable() {
        use crate::models::{gguf::Metadata, group::Group, Candidate};
        use crossterm::event::KeyCode;
        let group = Group {
            name: "Fixture Qwen Q4".into(),
            files: vec![Candidate {
                path: "/models/fixture.gguf".into(),
                bytes: 1024,
                version: 3,
                metadata: Metadata::default(),
            }],
            expected: 1,
            complete: true,
            projector: false,
        };
        let mut app = App {
            launch_menu: Some(group),
            launch_advanced: true,
            ..Default::default()
        };
        for (w, h) in [(1, 1), (20, 5), (50, 14), (80, 24), (140, 36)] {
            for index in 0..10 {
                app.launch_cursor = index;
                let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
                terminal.draw(|f| draw(f, &app)).unwrap();
                let area = Rect::new(0, 0, w, h);
                if w >= 50 && h >= 14 {
                    let hits = (0..h)
                        .flat_map(|y| (0..w).map(move |x| (x, y)))
                        .filter_map(|(x, y)| launch_hit(area, &app, x, y))
                        .collect::<Vec<_>>();
                    assert!(hits.contains(&(Some(index), KeyCode::Enter)));
                    if (3..=5).contains(&index) {
                        assert!(hits.contains(&(Some(index), KeyCode::Left)));
                        assert!(hits.contains(&(Some(index), KeyCode::Right)));
                    }
                }
            }
        }
        app.context_input = Some("65536".into());
        let area = Rect::new(0, 0, 80, 24);
        assert!((0..24).all(|y| (0..80).all(|x| launch_hit(area, &app, x, y).is_none())));
    }
    #[test]
    fn choice_picker_does_not_activate_underlying_controls() {
        let mut app = App {
            tab: 1,
            ..Default::default()
        };
        app.picker = Some(choices::Picker::new(choices::Kind::Sort, &app));
        for (w, h) in [(1, 1), (30, 12), (80, 24), (140, 36)] {
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            t.draw(|f| draw(f, &app)).unwrap();
            let area = Rect::new(0, 0, w, h);
            assert!((0..h).all(|y| (0..w).all(|x| controls::hit(area, &app, x, y).is_none())));
            if w >= 80 {
                let hits = (0..h)
                    .flat_map(|y| (0..w).map(move |x| (x, y)))
                    .filter_map(|(x, y)| choices::hit(area, &app, x, y))
                    .collect::<std::collections::HashSet<_>>();
                assert_eq!(hits.len(), 5);
            }
        }
    }
    #[test]
    fn visible_controls_share_pointer_geometry() {
        let app = App::default();
        let area = Rect::new(0, 0, 80, 24);
        let body = regions(area)[2];
        assert_eq!(
            controls::hit(area, &app, 2, body.y + 1),
            Some(controls::Command::Key(crossterm::event::KeyCode::Char('2')))
        );
        assert_eq!(controls::hit(area, &app, 0, body.y + 1), None);
        let mut app = app;
        app.tab = 1;
        app.discovery.editing = true;
        assert_eq!(
            controls::hit(area, &app, 2, body.y + 3),
            Some(controls::Command::SubmitSearch)
        );
        app.help = true;
        assert_eq!(controls::hit(area, &app, 2, body.y + 3), None);
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
        let indices = crate::settings::Field::category_indices(app.setting);
        let cursor = indices.iter().position(|&i| i == app.setting).unwrap();
        let (offset, _) = viewport(cursor, body.height, 6);
        assert_eq!(row_at(area, &app, 5, body.y + 2), Some(indices[offset]));
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
    fn populated_catalog_transfers_and_workspace_keep_mouse_geometry() {
        use crate::{
            downloads::Controller,
            huggingface::{discovery::Variant, File, Repository},
            settings::Field,
        };
        use crossterm::event::KeyCode;
        let tmp = tempfile::tempdir().unwrap();
        let mut app = App::default();
        let files: Vec<_> = (0..30)
            .map(|i| File {
                repo: "fixture/model".into(),
                revision: "a".repeat(40),
                name: format!("model-{i:02}.gguf"),
                size: 1024 * 1024,
                sha256: None,
            })
            .collect();
        app.discovery.repos = (0..30)
            .map(|i| Repository {
                id: format!("fixture/model-{i:02}"),
                downloads: 1200,
                likes: 10,
                trending_score: None,
                created_at: None,
                updated_at: None,
                gated: serde_json::Value::Bool(false),
                private: false,
            })
            .collect();
        app.discovery.variants = files
            .iter()
            .map(|f| Variant {
                name: f.name.clone(),
                files: vec![f.clone()],
                expected: 1,
            })
            .collect();
        let mut downloads = Controller::new(tmp.path().join("queue.toml")).unwrap();
        downloads.add(files, tmp.path().into()).unwrap();
        downloads.cursor = 20;
        app.downloads = Some(downloads);
        app.discovery.cursor = 20;
        app.setting = Field::ALL
            .iter()
            .position(|f| *f == Field::WorkspaceRoot)
            .unwrap();
        for (w, h) in [(30, 12), (50, 14), (80, 24), (120, 36), (180, 50)] {
            let area = Rect::new(0, 0, w, h);
            for variant in [false, true] {
                app.tab = 1;
                app.discovery.repo = variant.then(|| "fixture/model".into());
                let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
                term.draw(|f| draw(f, &app)).unwrap();
                let (body, _) = content_regions(regions(area)[2], &app);
                let (offset, cap) = viewport(
                    app.discovery.cursor,
                    body.height,
                    catalog::DISCOVER_RESERVED,
                );
                if cap > 0 && body.height >= 7 {
                    assert_eq!(
                        row_at(area, &app, body.x + 2, body.y + catalog::DISCOVER_HEADER),
                        Some(offset)
                    );
                }
            }
            app.tab = 2;
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            term.draw(|f| draw(f, &app)).unwrap();
            let (body, _) = content_regions(regions(area)[2], &app);
            let (offset, cap) = viewport(20, body.height, catalog::DOWNLOAD_RESERVED);
            if cap > 0 && body.height >= 8 {
                assert_eq!(
                    row_at(area, &app, body.x + 2, body.y + catalog::DOWNLOAD_HEADER),
                    Some(offset)
                );
            }
            app.tab = 4;
            term.draw(|f| draw(f, &app)).unwrap();
            if w >= 50 {
                assert!(
                    (0..h).any(|y| (0..w).any(|x| controls::hit(area, &app, x, y)
                        == Some(controls::Command::Key(KeyCode::Char('O')))))
                );
            }
            app.folder_browsing = true;
            term.draw(|f| draw(f, &app)).unwrap();
            assert!((0..h)
                .all(|y| (0..w).all(|x| row_at(area, &app, x, y).is_none()
                    && controls::hit(area, &app, x, y).is_none())));
            app.folder_browsing = false;
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
