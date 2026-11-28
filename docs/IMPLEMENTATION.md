# Implementation specification

All project documentation, UI strings, issues, commits and source comments are in English. OMP-first, not OMP-only. Linux-first; no telemetry or mandatory containers.

## Phase 0 — Foundation
Deliver a library/binary split, pure reducer, typed actions, keyboard navigation, responsive five-view layout, contextual help, terminal restoration, event-driven rendering and tests. Next add mouse/focus, bounded asynchronous workers, cancellation, XDG TOML settings, atomic persistence, theme loading and redacted logging. Use stable Rust, commit Cargo.lock and run fmt, clippy and tests. CI requires workflow authorization.
Acceptance: navigation wraps, help is dismissible, quit restores terminal, narrow terminals do not panic, idle does not busy-poll, configuration survives restart.

## Phase 1 — Model library
Define stable model IDs, source roots, canonical paths and metadata. Scan recursively with bounded concurrency, symlink cycle detection and cancellation. Parse GGUF headers with bounded allocations and checked offsets; never read weights. Group multipart shards and associate mmproj explicitly. Track incomplete/invalid models separately. Support import by reference, favorites, search, sorting, details and explicit deletion confirmation; reject deletion of loaded models. Profiles distinguish engine and request settings, with basic/advanced editors. Estimate memory with uncertainty, including KV cache and overhead.
Acceptance: fixtures cover malformed/truncated headers, duplicate paths, multipart gaps, read-only roots and safe removal.

## Phase 2 — Inference
Probe installed llama-server version/help and documented API; pin compatibility fixtures. Implement external attach, owned child process and optional systemd user service. External servers are never stopped implicitly. Bind loopback by default; validate endpoint and redact credentials. Independent server/model state machines, health polling with backoff, timeouts, bounded logs, graceful stop and explicit restart. Discover CPU and AMD Vulkan/ROCm builds without installing drivers. Translate profiles only to supported flags; router load/unload/switch only when supported. Expose web UI and btop launch.
Acceptance: mock HTTP tests and real small-GGUF smoke tests, model switching, failure recovery, external ownership safety. Benchmark AMD backends on Andromeda.

## Phase 3 — Discovery and downloads
HF search with pagination, filters, repository details, revision pinning, quantization and license/gated metadata. Optional authentication via existing environment or secure storage; never forward credentials to unrelated hosts. Persistent queue with bounded concurrency, pause/resume/cancel, retry/backoff, disk-space checks, progress/ETA. Use partial files and Range/If-Range validators; handle 200/206/416 correctly. Sanitize destination paths, verify size and authoritative digest when available, atomically rename complete files. Never index partial files.
Acceptance: interrupted transfer resumes, changed object restarts safely, offline recovery, multipart completion and checksum mismatch tests.

## Phase 4 — Agents
Define AgentAdapter detect/prepare and LaunchContext/LaunchPlan (argv, cwd, env, model, endpoint). Verify upstream OMP and Pi schemas before implementation; OMP first, Pi second, custom argv adapter third. Preview config changes, preserve unrelated keys, atomic backups and rollback. Wait for model readiness. Launch current terminal or Kitty tab/window, fallback for other terminals; no shell interpolation. Avoid logging prompts or secrets.
Acceptance: real OMP/Pi smoke tests, existing configurations preserved, failed launch recoverable.

## Phase 5 — Beta and release
Accessibility, narrow layouts, keyboard/mouse parity, error recovery, dependency audit, security review, documentation and benchmarks. Targets (not measured): startup <100ms, idle RSS <30MB, binary 5–15MB, near-zero idle CPU. Build Linux x86_64/aarch64, checksums, release notes, Arch PKGBUILD; verify crates.io/AUR names before publishing.
Acceptance: clean CI, documented installation, no critical known defects, end-to-end smoke tests.

## Delivery rules
Phase 0 precedes 1; phase 2 and 3 depend on 1; phase 4 depends on 2; release depends on all. Small reviewable commits with tests and docs. No chat, training, RAG or detailed hardware monitoring. No automatic deletion or public server exposure. Exact upstream API contracts remain a validation gate.

