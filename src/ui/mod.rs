use ratatui::{Frame, layout::{Constraint, Layout}, style::{Color, Style}, widgets::{Block, Paragraph, Tabs, Wrap}};
use crate::app::{App, TABS};

pub fn draw(frame: &mut Frame, app: &App) {
    let [header, tabs, body, footer] = Layout::vertical([
        Constraint::Length(2), Constraint::Length(3), Constraint::Min(1), Constraint::Length(1)
    ]).areas(frame.area());
    let base = Style::default().bg(Color::Rgb(40,42,54)).fg(Color::Rgb(248,248,242));
    frame.render_widget(Block::default().style(base), frame.area());
    frame.render_widget(Paragraph::new("LLAMANCER / Summon your models.").style(base.fg(Color::Rgb(189,147,249))), header);
    frame.render_widget(Tabs::new(TABS.to_vec()).select(app.tab).highlight_style(base.fg(Color::Rgb(255,121,198))).block(Block::bordered()), tabs);
    let (title, content) = if app.help {
        ("Help", "Tab / Right: next view\nShift+Tab / Left: previous view\n?: toggle help\nEsc: close help or quit\nq: quit")
    } else { (TABS[app.tab], "Development scaffold — functionality coming soon.") };
    frame.render_widget(Paragraph::new(content).wrap(Wrap { trim: false }).block(Block::bordered().title(title)), body);
    frame.render_widget(Paragraph::new("Tab / arrows: navigate   ?: help   q: quit"), footer);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};
    #[test]
    fn renders_small_terminal() {
        let mut terminal = Terminal::new(TestBackend::new(20, 5)).unwrap();
        terminal.draw(|frame| draw(frame, &App::default())).unwrap();
    }
}

