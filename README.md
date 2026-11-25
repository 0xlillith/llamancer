# Llamancer

![Llamancer](assets/logo.svg)

**Summon your models.**

A lightweight Rust TUI for llama.cpp model and inference management.

**OMP-first. Not OMP-only.**

Llamancer is a terminal-native control center for local LLM inference, designed around Oh My Pi (OMP), with planned native Pi support and an extensible coding agent architecture.

## Status

Early development. The initial TUI is a navigation scaffold; inference, downloads and agent integrations are not yet implemented.

## Planned features

- GGUF library discovery, import and model profiles.
- Hugging Face search and resumable downloads.
- llama-server lifecycle, router model loading and unloading.
- Vulkan, ROCm and CPU configuration.
- First-class Oh My Pi integration, Pi and custom agent adapters.
- Dracula theme, keyboard and mouse navigation.
- Local-first operation, no telemetry, no Electron or required containers.

## Build

Install stable Rust and run `cargo run` on Linux. Press Tab to switch views and q to quit.

## Documentation

[Architecture](docs/ARCHITECTURE.md) · [Design](docs/DESIGN.md) · [Roadmap](docs/ROADMAP.md) · [Integrations](docs/INTEGRATIONS.md)

MIT licensed. Contributions welcome.


## Configuration (early development)

On first launch, Llamancer creates `$XDG_CONFIG_HOME/llamancer/config.toml` (or `~/.config/llamancer/config.toml`). The file contains schema version 1, the theme, model roots, default agent and endpoint. Model roots can be scanned from Library; the agent and endpoint settings are reserved for upcoming integrations. Dracula is built in. Set `theme` to a custom name and place a matching TOML file under the configuration directory's `themes/` subdirectory; see `themes/dracula.toml` for an example. Use Tab, arrow keys, 1–5 or click a tab to navigate; press ? for help, q or Ctrl+C to exit.

Development checks: `cargo fmt --all -- --check`, `cargo test --locked`, `cargo clippy --locked --all-targets -- -D warnings`.

## Local model discovery preview

Open Settings (5), select Add model root, enter an existing absolute directory and save with Enter. Switch to Library (1) and press r to scan or x to cancel. The read-only scan parses bounded little-endian GGUF v2/v3 metadata and tensor descriptors without reading tensor payloads. It groups conventional multipart names and checks shard numbering against split metadata. Press Enter for model details and Esc to close them. Shard presence and readable metadata do not establish loadability or payload integrity. Projector recognition is a filename heuristic; pairing is not yet implemented. Nested symlinks are skipped. No model files are modified.

### Library navigation

Press / to search by name, path, architecture or quantization (all search terms must match). Enter or Esc leaves search mode; Ctrl+U clears the query while editing. Press s to cycle name/size ordering, f to toggle a favorite and v to display favorites only. Favorites persist in the configuration file. A multipart group's identity is its expected first shard path, even when that shard is missing.

Quantization labels use the GGUF general.file_type / llama_ftype identifiers, not individual tensor type IDs. Unknown identifiers remain numeric.

### Inference profiles

Select a complete model group in Library and press p. The editor starts in Basic mode; press a to switch to Advanced. Use Up/Down to choose a parameter and Enter to edit it. Enter applies the value to the draft, not to disk. Press s to save the full profile or Esc to discard it. An empty value or auto inherits the eventual backend default; zero GPU layers explicitly requests CPU-only model placement.

Profiles are stored by model identity in the versioned TOML configuration. Engine parameters and generation defaults are independent. Validation catches invalid numeric ranges, non-finite values, unrecognized cache types and inconsistent batch sizes. The editor does not yet apply settings to llama-server or verify backend-specific capabilities.
