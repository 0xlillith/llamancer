# Implementation master plan

Status: proposed, not implemented. Linux-first; OMP-first, not OMP-only.

## M0 — Foundation
- Split library and binary; typed actions/events, reducer, bounded async worker channels, cancellation and graceful terminal restoration.
- XDG config/data/cache, schema-versioned TOML, atomic writes, restrictive credential permissions, structured redacted logs.
- Responsive five-view Dracula TUI, keyboard/mouse, focus, dialogs, help, error states; test with Ratatui TestBackend.
- Commit Cargo.lock, establish fmt/clippy/test CI, verify on Arch and Debian. No performance claims until measured.
Acceptance: clean build, tests, terminal restored on exit/error, no busy polling.

## M1 — Library
- Multiple roots, bounded recursive scan, symlink cycle protection, stable identity, no automatic file moves.
- Bounded GGUF metadata parser; validate magic/version, lengths, multipart completeness and mmproj association; never load weights for indexing.
- Search/filter/sort/favorites, import by reference, explicit physical deletion confirmation; protect loaded files.
- Profiles distinguish engine and request generation parameters; basic/advanced editors, capability validation.
Acceptance: fixture tests for malformed/truncated/multipart GGUF, duplicate paths, read-only roots and safe removal.

## M2 — Server
- Probe installed llama-server version/help/API; maintain compatibility fixtures and capability matrix.
- External attach (read-only lifecycle), managed child, optional systemd user service; ownership and PID identity checks; loopback default.
- Router load/unload/switch where supported, independent server/model states, timeouts, health polling/backoff, bounded logs.
- Backend discovery CPU/Vulkan/ROCm/CUDA, explicit binary selection, no automatic driver installation. Profile translation only for supported flags.
- Test with mock HTTP server and real pinned llama.cpp versions; validate on Andromeda with small GGUF.
Acceptance: start/stop/restart, attach without killing external server, model switch, failure recovery and clean shutdown.

## M3 — Hugging Face and downloads
- Search/pagination, GGUF filters, repository revision and file metadata, license/gated status, multipart and mmproj selection.
- Optional token via secure storage or existing environment; never log or forward auth to unrelated hosts.
- Persistent queue, bounded concurrency, partial files, Range/If-Range resume with validator checks, handle 200/206/416, retries/backoff, disk-space checks.
- Pin revisions, verify size and authoritative digest when available, atomic final rename; sanitize paths and prevent traversal.
Acceptance: interrupted transfer resumes, changed remote object restarts safely, incomplete files never indexed, offline recovery.

## M4 — Agents
- AgentAdapter detect/prepare contract; LaunchContext and LaunchPlan with executable/argv/env/cwd, no shell interpolation.
- Verify actual OMP and Pi provider schemas and versions; preserve user config, preview changes, atomic backups and rollback.
- OMP first end-to-end; Pi next; custom adapter with explicit argv. Select project and current terminal/new tab/window; Kitty first, fallback to current terminal.
- Ensure model is ready before launch; avoid logging prompts or credentials.
Acceptance: real OMP and Pi smoke tests, existing configs preserved, failed launch recoverable.

## M5 — Release
- Accessibility, narrow terminal layouts, keyboard/mouse parity, help, docs, security review, dependency audit.
- Benchmark startup/idle RSS/CPU on defined hardware; targets not guarantees: <100ms startup, <30MB idle RSS, 5–15MB binary.
- Linux x86_64/aarch64 builds, checksums, release notes, Arch PKGBUILD, crates.io/AUR availability recheck before publishing.
Acceptance: reproducible documented installation, clean CI, no critical known defects, end-to-end smoke tests.

## Cross-cutting decisions
Use Rust/Ratatui/Crossterm, Tokio/Reqwest as needed, Serde/TOML, tracing, thiserror. No database initially; atomic versioned files. No telemetry, chat UI, training or detailed hardware monitor. Use btop for monitoring. No mandatory containers. Never expose server publicly by default. Never overwrite existing agent config or delete models silently.

## Dependencies
M0 -> M1 -> M2; M3 can proceed after M1; M4 requires M2; M5 requires all. Keep PRs small, each with tests and documentation. Validate upstream API contracts before committing to exact flags or schemas.

