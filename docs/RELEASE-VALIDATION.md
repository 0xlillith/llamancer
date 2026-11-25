# Release validation

The CI workflow runs Rust formatting, all offline tests, strict Clippy, release builds and PTY restoration on native Linux x86_64 and aarch64 runners.

The minimum supported Rust version is 1.88 and is checked separately. The terminal stack uses only its Crossterm and layout-cache features.

Each native release-build CI job also uploads a benchmark JSON with executable size, three fresh empty-library launches, median time to first rendered header, idle RSS and process CPU ticks over a two-second interval. These are host-dependent observations, not a GPU or active-server benchmark. Release targets remain startup below 100 ms, idle RSS below 30 MiB, near-zero idle CPU, and a 5–15 MB executable. Investigate deviations rather than silently removing the targets.

Run the measurements locally:

```sh
cargo build --locked --release
python3 tests/benchmark.py target/release/llamancer
python3 tests/license_inventory.py > licenses.json
cargo audit
```

The Dependency audit workflow inventories license metadata and uses RustSec cargo-audit against Cargo.lock. The resulting JSON artifacts require review; inventory alone is not a license compliance conclusion.

The completed CPU-side gates include isolated Arch x86_64 package installation, an actual ARM64 systemd user-service lifecycle, recoverable quarantine safety tests, and real single-model/router multimodal CPU inference. Installed-agent protocol tests cover authenticated streaming and a read-tool round trip using controlled responses. See [development status](STATUS.md) for the linked evidence and limits.

Before a stable release, validate the actual Arch desktop, AMD Vulkan/ROCm inference, additional model architectures and measured memory overhead, gated access with an authorized fixture, real-model OMP/Pi tool selection and graphical Kitty integration. Review the license inventory and remaining security assumptions manually. Do not publish a stable tag solely because CI passes.
