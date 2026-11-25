use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton,
    MouseEventKind,
};
use crossterm::execute;
use llamancer::{
    app::{Action, App},
    config::{self, Config},
    ui::{self, theme::Theme},
};
use std::io;

fn main() -> io::Result<()> {
    let root = config::config_dir()?;
    let path = root.join("config.toml");
    let config = Config::load(&path)?;
    let theme = Theme::load(&root, &config.theme)?;
    if !path.exists() {
        config.save(&path)?;
    }
    let mut terminal = ratatui::init();
    let result: io::Result<()> = (|| {
        execute!(io::stdout(), EnableMouseCapture)?;
        let mut app = App::default();
        while !app.quit {
            terminal.draw(|frame| ui::draw_themed(frame, &app, &theme))?;
            let action = match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Char('c')
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL) =>
                    {
                        Action::Quit
                    }
                    KeyCode::Char('q') => Action::Quit,
                    KeyCode::Esc => Action::Escape,
                    KeyCode::Char('?') => Action::ToggleHelp,
                    KeyCode::Tab | KeyCode::Right => Action::Next,
                    KeyCode::BackTab | KeyCode::Left => Action::Previous,
                    KeyCode::Char(c @ '1'..='5') => Action::Select((c as usize) - ('1' as usize)),
                    _ => continue,
                },
                Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                    let size = terminal.size()?;
                    match ui::tab_at(
                        ratatui::layout::Rect::new(0, 0, size.width, size.height),
                        mouse.column,
                        mouse.row,
                    ) {
                        Some(index) => Action::Select(index),
                        None => continue,
                    }
                }
                Event::Resize(_, _) => continue,
                _ => continue,
            };
            app.dispatch(action);
        }
        Ok(())
    })();
    let mouse_result = execute!(io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result.and(mouse_result)
}
