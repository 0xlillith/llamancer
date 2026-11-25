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

Before a stable release, validate the Arch package, actual systemd user service, AMD Vulkan/ROCm inference, model removal safety, multimodal workflows, gated access with an authorized fixture, and OMP/Pi/Kitty integration on a desktop. Do not publish a stable tag solely because CI passes.
