mod app;
mod ui;
mod agents;
mod llama;
mod models;
mod huggingface;
mod config;

use std::io;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};

fn main() -> io::Result<()> {
    let mut terminal = ratatui::init();
    let result = (|| {
        let mut app = app::App::default();
        loop {
            terminal.draw(|frame| ui::draw(frame, &app))?;
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press { continue; }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Tab | KeyCode::Right => app.next(),
                    KeyCode::BackTab | KeyCode::Left => app.previous(),
                    _ => {}
                }
            }
        }
        Ok(())
    })();
    ratatui::restore();
    result
}

