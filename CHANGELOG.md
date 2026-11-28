# Changelog

## 0.1.0-beta.6 — 2026-11-28

- Offline recovery of complete SHA-256-pinned staging bundles, requiring both buffered and independent direct reads before any staged part is published.
- Explicit same-inode device-number rebind for recovered staging files after Linux disk enumeration changes; no automatic rebind or overwrite.
- Reject duplicate tool argument keys, mixed Responses SSE call identities/indices and out-of-order argument completion in diagnostic tool probes; accept corroborated output-item completion from llama.cpp when its identity, stream and final arguments all agree.
- Updated canonical GitHub repository references after the account rename.

## 0.1.0-beta.5 — 2026-11-26

- Strict dual-read SHA-256 audits with exact managed-bundle selection via CLI.
- Resume verified Hub cache prefixes without switching transport; retain invalid full-size partials.
- Live acceptance tests for interrupted Hub HTTP transfers, multipart transport parity, and actual reviewed/overridden/reset server sampling parameters.
- Distinguish exact reviewed model-card revisions from repository guidance, support standard Hub snapshots, and add the reviewed Qwen3.5-0.8B default text profile without guessing vision or thinking settings.

- Responsive asynchronous KDE/Zenity workspace picker, cancellation and compact workspace actions, plus populated responsive-layout and PTY regression checks.

- Library and CLI SHA-256 audits with per-pass byte progress, persistent mismatch evidence, fail-closed cancellation and headless router exclusion of suspect models.

- Reviewed model-specific sampler recommendations, effective configuration preview and 128K auto-context target with native hybrid memory fitting.
- Optional official Hugging Face CLI download transport, verified same-digest local reuse and explicit fresh retry preserving failed partials.
- Complete-model SHA-256 audits, independent direct-read checks on Linux when applicable, and persistent launch blocking for failed audits.
- Real tables and transfer gauges, contextual controls, direct text search and graphical workspace-folder selection when available.
- Separate known MTP draft weights from standalone models; do not rank them first or offer unsupported draft attachment.
- Illustrated user guide and reproducible real terminal screenshot gallery.

## 0.1.0-beta.4 — 2026-11-25

- Compact navigation, searchable model variants, direct ranked sort/filter pickers, categorized settings and enumerated setting choices.
- Fold advanced launch preferences; choose context presets or custom token counts, with declared model-context warnings.
- Model running state, model-first download progress, aggregate transfer ETA, server reported context and measured available RAM.

- Shared clickable action bars for Library, Discover, Downloads and Server; responsive launch panel with context and thinking steppers, plus double-click settings editing.

- Opt-in local tool-call preflight (Responses SSE for OMP), exact argument checks and explicit failure confirmation.
- Persistent OMP thinking preference and an explicitly selected revision-specific model-card sampler preset.

- Release inference owned by independent agent windows when the last window closes; retain independently started sessions.
- Expose clickable server stop, unload and disconnect actions.
- Persistent global and per-model context settings directly in the launch menu; apply changed single-model context on relaunch.

## 0.1.0-beta.3 — 2026-11-25

- Two-step model launch from Library with Enter or double click, explicit Run model only and Inspect choices.
- Independent Kitty agent window by default, with readiness-gated automatic OMP launch and explicit cancellation.
- Reuse the same owned single model, replace a different owned single session on confirmation, or load a registered router model.
- Refresh Library automatically after complete model bundles, preserving the selected model.
- Includes all beta.2 improvements; previous beta.2 candidate was not publicly released.

## 0.1.0-beta.2 — 2026-11-25

- Directly editable search fields with automatic Discover focus, click-to-filter and keyboard navigation.
- Real Hub repository ranking, author/license/quantization filters and sortable grouped model variants.
- One download action per model variant, unified multipart queue rows, aggregate progress and model-level resume.
- Automatic download start and Library refresh; per-part verification, transfer history and persistent errors.
- Clearer grouped settings, explicit default model storage, active project label, ASCII welcome art and simplified model execution.
- Preserve full checksum-invalid partials rather than silently redownloading; original large-file checksum discrepancy remains under investigation (#80).
- CUDA remains experimental pending NVIDIA hardware validation (#66).

- Preserve complete checksum-invalid downloads on resume and retain per-entry diagnostic details.

## 0.1.0-beta.1 — 2026-11-25

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
