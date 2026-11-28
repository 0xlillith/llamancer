# Release validation

The CI workflow runs Rust formatting, all offline tests, strict Clippy, release builds and PTY restoration on native Linux x86_64 and aarch64 runners.

The minimum supported Rust version is 1.88 and is checked separately. Ratatui uses Crossterm and layout-cache; Crossterm uses the Unix poll event source to handle terminal EOF.

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

The first beta has documented acceptance for the actual CachyOS desktop, managed AMD Vulkan/ROCm, Qwen architecture and measured KV overhead, authorized gated access, real model-selected OMP/Pi tools and graphical Kitty integration. See the [current release status](STATUS.md) for the Andromeda validation of v0.1.0 and the [beta review](BETA-REVIEW.md) for earlier evidence.

Use the [desktop acceptance runbook](DESKTOP-ACCEPTANCE.md) to execute the remaining gates. Distribution archives include original third-party Cargo notices and a conservative all-target inventory; missing or incomplete notices still require review before publishing.

Binary archives and Arch packages include the README, linked documentation, mascot assets, sample systemd unit, Arch recipe, and optional source-build helper when present. The release workflow validates links before archiving and again after checksum verification and extraction; the Arch smoke workflow validates the installed documentation. These checks use only the checked-out source revision.
