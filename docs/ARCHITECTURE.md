# Architecture

Linux-first single Rust binary. Ratatui and Crossterm render the UI. Planned Tokio workers and Reqwest HTTP clients isolate blocking work from rendering. UI actions update application state through typed events. Keep model library, downloads, inference and agents separate.

Use llama-server as an external process or user service, not FFI. Bind to loopback by default. The service should survive TUI exit when explicitly managed as a service. Validate router endpoints and flags against installed llama.cpp versions.

Use XDG config/data/cache paths, TOML profiles and atomic writes. Never delete or move existing model files without explicit confirmation. Store credentials separately and redact logs. No telemetry.

