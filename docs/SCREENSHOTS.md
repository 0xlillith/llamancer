# Screenshots and terminal captures

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
