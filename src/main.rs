use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind, MouseButton,
        MouseEventKind,
    },
    execute,
    terminal::{self, EnterAlternateScreen},
};
use llamancer::{
    app::Action,
    runtime::{Effect, Runtime},
    ui,
};
use std::{
    io,
    process::{Command, Stdio},
    time::Duration,
};

fn run_child(terminal: &mut ratatui::DefaultTerminal, mut command: Command) -> io::Result<()> {
    execute!(io::stdout(), DisableMouseCapture)?;
    ratatui::restore();
    let status = command.status();
    // Always restore the TUI before reporting a failed spawn or exit.
    terminal::enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    terminal.clear()?;
    if status?.success() {
        Ok(())
    } else {
        Err(io::Error::other("Child process exited with an error"))
    }
}
fn main() -> io::Result<()> {
    let mut args = std::env::args_os().skip(1);
    if let Some(arg) = args.next() {
        if args.next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Unexpected argument; see --help",
            ));
        }
        match arg.to_str() {
            Some("serve") => return llamancer::service::from_config(),
            Some("--version" | "-V") => {
                println!("llamancer {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            Some("--help" | "-h") => {
                println!("Llamancer — Summon your models.\n\nUsage: llamancer [serve|--help|--version]\n  serve   Supervise a local router without a TUI");
                return Ok(());
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Unknown argument; see --help",
                ))
            }
        }
    }

    let mut runtime = Runtime::new()?;
    if !runtime.app.config.scan_roots().is_empty() {
        runtime.scan()?;
    }
    let mut terminal = ratatui::init();
    let result = (|| -> io::Result<()> {
        execute!(io::stdout(), EnableMouseCapture)?;
        while !runtime.app.quit {
            runtime.tick();
            terminal.draw(|frame| ui::draw_themed(frame, &runtime.app, &runtime.theme))?;
            if runtime.active() && !event::poll(Duration::from_millis(150))? {
                continue;
            }
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    match runtime.handle_key(key) {
                        Ok(Effect::None) => {}
                        Ok(Effect::Launch(plan, mode)) => {
                            runtime.app.notice =
                                match run_child(&mut terminal, plan.terminal_command(mode)) {
                                    Ok(()) => {
                                        if mode == llamancer::agents::TerminalMode::Current {
                                            "Agent session ended".into()
                                        } else {
                                            "Kitty launch accepted".into()
                                        }
                                    }
                                    Err(e) => {
                                        if mode == llamancer::agents::TerminalMode::Current {
                                            e.to_string()
                                        } else {
                                            format!("{e}; press A for current terminal")
                                        }
                                    }
                                };
                        }
                        Ok(Effect::Monitor) => {
                            runtime.app.notice =
                                match run_child(&mut terminal, Command::new("btop")) {
                                    Ok(()) => "Monitor closed".into(),
                                    Err(e) => e.to_string(),
                                };
                        }
                        Ok(Effect::Open(url)) => {
                            let result = Command::new("xdg-open")
                                .arg(url)
                                .stdin(Stdio::null())
                                .stdout(Stdio::null())
                                .stderr(Stdio::null())
                                .spawn();
                            runtime.app.notice = match result {
                                Ok(mut child) => {
                                    std::thread::spawn(move || {
                                        let _ = child.wait();
                                    });
                                    "Opening server Web UI".into()
                                }
                                Err(e) => e.to_string(),
                            };
                        }
                        Err(e) => {
                            runtime.input_failed();
                            runtime.app.notice = e.to_string();
                        }
                    }
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && !runtime.modal() =>
                {
                    let size = terminal.size()?;
                    if let Some(index) = ui::tab_at(
                        ratatui::layout::Rect::new(0, 0, size.width, size.height),
                        mouse.column,
                        mouse.row,
                    ) {
                        runtime.app.dispatch(Action::Select(index));
                    }
                }
                _ => {}
            }
        }
        Ok(())
    })();
    let mouse_result = execute!(io::stdout(), DisableMouseCapture);
    ratatui::restore();
    let shutdown = runtime.shutdown();
    result.and(mouse_result).and(shutdown)
}
