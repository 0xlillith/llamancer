//! Guided setup is a read-only status checklist; actions delegate to existing
//! validated settings, backend manager and probe. No implicit installation.
use crate::app::App;
pub const LABELS: [&str; 5] = [
    "Add a model folder",
    "Choose/install an inference backend",
    "Probe runtime and available devices",
    "Choose optional coding agent",
    "Set workspace parent directory",
];
pub fn statuses(app: &App) -> [bool; 5] {
    let c = &app.config;
    let path = std::env::var_os("PATH");
    let agent = match c.default_agent.as_str() {
        "omp" => "omp",
        "pi" | "pi-native" => "pi",
        "custom" => &c.custom_agent,
        _ => "",
    };
    [
        c.model_roots.iter().any(|p| p.is_dir()) || c.model_imports.iter().any(|p| p.is_file()),
        crate::doctor::executable(&c.server_binary, path.as_deref()),
        !app.server.devices.is_empty()
            && !app.server.devices.contains("unavailable")
            && (c.device == "none"
                || !crate::models::fit::parse_devices(&app.server.devices).is_empty()),
        crate::doctor::executable(agent, path.as_deref()),
        c.workspace_root.as_ref().is_some_and(|p| p.is_dir()),
    ]
}
