# Optional pinned source builds

Official binary archives remain the recommended Backend Manager path. For users who want host-native compilation, the repository includes an explicit helper targeting the reviewed llama.cpp b12726 source commit 81ff0cab06cf97b445c647d278594c7f97b39740.

Requirements: Python 3.11+, Git, CMake, Ninja, C/C++ compiler, disk space and appropriate system GPU development packages. Vulkan requires development headers and a shader compiler; ROCm requires the HIP development toolchain at /opt/rocm and an explicit ISA target. No packages, drivers, services or PATH entries are installed or modified.

Run from the repository:

```sh
python3 scripts/build-backend.py cpu --jobs 8
python3 scripts/build-backend.py vulkan --jobs 8
python3 scripts/build-backend.py rocm --gpu-target gfx1201 --jobs 8
```

The helper fetches a fixed upstream commit (not a moving branch or unchecked tag) into a private staging directory, checks its identity, builds llama-server and llama-bench, copies the generated binaries and libraries, removes the build tree, and probes the standalone executable. The output is a separate versioned directory under `$XDG_DATA_HOME/llamancer/source-builds` (default `~/.local/share/llamancer/source-builds`). Existing builds are never overwritten; a lock serializes source builds. The original license and build metadata are retained. The host-native configuration makes the recipe repeatable, not bit-for-bit reproducible across toolchains or hosts.

On success the helper prints the absolute executable path. Select it in Settings as an external llama-server executable, then select the exact device. Source builds are not managed binary-archive receipts: install/remove/version browsing in the Backend Manager still applies to reviewed archive installations only. Stop all inference before manually removing a source build. Source compilation is not yet wired into the TUI.

## Andromeda verification

CachyOS, Ryzen 9 9950X3D, AMD Radeon RX 9070 XT, ROCm 10.1. Actual pinned CPU and gfx1201 ROCm builds completed from clean staging using 16 build jobs. Both ran standalone after the build directories were deleted, and both passed the production-controller authenticated single-model/router generation and clean shutdown test using the pinned SmolLM2-135M fixture at context 2048. CPU used zero GPU layers; ROCm requested eight layers. ROCm enumerated ROCm0 (discrete RX 9070 XT) and ROCm1 (integrated GPU). Vulkan source compilation remains untested on this host because the development headers are missing; the official managed Vulkan archive is separately verified.

These short-prompt tests validate the build and inference path, not peak throughput, every upstream architecture, or cross-host reproducibility.
