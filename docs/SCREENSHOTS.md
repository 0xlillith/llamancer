# Screenshot gallery — model-first workflow

These seven images are rasterized from real Kitty terminal cell captures of the
development build following beta.4. The Andromeda theme and optional Nerd Font
icons are enabled. Public GGUF fixtures, a public Hugging Face repository,
temporary project folders and an isolated loopback inference session were used.
No personal configuration, private project contents, or credentials are included.
The version in the header is the development package version, not a new release.

| Screen | Capture |
| --- | --- |
| Library | ![Library](../assets/screenshots/workflow-library.png) |
| Launch | ![Launch and profile preview](../assets/screenshots/workflow-launch.png) |
| Discover | ![Variant catalog](../assets/screenshots/workflow-discover.png) |
| Downloads | ![Transfer dashboard](../assets/screenshots/workflow-downloads.png) |
| Workspace | ![Project picker](../assets/screenshots/workflow-projects.png) |
| Server | ![Actual inference session](../assets/screenshots/workflow-server.png) |
| Settings | ![Settings](../assets/screenshots/workflow-settings.png) |

Read the [user guide](USAGE.md).

To reproduce, build the binary and run
`python3 tests/capture_workflow.py target/release/llamancer /absolute/path/llama-server /path/to/public-gguf-fixtures /path/to/output`
from a graphical Kitty session with Pillow, Fontconfig and a compatible Nerd Font.
Set optional `LLAMANCER_CAPTURE_HYBRID` to a verified local Qwen3.5-0.8B GGUF
to capture the hybrid auto-context session. The script uses temporary XDG
directories, real Hub search/download, and cleans up the managed server.

---

## Previous captures

# Screenshots and terminal captures

## Beta.4 development interface

These images are rasterized from actual Kitty ANSI terminal buffers of the reviewed beta.4 development code (before the final version bump). Isolated public fixtures and temporary configuration were used.

![Library with direct actions](../assets/screenshots/beta4-library.png)

![Launch panel](../assets/screenshots/beta4-launch.png)

![Context presets](../assets/screenshots/beta4-context.png)

![Advanced launch preferences](../assets/screenshots/beta4-advanced.png)

![Repository variants](../assets/screenshots/beta4-discover.png)

![Variant sort selector](../assets/screenshots/beta4-sort.png)

## Polished Nerd Font interface

These captures show the running application with the optional JetBrainsMono Nerd Font Mono glyphs on CachyOS. The first is a real KDE active-window screenshot; the four remaining images are rasterizations of actual Kitty terminal buffers. All paths in the storage demo are temporary; no private repositories, credentials or model files are included. The portable icon mode also works without Nerd Fonts.

### Andromeda Library

![Polished Andromeda Library and inspector](../assets/screenshots/polished-library.png)

### Catppuccin Mocha

![Polished Catppuccin Mocha](../assets/screenshots/polished-catppuccin.png)

### Backend management

![Backend manager and runtime sidebar](../assets/screenshots/polished-backends.png)

### Public Hub discovery

![Hugging Face discovery and repository sidebar](../assets/screenshots/polished-discover.png)

### Storage planning

![Storage destination and capacity sidebar](../assets/screenshots/polished-storage.png)

## Earlier captures


The original captures below show the running Llamancer v0.1.0 on CachyOS with Kitty and the default Dracula theme. An isolated temporary configuration used public GGUF fixtures and a public Hub repository; no personal configuration or credentials are shown. The Library is a KDE active-window screenshot. The other images are rasterizations of the actual ANSI terminal buffer exported from Kitty, not mockups or independent desktop screenshots.

## Library

Four indexed public GGUF models and the metadata inspector.

![Model library and GGUF inspector](../assets/screenshots/library.png)

## Inference profile

Basic per-model engine and generation options. Advanced mode exposes additional parameters.

![Inference profile editor](../assets/screenshots/profile.png)

## Hugging Face discovery

Live public repository lookup, with nine quantization files and their sizes.

![Hugging Face discovery](../assets/screenshots/discover.png)

## Download queue

A real file entry queued for download in the isolated demo; no download was started.

![Persistent download queue](../assets/screenshots/downloads.png)

## Backend Manager

Real installed CPU, Vulkan and ROCm receipts; Vulkan selected on the test host.

![Backend Manager](../assets/screenshots/backends.png)

## Active inference router

A real managed Vulkan router, with Qwen2.5 loaded and three other indexed models available. The isolated loopback server was shut down after capture.

![Active inference router](../assets/screenshots/server.png)


## Refreshed terminal interface

These six images are rasterizations of actual Kitty ANSI buffers from the updated application on Andromeda. The demo used temporary public model copies and synthetic workspace directories. Benchmark results are actual measurements; CUDA was not measured.

### Andromeda

![Andromeda library](../assets/screenshots/andromeda-library.png)

### Catppuccin Mocha

![Catppuccin Mocha library](../assets/screenshots/catppuccin-library.png)

### Project workspace

![Project selector](../assets/screenshots/project-workspace.png)

### Guided setup

![Guided setup](../assets/screenshots/guided-setup.png)

### Memory advisory

![Device-aware model fit advisory](../assets/screenshots/memory-advisory.png)

### Benchmark comparison

![Actual same-model CPU Vulkan ROCm benchmarks](../assets/screenshots/benchmark-comparison.png)
