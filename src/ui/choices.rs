use super::{regions, viewport, Palette};
use crate::{app::App, settings::Field};
use ratatui::{
    layout::{Position, Rect},
    style::Modifier,
    text::Line,
    widgets::{Clear, Paragraph},
    Frame,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Sort,
    Filters,
    Author,
    License,
    Quant,
    Setting(Field),
    Context(bool),
    Category,
}
pub struct Picker {
    pub kind: Kind,
    pub cursor: usize,
    pub options: Vec<String>,
}
impl Picker {
    pub fn new(kind: Kind, app: &App) -> Self {
        use Kind::*;
        let choices: &[&str] = match kind {
            Sort if app.discovery.repo.is_some() => &[
                "Balanced Q4/Q5 first",
                "Smallest first",
                "Largest first",
                "Name A-Z",
            ],
            Sort => &[
                "Most downloaded",
                "Trending",
                "Newest",
                "Recently updated",
                "Most liked",
            ],
            Filters if app.discovery.repo.is_some() => &["Quantization"],
            Filters => &["Quantization", "Publisher", "License", "Clear all filters"],
            Author => &[
                "Any publisher",
                "unsloth",
                "bartowski",
                "lmstudio-community",
                "DavidAU",
                "ggml-org",
            ],
            License => &[
                "Any license",
                "apache-2.0",
                "mit",
                "llama3",
                "gemma",
                "other",
            ],
            Quant => &[
                "Any quantization",
                "Q4_K_M",
                "Q5_K_M",
                "Q6_K",
                "Q8_0",
                "IQ3_S",
                "UD-Q4_K_XL",
                "BF16",
            ],
            Setting(f) => f.choices(),
            Category => &["Storage", "Inference", "Agent", "Workspace", "Appearance"],
            Context(_) => &[
                "auto",
                "8192",
                "16384",
                "32768",
                "65536",
                "131072",
                "262144",
                "Custom value…",
            ],
        };
        let selected = match kind {
            Sort if app.discovery.repo.is_some() => app.discovery.variant_sort.label().to_owned(),
            Sort => app.discovery.ranking.label().to_owned(),
            Setting(f) => f.initial(&app.config),
            Category => Field::ALL[app.setting].category().into(),
            Context(global) => if global {
                app.config.default_context
            } else {
                app.launch_menu
                    .as_ref()
                    .and_then(|g| app.config.effective_profile(g).engine.context_size)
            }
            .map(|n| n.to_string())
            .unwrap_or_else(|| "auto".into()),
            Author | License | Quant => {
                let prefix = match kind {
                    Author => "author:",
                    License => "license:",
                    _ => "quant:",
                };
                if kind == Quant && app.discovery.repo.is_some() {
                    app.discovery.variant_filter.clone()
                } else {
                    app.discovery
                        .query
                        .split_whitespace()
                        .find_map(|w| w.strip_prefix(prefix))
                        .unwrap_or("")
                        .to_owned()
                }
            }
            _ => String::new(),
        };
        Self {
            kind,
            cursor: choices
                .iter()
                .position(|v| v.eq_ignore_ascii_case(&selected))
                .unwrap_or(0),
            options: choices.iter().map(|s| s.to_string()).collect(),
        }
    }
    fn title(&self) -> &'static str {
        match self.kind {
            Kind::Sort => "Sort by",
            Kind::Filters => "Filter models",
            Kind::Author => "Publisher",
            Kind::License => "License",
            Kind::Quant => "Quantization (repository variants)",
            Kind::Setting(_) => "Choose a value",
            Kind::Context(_) => "Context size (tokens)",
            Kind::Category => "Settings category",
        }
    }
}
fn area(screen: Rect) -> Rect {
    let body = regions(screen)[2];
    let w = body.width.min(62);
    let h = body.height.min(15);
    Rect::new(
        body.x + (body.width - w) / 2,
        body.y + (body.height - h) / 2,
        w,
        h,
    )
}
pub fn hit(screen: Rect, app: &App, x: u16, y: u16) -> Option<usize> {
    let p = app.picker.as_ref()?;
    let rect = area(screen);
    if rect.width < 3 || rect.height < 4 {
        return None;
    }
    let (offset, capacity) = viewport(p.cursor, rect.height, 3);
    let pos = Position::new(x, y);
    let rows = Rect::new(rect.x + 1, rect.y + 1, rect.width - 2, rect.height - 3);
    if !rows.contains(pos) {
        return None;
    }
    let i = offset + usize::from(y - rows.y);
    (i < offset + capacity && i < p.options.len()).then_some(i)
}
pub(super) fn draw(f: &mut Frame, app: &App, p: &Palette) {
    let Some(picker) = &app.picker else {
        return;
    };
    let r = area(f.area());
    f.render_widget(Clear, r);
    f.render_widget(p.panel(format!(" {} ", picker.title())).style(p.base), r);
    if r.width < 3 || r.height < 4 {
        return;
    }
    let (offset, capacity) = viewport(picker.cursor, r.height, 3);
    for (n, (i, value)) in picker
        .options
        .iter()
        .enumerate()
        .skip(offset)
        .take(capacity)
        .enumerate()
    {
        let selected = i == picker.cursor;
        let row = Rect::new(r.x + 1, r.y + 1 + n as u16, r.width - 2, 1);
        f.render_widget(
            Paragraph::new(format!(
                "{} {}",
                if selected { "›" } else { " " },
                super::safe(value)
            ))
            .style(if selected {
                p.selected.add_modifier(Modifier::BOLD)
            } else {
                p.base
            }),
            row,
        );
    }
    f.render_widget(
        Paragraph::new(Line::styled(" Click / Enter select · Esc cancel", p.dim)),
        Rect::new(r.x + 1, r.bottom() - 2, r.width - 2, 1),
    );
}
