# Linux beta readiness review

2026-11-25. Reviewed combined development revision 843d62e7. This is a scoped engineering review, not a comprehensive security audit, legal opinion or stable compatibility certification.

## Verified acceptance

- Native Rust 1.88 compatibility, Linux x86_64 and aarch64 CI, isolated Arch packaging, strict Clippy and release build.
- Managed official CPU/Vulkan/ROCm text inference, GPU vision on Vulkan/ROCm, Qwen2.5 and Qwen3 actual KV measurements. Native pinned CPU/Vulkan/ROCm source builds.
- Public and authorized gated Hub downloads, authoritative checksums, resume, quarantine/restore. No credentials, downloaded weights or external inference binaries are distributed.
- OMP and Pi real model-selected read tools, normal graphical exits, synthetic stored configuration precedence. Authenticated OMP explicitly isolates provider and daemon state (#61); authenticated pi-native is refused.
- Actual PTY narrow-modal keyboard/pointer parity, confirmation scroll safety, normal terminal restoration, SIGTERM/SIGHUP/controlling-terminal hangup cleanup. The combined native suite and three owned-router shutdown cases passed.
- Actual graphical Kitty custom-agent windows and tabs, graphical agent foreground handoff and normal exit. High-contrast and narrow-terminal rendering tests. This is not comprehensive accessibility certification.

## Native benchmark

Andromeda (CachyOS x86_64), three fresh empty-library launches of combined revision 843d62e7: binary 6,206,264 bytes; median startup 1.364 ms; median idle RSS 5140 KiB; measured idle CPU 0.0% over the short sampling window. These are host-dependent measurements, not an active inference benchmark.

## Notices and security review

The RustSec dependency audit for #60 (run 45663627247) reported zero known vulnerabilities and no warnings. It covers the final merged lockfile; #61 changes no dependencies. The conservative source-notice collector found notices for 177 of 180 external packages. Traversing the resolved Cargo dependency graph independently for x86_64-unknown-linux-gnu and aarch64-unknown-linux-gnu found 161 external packages per target, all with packaged notices. The three unmatched all-target entries are r-efi 6.0.0 and the Windows-only winapi-i686-pc-windows-gnu and winapi-x86_64-pc-windows-gnu 0.4.0, outside both Linux graphs. Original notice files, including ring, Unicode and CDLA-Permissive materials, are retained. License metadata expressions include permissive dual-license choices; this scoped review does not certify legal compliance in every redistribution context.

The review covers credential scoping, bounded file/network inputs, locked downloads, verified backend archives, path-anchored publication, process shutdown and safe refusal on unsupported quarantine operations. Managed data remains trusted same-user storage; process and filesystem guards are not protection against hostile privileged or same-user code. Agent/project extensions and tools are not sandboxed. Source builds rely on the host toolchain and pinned source, not byte-for-byte reproducibility. Driver and external agent installation remain separate from Llamancer.

## Release boundary

A GitHub beta must include the exact reviewed tag's native archives and checksums, verified after downloading. Registry publication is separate; recheck names before publishing. No model weights, tokens, personal configuration, externally downloaded backends, or agent binaries belong in release artifacts. Keep stable release claims out of the initial beta. Other GPUs, model families, large-memory scenarios, terminal environments and agent versions remain outside the tested matrix.
