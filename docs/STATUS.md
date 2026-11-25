# Development status

This is a pre-release snapshot, not a production-ready claim.

| Area | Implemented and validated | Outstanding release evidence |
| --- | --- | --- |
| Foundation | Typed navigation, runtime/controller separation, bounded cancellable workers, XDG configuration, custom and high-contrast themes, keyboard and mouse navigation, PTY restoration, exclusive TUI lock, bounded diagnostics | Desktop usability and accessibility review |
| Library | GGUF metadata and tensor extent validation, shard grouping, stable identities, search, favorites, profiles, import and hide/restore, KV-cache estimates, explicit projector selection, ownership-verified recoverable quarantine | GPU vision and larger/hybrid architectures |
| Inference | Owned and external single-model/router sessions, device probing, authentication, process-group cleanup, reconnect/backoff, real CPU text and vision generation on both architectures, real systemd user-service lifecycle on ARM64 | GPU vision, large-model and memory-pressure behavior; official ROCm and Vulkan desktop inference verified |
| Hub | Search, pinned revisions, shard bundles, persistent resumable queue, integrity validation, descriptor-anchored publication, interruption recovery, real public download and quarantine/restore on both architectures | Authorized gated Gemma download and checksum/index validation passed; broader private-repo scenarios optional |
| Agents | OMP/Pi/custom launch plans, current-terminal handoff, Kitty launch plans, project directory, no upstream configuration changes; installed OMP and Pi streaming and read-tool protocol smoke tests | Real tool execution and desktop Pi generation demonstrated; exact tool-result fidelity, stored configuration precedence and interactive OMP/Pi launches remain open |
| Release | Native x86_64/aarch64 CI, Rust 1.88 MSRV, benchmark artifacts, release tarballs with checksums, RustSec audit and license inventory, isolated Arch x86_64 package build/install, Hex HiDPI mascot | Manual license/security review, final desktop usability/acceptance evidence, tag and registry publishing |

## Evidence

- [PR #16](https://github.com/0xlillith/llamancer/pull/16): Ratatui/Crossterm migration; the subsequent RustSec audit reported zero known vulnerabilities and warnings. This is not a complete security audit.
- [PR #17](https://github.com/0xlillith/llamancer/pull/17) and [PR #24](https://github.com/0xlillith/llamancer/pull/24): installed Pi 1.6.0 and OMP 19.2.0 exercised against controlled loopback HTTP responses, including authenticated streaming and actual read-tool execution. These fixtures validate the agent protocol, not a model's independent coding decisions. Real authenticated OMP and Pi CPU text generation was separately demonstrated. OMP 19.1.1 also passed real authenticated Vulkan generation on the desktop; graphical custom-agent tab/window launches passed. Interactive desktop Pi and independent real-model tool selection remain release tests.
- [PR #20](https://github.com/0xlillith/llamancer/pull/20): a temporary systemd user unit started, restarted and stopped a real CPU router on ARM64; the temporary unit was removed.
- [PR #21](https://github.com/0xlillith/llamancer/pull/21): native and live CPU/Hub validation on both architectures, including download, ownership receipt, quarantine, restart reconciliation, restore and resume. Quarantine retains bytes and refuses unsupported filesystems and unverifiable files.
- [PR #22](https://github.com/0xlillith/llamancer/pull/22): isolated Arch x86_64 makepkg build, package installation and terminal smoke.
- [PR #25](https://github.com/0xlillith/llamancer/pull/25): checksum-pinned SmolVLM-256M Q8_0 and projector, llama.cpp b12721, single-model and router CPU image inference on both native architectures. All four runs correctly described the test image as red.

Follow [open issues](https://github.com/0xlillith/llamancer/issues) and [release validation](RELEASE-VALIDATION.md) for remaining gates. Do not infer general GPU or real-world model compatibility from a passing CPU fixture.

See [desktop validation evidence](DESKTOP-RESULTS.md) for measured AMD backend, native CachyOS, systemd and Kitty results on the initial desktop hardware.
