# Changelog

## Unreleased

- Initial project identity, architecture and TUI navigation scaffold.

- Added versioned XDG configuration, atomic persistence, custom TOML themes, mouse tab navigation and keyboard shortcuts.

- Added cancellable background jobs, an interactive settings editor and read-only GGUF candidate discovery.

- Added bounded GGUF metadata and tensor descriptor parsing, multipart presence checks, and a model details view.

- Added library search, size/name ordering, persistent favorites and readable quantization labels.

- Added per-model inference profiles, Basic/Advanced editing, transactional persistence and parameter validation.

- Implemented the initial inference, Hub download and coding-agent workflows; refactored runtime and UI, added original Hex branding and experimental Arch packaging.

- Hardened partial downloads with exclusive locks and free-space checks, added sequential queue execution, backend/device probing, explicit projector paths, and known GGUF tensor extent validation.

- Added real CPU router integration coverage, optional foreground supervisor and systemd unit, paginated Hub discovery, bounded retries, transfer rates and ETA, and configurable Kitty agent launch plans.

- Added exclusive TUI locking, bounded local diagnostics and ordered download shutdown; single-file imports, invalid-model reporting, hide/restore, KV-cache estimates; private-file API authentication and external endpoint backoff; improved range and checksum recovery.

- Updated the terminal stack and checked Rust 1.88 support; added repeatable release benchmarks, RustSec and dependency license inventory workflows.
- Hardened descriptor-anchored download publication and owned process-group cleanup; added interruption, external reconnect and multipart recovery tests.
- Added recoverable managed-file quarantine with ownership receipts, checksums, Linux write leases and explicit restore.
- Added mouse row selection, textual selection markers and a high-contrast theme.
- Validated real systemd user-service start/restart/stop, isolated Arch x86_64 package installation, installed OMP/Pi authenticated streaming and read-tool round trips, and real single-model/router CPU image inference on both native architectures.

- Added checksum-pinned official CPU, Vulkan and ROCm backend installation, device probing, selection, removal and bounded small-model benchmarks in the TUI. Added CLI installation/probe commands and actual managed CPU installation in live CI. GPU runtime requirements remain external.
- Added side-by-side pinned b12721/b12726 upgrades and rollback with explicit version selection and a shared execution/exclusive mutation lock.
- Added upstream release discovery, pinned source-build helper, gated Hub credential validation, actual GPU vision, real agent tool selection and graphical handoff, scoped authenticated OMP profiles and daemon cleanup, bounded local state, narrow modal navigation, and owned-router shutdown on terminal hangup.

- Added an offline, redacted `llamancer doctor` setup report.

- Added workspace parent directories and an active-project picker for agent launches.

- Added six additional built-in palettes and a more expressive terminal header, numbered tabs and highlighted selections.

- Added per-bundle download destination selection and disk-space preflight.

- Added explicit model memory advisory with actual backend-reported device capacity.

- Added experimental managed NVIDIA CUDA archives and separately verified runtime companions.

- Added guided setup and bounded comparable benchmark history.

- Optional auto/nerd/plain icon modes, responsive contextual sidebars, polished theme-aware visual chrome and expandable model inspector.

- Pin new queued model destination directory identities and reject replaced/redirected roots. Add opt-in multi-root/hybrid and bounded memory-failure acceptance tests.
