use ratatui::{Frame, layout::{Constraint, Layout}, style::{Color, Style}, widgets::{Block, Paragraph, Tabs}};
use crate::app::{App, TABS};

pub fn draw(frame: &mut Frame, app: &App) {
    let [header, tabs, body, footer] = Layout::vertical([
        Constraint::Length(2), Constraint::Length(3), Constraint::Min(1), Constraint::Length(1)
    ]).areas(frame.area());
    frame.render_widget(Block::default().style(Style::default().bg(Color::Rgb(40,42,54))), frame.area());
    frame.render_widget(Paragraph::new("LLAMANCER  /  Summon your models.").style(Style::default().fg(Color::Rgb(189,147,249))), header);
    frame.render_widget(Tabs::new(TABS.to_vec()).select(app.tab).highlight_style(Style::default().fg(Color::Rgb(255,121,198))).block(Block::bordered()), tabs);
    frame.render_widget(Paragraph::new("Development scaffold — functionality coming soon.").block(Block::bordered().title(TABS[app.tab])), body);
    frame.render_widget(Paragraph::new("Tab / arrows: navigate   q: quit"), footer);
}

