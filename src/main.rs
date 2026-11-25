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
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

fn run_child(
    terminal: &mut ratatui::DefaultTerminal,
    mut command: Command,
    stop: &AtomicBool,
) -> io::Result<()> {
    execute!(io::stdout(), DisableMouseCapture)?;
    ratatui::restore();
    let status = (|| {
        let mut child = command.spawn()?;
        loop {
            if let Some(status) = child.try_wait()? {
                return Ok(status);
            }
            if stop.load(Ordering::Acquire) {
                // Only signal the child we spawned, not the shell's foreground group.
                let _ = child.kill();
                return child.wait();
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    })();
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
fn backend_cli(mut args: impl Iterator<Item = std::ffi::OsString>) -> io::Result<()> {
    use llamancer::backends::{self, Kind};
    use std::sync::{
        atomic::{AtomicBool, AtomicU64},
        Arc,
    };
    let action = args
        .next()
        .and_then(|s| s.into_string().ok())
        .unwrap_or_default();
    if action == "check" {
        if args.next().is_some() {
            return Err(io::Error::other("Unexpected backend check argument"));
        }
        let cancel = Arc::new(AtomicBool::new(false));
        signal_hook::flag::register(libc::SIGINT, cancel.clone())?;
        signal_hook::flag::register(libc::SIGTERM, cancel.clone())?;
        println!("{}", backends::discovery::check(&cancel)?);
        return Ok(());
    }
    let root = backends::root()?;
    if action == "list" && args.next().is_none() {
        for release in backends::RELEASES {
            println!("Official llama.cpp {release}");
            for kind in Kind::ALL {
                if let Some(asset) = backends::catalog_version(kind, release) {
                    println!(
                        "{}: {} ({} bytes)",
                        kind.name(),
                        if asset.installed(&root) {
                            "installed"
                        } else {
                            "not installed"
                        },
                        asset.size
                    );
                } else {
                    println!("{}: unavailable on this architecture", kind.name());
                }
            }
        }
        return Ok(());
    }
    let kind = Kind::parse(
        &args
            .next()
            .and_then(|s| s.into_string().ok())
            .unwrap_or_default(),
    )?;
    let version = match args.next() {
        Some(value) => value
            .into_string()
            .map_err(|_| io::Error::other("Invalid release tag"))?,
        None => backends::RELEASE.into(),
    };
    if args.next().is_some() {
        return Err(io::Error::other("Unexpected backend argument"));
    }
    let asset = backends::catalog_version(kind, &version)
        .ok_or_else(|| io::Error::other("No pinned release asset for this architecture/version"))?;
    match action.as_str() {
        "install" => {
            let cancel = Arc::new(AtomicBool::new(false));
            signal_hook::flag::register(libc::SIGINT, cancel.clone())?;
            signal_hook::flag::register(libc::SIGTERM, cancel.clone())?;
            println!(
                "{}",
                backends::install(&root, asset, &cancel, &AtomicU64::new(0))?.display()
            );
        }
        "probe" => {
            if !asset.installed(&root) {
                return Err(io::Error::other("Backend not installed"));
            }
            println!("{}", backends::probe(&asset.binary(&root))?);
        }
        _ => {
            return Err(io::Error::other(
                "Use backend list, check, install KIND or probe KIND",
            ))
        }
    }
    Ok(())
}
fn main() -> io::Result<()> {
    let mut args = std::env::args_os().skip(1);
    if let Some(arg) = args.next() {
        if arg == "backend" {
            return backend_cli(args);
        }
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
                println!("Llamancer — Summon your models.\n\nUsage: llamancer [serve|backend|--help|--version]\n  serve   Supervise a local router without a TUI\n  backend list | check | install KIND [VERSION] | probe KIND [VERSION]");
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

    let stop = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(libc::SIGTERM, Arc::clone(&stop))?;
    let mut runtime = Runtime::new()?;
    if !runtime.app.config.scan_roots().is_empty() {
        runtime.scan()?;
    }
    let mut terminal = ratatui::init();
    let result = (|| -> io::Result<()> {
        execute!(io::stdout(), EnableMouseCapture)?;
        while !runtime.app.quit && !stop.load(Ordering::Acquire) {
            runtime.tick();
            terminal.draw(|frame| ui::draw_themed(frame, &runtime.app, &runtime.theme))?;
            // When idle, wait in bounded polls without repainting repeatedly.
            // SIGTERM can now wake an otherwise idle TUI.
            loop {
                if stop.load(Ordering::Acquire) {
                    return Ok(());
                }
                if event::poll(Duration::from_millis(150))? {
                    break;
                }
                if runtime.active() {
                    break;
                }
            }
            if stop.load(Ordering::Acquire) {
                return Ok(());
            }
            if !event::poll(Duration::ZERO)? {
                continue;
            }
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    match runtime.handle_key(key) {
                        Ok(Effect::None) => {}
                        Ok(Effect::Launch(plan, mode)) => {
                            runtime.app.notice = match plan
                                .terminal_command(mode)
                                .and_then(|command| run_child(&mut terminal, command, &stop))
                                .and(plan.finish())
                            {
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
                                match run_child(&mut terminal, Command::new("btop"), &stop) {
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
                    if !runtime.modal() || runtime.app.backend_open || runtime.app.show_invalid =>
                {
                    let size = terminal.size()?;
                    let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
                    match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            if let Some(index) =
                                ui::row_at(area, &runtime.app, mouse.column, mouse.row)
                            {
                                runtime.select_row(index);
                            } else if !runtime.modal() {
                                if let Some(index) = ui::tab_at(area, mouse.column, mouse.row) {
                                    runtime.app.dispatch(Action::Select(index));
                                }
                            }
                        }
                        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                            let code = if mouse.kind == MouseEventKind::ScrollUp {
                                event::KeyCode::Up
                            } else {
                                event::KeyCode::Down
                            };
                            if let Err(e) = runtime
                                .handle_key(event::KeyEvent::new(code, event::KeyModifiers::NONE))
                            {
                                runtime.app.notice = e.to_string();
                            }
                        }
                        _ => {}
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
