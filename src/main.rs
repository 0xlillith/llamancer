use std::io;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use llamancer::{app::{Action, App}, ui};

fn main() -> io::Result<()> {
    let mut terminal = ratatui::init();
    let result = (|| {
        let mut app = App::default();
        while !app.quit {
            terminal.draw(|frame| ui::draw(frame, &app))?;
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press { continue; }
                let action = match key.code {
                    KeyCode::Char('q') => Action::Quit,
                    KeyCode::Esc => Action::Escape,
                    KeyCode::Char('?') => Action::ToggleHelp,
                    KeyCode::Tab | KeyCode::Right => Action::Next,
                    KeyCode::BackTab | KeyCode::Left => Action::Previous,
                    _ => continue,
                };
                app.dispatch(action);
            }
        }
        Ok(())
    })();
    ratatui::restore();
    result
}

