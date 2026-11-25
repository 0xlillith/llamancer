# Contributing

Open an issue before large changes. Keep modules focused, run cargo test and cargo clippy, and document new behavior. Never commit credentials or model weights.


## Continuous integration

CI runs formatting, tests, strict Clippy and release builds on native Linux x86_64 and aarch64 runners for pushes to main and pull requests. Dependencies are resolved from the committed Cargo.lock.

The Release builds workflow can be triggered manually or by a version tag. It produces architecture-specific tarballs and SHA-256 checksums as workflow artifacts. It does not publish a GitHub Release or registry package automatically.
