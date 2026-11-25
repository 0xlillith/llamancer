use crate::{
    config::Config,
    models::{
        group::Group,
        library::{self, Sort},
    },
    settings::Field,
};
pub const TABS: [&str; 5] = ["Library", "Discover", "Downloads", "Server", "Settings"];
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Next,
    Previous,
    Select(usize),
    ToggleHelp,
    Escape,
    Quit,
}
#[derive(Default)]
pub struct App {
    pub tab: usize,
    pub nerd_available: bool,
    pub activity_frame: usize,
    pub backend_open: bool,
    pub benchmark_open: bool,
    pub setup_open: bool,
    pub setup_cursor: usize,
    pub backend_cursor: usize,
    pub backend_version: usize,
    pub backend_confirmation: Option<char>,
    pub backend_busy: bool,
    pub backend_hardware: String,
    pub backend_status: String,
    pub backend_progress: Option<(u64, u64)>,
    pub backend_root: Option<std::path::PathBuf>,
    pub help: bool,
    pub quit: bool,
    pub config: Config,
    pub setting: usize,
    pub picker: Option<crate::ui::choices::Picker>,
    pub projects: Option<Vec<std::path::PathBuf>>,
    pub download_destination: Option<crate::downloads::DestinationChoice>,
    pub project_cursor: usize,
    pub edit: Option<(Field, String)>,
    pub notice: String,
    pub scanning: bool,
    pub candidates: Vec<Group>,
    pub invalid: Vec<(std::path::PathBuf, std::io::ErrorKind)>,
    pub show_invalid: bool,
    pub invalid_cursor: usize,
    pub invalid_total: usize,
    pub quarantine_confirmation: Option<bool>,
    pub hide_confirmation: Option<std::path::PathBuf>,
    pub model_cursor: usize,
    pub details: bool,
    pub launch_menu: Option<Group>,
    pub launch_cursor: usize,
    pub launch_advanced: bool,
    pub context_input: Option<String>,
    pub launching_agent: bool,
    pub probe_warning: Option<String>,
    pub inspector_scroll: u16,
    pub fit_report: Option<String>,
    pub query: String,
    pub searching: bool,
    pub only_favorites: bool,
    pub sort: Sort,
    pub profile_editor: Option<crate::models::profile::Editor>,
    pub server: crate::llama::session::Controller,
    pub discovery: crate::huggingface::discovery::Discovery,
    pub downloads: Option<crate::downloads::Controller>,
}
impl App {
    pub fn launch_order(&self) -> &'static [usize] {
        if self.launch_advanced {
            &[0, 1, 2, 3, 4, 8, 5, 6, 7]
        } else {
            &[0, 1, 2, 3, 4, 8]
        }
    }

    pub fn nerd_icons(&self) -> bool {
        self.config.icon_mode.enabled(self.nerd_available)
    }
    pub fn visible(&self) -> Vec<usize> {
        library::visible(
            &self.candidates,
            &self.query,
            &self.config.favorites,
            self.only_favorites,
            self.sort,
        )
    }
    pub fn selected(&self) -> Option<&Group> {
        self.visible()
            .get(self.model_cursor)
            .and_then(|&i| self.candidates.get(i))
    }
    pub fn clamp_cursor(&mut self) {
        self.model_cursor = self
            .model_cursor
            .min(self.visible().len().saturating_sub(1));
    }

    pub fn dispatch(&mut self, action: Action) {
        let previous = self.tab;
        match action {
            Action::Next if !self.help => self.tab = (self.tab + 1) % TABS.len(),
            Action::Previous if !self.help => self.tab = (self.tab + TABS.len() - 1) % TABS.len(),
            Action::Select(index) if !self.help && index < TABS.len() => self.tab = index,
            Action::ToggleHelp => self.help = !self.help,
            Action::Escape if self.help => self.help = false,
            Action::Escape if self.show_invalid && self.tab == 0 => self.show_invalid = false,
            Action::Escape if self.details && self.tab == 0 => self.details = false,
            Action::Escape | Action::Quit => self.quit = true,
            _ => {}
        }
        if previous != self.tab {
            self.searching = false;
            self.discovery.editing = self.tab == 1 && !self.discovery.busy();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn navigation_wraps() {
        let mut app = App::default();
        app.dispatch(Action::Previous);
        assert_eq!(app.tab, 4);
        app.dispatch(Action::Next);
        assert_eq!(app.tab, 0);
    }
    #[test]
    fn entering_discover_focuses_search_and_leaving_clears_focus() {
        let mut app = App::default();
        app.dispatch(Action::Select(1));
        assert!(app.discovery.editing);
        app.dispatch(Action::Select(2));
        assert!(!app.discovery.editing);
        app.dispatch(Action::Previous);
        assert!(app.discovery.editing);
    }
    #[test]
    fn help_is_modal() {
        let mut app = App::default();
        app.dispatch(Action::ToggleHelp);
        app.dispatch(Action::Next);
        assert_eq!(app.tab, 0);
        app.dispatch(Action::Escape);
        assert!(!app.help && !app.quit);
        app.dispatch(Action::Escape);
        assert!(app.quit);
    }
}
