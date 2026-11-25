pub const TABS: [&str; 5] = ["Library", "Discover", "Downloads", "Server", "Settings"];

#[derive(Default)]
pub struct App { pub tab: usize }

impl App {
    pub fn next(&mut self) { self.tab = (self.tab + 1) % TABS.len(); }
    pub fn previous(&mut self) { self.tab = (self.tab + TABS.len() - 1) % TABS.len(); }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn navigation_wraps() {
        let mut app = App::default();
        app.previous();
        assert_eq!(app.tab, 4);
        app.next();
        assert_eq!(app.tab, 0);
    }
}

