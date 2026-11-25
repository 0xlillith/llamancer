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

