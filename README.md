<div align="center">

<img src="assets/logo.svg" alt="Llamancer — Summon your models." width="840">

[![CI](https://github.com/0xlillith/llamancer/actions/workflows/ci.yml/badge.svg)](https://github.com/0xlillith/llamancer/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-bd93f9.svg)](LICENSE)
[![Status: development](https://img.shields.io/badge/status-development-ffb86c)](docs/IMPLEMENTATION.md)

**A lightweight Rust TUI for llama.cpp model and inference management.**
**OMP-first. Not OMP-only.**

</div>

Llamancer brings local GGUF model discovery, inference preferences, server control, Hugging Face downloads and coding-agent launch into one terminal-native workspace. It does not bundle model weights, install GPU drivers, or rewrite your agent configuration.

## Status

Early development. The core workflows are implemented but are not yet a stable release. Synthetic fixtures, native Linux CI, and a live Hugging Face GGUF download and checksum test are the current validation baseline. Real llama.cpp, OMP and Pi compatibility is still being verified.

| Workspace | Current functionality |
| --- | --- |
| Library | Read-only GGUF indexing, shard grouping, search, ordering, favorites, inspector and per-model profiles |
| Discover | Explicit Hugging Face search, pinned revision/file listing, quantization/shard selection |
| Downloads | Persistent queue, sequential resumable transfers, free-space checks, file locking, size and available SHA-256 validation |
| Server | External attach, managed session, router presets, load/unload, logs, Web UI and btop |
| Settings | XDG TOML preferences, Dracula/custom themes, model roots, agent, server executable, device selection, project directory |

## Quick start

Requirements: Linux, a terminal, a recent stable Rust toolchain to build from source, and a separately installed compatible llama-server for inference. OMP or Pi is optional. Install external software from its official source; Llamancer does not install or upgrade it.

```sh
git clone https://github.com/0xlillith/llamancer.git
cd llamancer
cargo run --locked --release
```

1. Open Settings (5), add an existing absolute model directory, and optionally specify the llama-server executable and agent project directory.
2. Open Library (1), scan with r, inspect a model with Enter and edit its inference profile with p.
3. Press l to launch an owned single-model session, or open Server (4) and press s to start a router using indexed models and generated presets. Press r to attach to an existing server instead.
4. In the Server view, select a router model and press l to load it. Once ready, press a to launch the configured agent in the current terminal. Return to Llamancer by exiting the agent.
5. Press x in Server to stop an owned session. External servers are never stopped by Llamancer.

For Pi, use router mode. OMP uses the documented LLAMA_CPP_BASE_URL runtime override; Pi uses LLAMA_BASE_URL. Existing agent settings remain untouched. The configured custom agent receives LLAMANCER_ENDPOINT and LLAMANCER_MODEL.

## Controls

| View | Keys |
| --- | --- |
| Global | 1–5 or Tab / Shift+Tab, mouse tabs, ? help, q / Ctrl+C quit |
| Library | r scan, x cancel, / search, s sort, f favorite, v favorites only, Enter details, p profile, l load |
| Discover | / search, Enter submit/open repo, Backspace back, d queue file or shard bundle |
| Downloads | r resume, R run queue, x pause, z remove queue entry (keeps files) |
| Server | r refresh, i probe backend, s router, x stop owned, l/u load/unload, a agent, g logs, w Web UI, b btop |
| Profile | Up/Down, Enter edit, a advanced/basic, s save, Esc discard, Ctrl+U clear input |

## Local-first by design

- No telemetry or mandatory accounts. Network access is user-triggered discovery, transfers, or explicitly configured inference endpoints.
- Models remain in place; indexing never reads tensor weights or deletes files.
- Configuration and downloads use versioned or typed TOML beneath the XDG config directory. Custom themes live in its themes subdirectory.
- Managed servers bind to loopback and are session-scoped. They stop when Llamancer exits. External servers remain independent.
- Downloaded files are stored under the first model root, by repository and pinned revision. Partial files use a .part suffix and are not indexed. Set HF_TOKEN in the environment if a repository requires authentication. Llamancer never sends that token to redirect hosts outside huggingface.co.
- Profiles separate engine options from generation defaults. Auto inherits backend defaults; unsupported CLI flags are rejected during launch. Advanced profiles accept an explicit multimodal projector path; Settings accepts a supported device identifier or automatic selection.

## Development

```sh
cargo fmt --all -- --check
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release
```

CI runs on native Linux x86_64 and aarch64. Release builds produce tarballs and SHA-256 files as workflow artifacts; no registry publication or stable release is automatic.

Read [Architecture](docs/ARCHITECTURE.md), [Implementation specification](docs/IMPLEMENTATION.md), [GGUF indexing](docs/GGUF.md), [Integrations](docs/INTEGRATIONS.md), [Roadmap](docs/ROADMAP.md), [Status](docs/STATUS.md) and [Contributing](CONTRIBUTING.md).

Hex, our llama sorcerer, lives in [assets/hex.svg](assets/hex.svg). The default palette is Dracula-inspired.

## License

MIT. Llamancer is an independent project and is not affiliated with llama.cpp, Oh My Pi, Pi or Hugging Face.
