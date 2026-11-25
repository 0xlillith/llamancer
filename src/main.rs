use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
        MouseButton, MouseEventKind,
    },
    execute,
};
use llamancer::{
    app::{Action, App},
    config::{self, Config},
    jobs::Job,
    models::{self, Scan},
    settings::{self, Field},
    ui::{self, theme::Theme},
};
use std::{io, time::Duration};

fn main() -> io::Result<()> {
    let root = config::config_dir()?;
    let path = root.join("config.toml");
    let mut config = Config::load(&path)?;
    let mut theme = Theme::load(&root, &config.theme)?;
    if !path.exists() {
        config.save(&path)?;
    }
    let mut terminal = ratatui::init();
    let result: io::Result<()> = (|| {
        execute!(io::stdout(), EnableMouseCapture)?;
        let mut app = App {
            config: config.clone(),
            ..App::default()
        };
        let mut job: Option<Job<Scan>> = None;
        while !app.quit {
            if let Some(active) = job.as_mut() {
                if let Some(outcome) = active.poll() {
                    let cancelled = active.is_cancelled();
                    job.take().expect("active job").finish();
                    app.scanning = false;
                    match outcome {
                        _ if cancelled => app.notice = "Scan cancelled".into(),
                        Ok(scan) => {
                            app.notice = format!(
                                "Scan complete: {} candidates, {} skipped",
                                scan.candidates.len(),
                                scan.skipped
                            );
                            app.candidates = models::group::group(scan.candidates);
                            app.details = false;
                            app.model_cursor = 0;
                        }
                        Err(e) => app.notice = format!("Scan ended: {}", e.kind()),
                    }
                }
            }
            terminal.draw(|frame| ui::draw_themed(frame, &app, &theme))?;
            if job.is_some() && !event::poll(Duration::from_millis(150))? {
                continue;
            }
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        app.dispatch(Action::Quit);
                        continue;
                    }
                    if let Some(editor) = app.profile_editor.as_mut() {
                        let field = editor.selected();
                        if let Some(input) = editor.input.as_mut() {
                            match key.code {
                                KeyCode::Esc => editor.input = None,
                                KeyCode::Backspace => {
                                    input.pop();
                                }
                                KeyCode::Char('u')
                                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                                {
                                    input.clear()
                                }
                                KeyCode::Enter => {
                                    let value = input.clone();
                                    match field.set(&mut editor.draft, &value) {
                                        Ok(()) => {
                                            editor.input = None;
                                            app.notice = "Draft updated; press s to save".into();
                                        }
                                        Err(error) => app.notice = error.to_string(),
                                    }
                                }
                                KeyCode::Char(c) if !c.is_control() && input.len() < 128 => {
                                    input.push(c)
                                }
                                _ => {}
                            }
                        } else {
                            match key.code {
                                KeyCode::Esc => {
                                    app.profile_editor = None;
                                    app.notice = "Profile edit cancelled".into();
                                }
                                KeyCode::Up => {
                                    editor.cursor = (editor.cursor + editor.fields().len() - 1)
                                        % editor.fields().len()
                                }
                                KeyCode::Down => {
                                    editor.cursor = (editor.cursor + 1) % editor.fields().len()
                                }
                                KeyCode::Char('a') => {
                                    editor.advanced = !editor.advanced;
                                    editor.cursor = 0;
                                }
                                KeyCode::Enter => {
                                    editor.input = Some(editor.selected().value(&editor.draft))
                                }
                                KeyCode::Char('s') => {
                                    let mut next = config.clone();
                                    next.profiles
                                        .insert(editor.id.clone(), editor.draft.clone());
                                    match next.save(&path) {
                                        Ok(()) => {
                                            config = next;
                                            app.config = config.clone();
                                            app.profile_editor = None;
                                            app.notice = "Inference profile saved".into();
                                        }
                                        Err(_) => {
                                            app.notice =
                                                "Could not save profile; draft retained".into()
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        continue;
                    }
                    if app.searching {
                        match key.code {
                            KeyCode::Enter | KeyCode::Esc => app.searching = false,
                            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                app.query.clear();
                                app.model_cursor = 0;
                            }
                            KeyCode::Backspace => {
                                app.query.pop();
                                app.model_cursor = 0;
                            }
                            KeyCode::Char(c) if !c.is_control() && app.query.len() < 512 => {
                                app.query.push(c);
                                app.model_cursor = 0;
                            }
                            _ => {}
                        }
                        continue;
                    }
                    if let Some((field, draft)) = app.edit.as_mut() {
                        match key.code {
                            KeyCode::Esc => {
                                app.edit = None;
                                app.notice = "Edit cancelled".into();
                            }
                            KeyCode::Backspace => {
                                draft.pop();
                            }
                            KeyCode::Enter => {
                                match settings::apply(&config, *field, draft, &root).and_then(
                                    |(next, next_theme)| {
                                        next.save(&path)?;
                                        Ok((next, next_theme))
                                    },
                                ) {
                                    Ok((next, next_theme)) => {
                                        if next.model_roots != config.model_roots {
                                            app.candidates.clear();
                                            app.details = false;
                                            app.model_cursor = 0;
                                            if let Some(active) = &job {
                                                active.cancel();
                                            }
                                        }
                                        config = next;
                                        theme = next_theme;
                                        app.config = config.clone();
                                        app.edit = None;
                                        app.notice = "Settings saved".into();
                                    }
                                    Err(_) => {
                                        app.notice =
                                            "Invalid setting or save failed; changes not applied"
                                                .into()
                                    }
                                }
                            }
                            KeyCode::Char(c) if !c.is_control() && draft.len() < 4096 => {
                                draft.push(c)
                            }
                            _ => {}
                        }
                        continue;
                    }
                    let action = match key.code {
                        KeyCode::Char('q') => Action::Quit,
                        KeyCode::Esc => Action::Escape,
                        KeyCode::Char('?') => Action::ToggleHelp,
                        KeyCode::Tab | KeyCode::Right => Action::Next,
                        KeyCode::BackTab | KeyCode::Left => Action::Previous,
                        KeyCode::Char(c @ '1'..='5') => {
                            Action::Select((c as usize) - ('1' as usize))
                        }
                        KeyCode::Up if !app.help && app.tab == 4 => {
                            app.setting = (app.setting + Field::ALL.len() - 1) % Field::ALL.len();
                            continue;
                        }
                        KeyCode::Down if !app.help && app.tab == 4 => {
                            app.setting = (app.setting + 1) % Field::ALL.len();
                            continue;
                        }
                        KeyCode::Enter if !app.help && app.tab == 4 => {
                            let field = Field::ALL[app.setting];
                            app.edit = Some((field, field.initial(&config)));
                            continue;
                        }
                        KeyCode::Enter if !app.help && app.tab == 0 && app.selected().is_some() => {
                            app.details = !app.details;
                            continue;
                        }
                        KeyCode::Up if !app.help && app.tab == 0 => {
                            app.model_cursor = app.model_cursor.saturating_sub(1);
                            continue;
                        }
                        KeyCode::Down if !app.help && app.tab == 0 => {
                            app.model_cursor =
                                (app.model_cursor + 1).min(app.visible().len().saturating_sub(1));
                            continue;
                        }
                        KeyCode::Char('p') if !app.help && app.tab == 0 => {
                            if let Some(group) = app.selected() {
                                if group.projector || !group.complete {
                                    app.notice =
                                        "Select a complete model group, not a projector".into();
                                } else {
                                    let id = models::library::identity(group);
                                    let draft =
                                        config.profiles.get(&id).cloned().unwrap_or_default();
                                    app.profile_editor =
                                        Some(models::profile::Editor::new(id, draft));
                                }
                            }
                            continue;
                        }
                        KeyCode::Char('/') if !app.help && app.tab == 0 => {
                            app.searching = true;
                            app.details = false;
                            continue;
                        }
                        KeyCode::Char('s') if !app.help && app.tab == 0 => {
                            app.sort = app.sort.next();
                            app.model_cursor = 0;
                            app.details = false;
                            continue;
                        }
                        KeyCode::Char('v') if !app.help && app.tab == 0 => {
                            app.only_favorites = !app.only_favorites;
                            app.model_cursor = 0;
                            app.details = false;
                            continue;
                        }
                        KeyCode::Char('f') if !app.help && app.tab == 0 => {
                            if let Some(g) = app.selected() {
                                let id = models::library::identity(g);
                                let mut next = config.clone();
                                if !next.favorites.remove(&id) {
                                    next.favorites.insert(id);
                                }
                                match next.save(&path) {
                                    Ok(()) => {
                                        config = next;
                                        app.config = config.clone();
                                        app.clamp_cursor();
                                        app.details = false;
                                        app.notice = "Favorites saved".into();
                                    }
                                    Err(_) => app.notice = "Could not save favorites".into(),
                                }
                            }
                            continue;
                        }
                        KeyCode::Char('r') if !app.help && app.tab == 0 && job.is_none() => {
                            let roots = config.model_roots.clone();
                            match Job::start(move |cancel| models::scan(roots, &cancel)) {
                                Ok(new) => {
                                    job = Some(new);
                                    app.scanning = true;
                                    app.notice = "Scanning model roots…".into();
                                }
                                Err(_) => app.notice = "Could not start scan".into(),
                            }
                            continue;
                        }
                        KeyCode::Char('x') if !app.help && app.tab == 0 => {
                            if let Some(active) = &job {
                                active.cancel();
                                app.notice = "Cancelling scan…".into();
                            }
                            continue;
                        }
                        _ => continue,
                    };
                    app.dispatch(action);
                }
                Event::Mouse(mouse)
                    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && app.edit.is_none()
                        && !app.searching
                        && app.profile_editor.is_none() =>
                {
                    let size = terminal.size()?;
                    if let Some(index) = ui::tab_at(
                        ratatui::layout::Rect::new(0, 0, size.width, size.height),
                        mouse.column,
                        mouse.row,
                    ) {
                        app.dispatch(Action::Select(index));
                    }
                }
                _ => {}
            }
        }
        // Dropping a job requests cancellation without blocking terminal restoration.
        drop(job);
        Ok(())
    })();
    let mouse_result = execute!(io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result.and(mouse_result)
}
