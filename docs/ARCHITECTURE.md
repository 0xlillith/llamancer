# Architecture

Llamancer is a Linux-first Rust application. It does not link llama.cpp or bundle agent runtimes.

## Boundaries

- The binary owns terminal initialization/restoration, input and foreground child-process handoff.
- Runtime coordinates application state, typed input, persistence and view-independent effects.
- UI modules render immutable application state; Library uses a responsive Ratatui table/inspector.
- Jobs run blocking filesystem or HTTP operations on bounded background workers. Cancellation is cooperative. Idle input blocks rather than polling.
- Models scan existing files without reading tensor payloads. Group identities are based on canonical paths and expected first shards.
- llama modules own HTTP contracts, CLI capability probing, router preset generation and owned child lifecycle. External endpoints are never signalled.
- Hugging Face discovery fetches pinned model file metadata. Downloads retain partial files and publish complete files without overwriting an existing model.
- Agent adapters emit executable/argv/environment/cwd launch plans. No shell interpolation or automatic edits to agent settings.

## Persistence and security

Use XDG configuration, atomic TOML writes and typed validation. No telemetry. Managed inference binds to loopback. Do not embed tokens in endpoints or log them. HF_TOKEN is used only for the exact Hub host. Router presets are temporary, validated and scoped to the owned session. Filesystem and HTTP data are untrusted; bound parsing and network responses. External software compatibility is tested separately.

See IMPLEMENTATION.md for the target specification and STATUS.md for actual progress.
