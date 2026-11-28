# Development status

The current Linux beta is v0.1.0-beta.6, not a production-ready claim.

| Area | Implemented and validated | Outstanding release evidence |
| --- | --- | --- |
| Foundation | Native CI, bounded local state, high-contrast theme, actual mouse/keyboard modal tests, narrow confirmations and shutdown restoration | Broader terminal, assistive technology and theme testing |
| Library | GGUF and shard inspection, profiles, search/import/hide, recoverable quarantine, Qwen2.5/Qwen3 measured KV validation | Qwen3.5 hybrid text inference and bounded CPU memory failure/recovery verified; broader hybrid architectures and GPU OOM remain |
| Inference | Managed CPU/Vulkan/ROCm; experimental CUDA archive installation; official and pinned source builds, desktop text and GPU vision, lifecycle/authentication, systemd and terminal hangup cleanup | Actual NVIDIA inference; larger model/device combinations |
| Hub | Public and authorized gated checksum/index verification, real multi-root pause/resume, pinned directory identity, scoped credential discovery, quarantine and restore | Broader private-repository configurations |
| Agents | Real OMP/Pi read-tool execution, graphical normal exit, configured-default precedence, scoped authenticated OMP profile/daemon, custom Kitty launch | Broader versions, arbitrary project extensions and model-specific fidelity |
| Release | Native x86_64/aarch64, Rust 1.88, Arch packaging, checksum archives and current RustSec audit | Broader hardware and distribution compatibility; registry publication |

## Beta.6 scope

Offline recovery validates every pinned complete staged shard using independent buffered and direct reads before publication, with explicit same-inode device-number rebind. Tool preflight rejects duplicate JSON keys, mixed SSE identities and out-of-order completions; it accepts corroborated output-item completion from llama.cpp without claiming full protocol conformance, and remains diagnostic rather than a guarantee.

The original pinned DavidAU IQ3_S bundle (83,617,879,456 bytes) was successfully recovered on the desktop on 2026-11-28 after both buffered and direct SHA-256 matched the authoritative hashes for both shards. The new recovery command recorded ownership receipts, with no redownload or overwrite. A bounded 8192-context ROCm b12726 server reproduced the original synthetic Chat Completions test with valid arguments in both recommended thinking and original non-thinking greedy modes. The initial Responses diagnostic isolated a missing argument-specific completion event: the function output-item completion, streamed bytes and final arguments agreed. The parser now tests this corroborated format. This is not a claim of universal model reliability or a hardware stability certification.

## Beta.5 scope

Model-first workflow PRs #110–#115 cover reviewed sampling provenance, optional official Hub transport, pinned multipart downloads, fail-closed integrity audits and native workspace dialogs. The new model-workflow acceptance suite checks real small-model server parameters and interrupted Hub transfers on independent hosted runners. Flash Next large-model inference and host data-integrity investigation remain separate issues (#80 and #100); NVIDIA #66 remains experimental.

## Evidence

- [PR #16](https://github.com/selun3/llamancer/pull/16): Ratatui/Crossterm migration; the subsequent RustSec audit reported zero known vulnerabilities and warnings. This is not a complete security audit.
- [PR #17](https://github.com/selun3/llamancer/pull/17) and [PR #24](https://github.com/selun3/llamancer/pull/24): installed Pi 1.6.0 and OMP 19.2.0 exercised against controlled loopback HTTP responses, including authenticated streaming and actual read-tool execution. These fixtures validate the agent protocol, not a model's independent coding decisions. Real authenticated OMP and Pi CPU text generation was separately demonstrated. OMP 19.1.1 also passed real authenticated Vulkan generation on the desktop; graphical custom-agent tab/window launches passed. These earlier results were superseded by the real desktop tool and normal-exit validation in #53, #59 and #61.
- [PR #20](https://github.com/selun3/llamancer/pull/20): a temporary systemd user unit started, restarted and stopped a real CPU router on ARM64; the temporary unit was removed.
- [PR #21](https://github.com/selun3/llamancer/pull/21): native and live CPU/Hub validation on both architectures, including download, ownership receipt, quarantine, restart reconciliation, restore and resume. Quarantine retains bytes and refuses unsupported filesystems and unverifiable files.
- [PR #22](https://github.com/selun3/llamancer/pull/22): isolated Arch x86_64 makepkg build, package installation and terminal smoke.
- [PR #25](https://github.com/selun3/llamancer/pull/25): checksum-pinned SmolVLM-256M Q8_0 and projector, llama.cpp b12721, single-model and router CPU image inference on both native architectures. All four runs correctly described the test image as red.

Follow [open issues](https://github.com/selun3/llamancer/issues) and [release validation](RELEASE-VALIDATION.md) for remaining gates. Do not infer general GPU or real-world model compatibility from a passing CPU fixture.

See [desktop validation evidence](DESKTOP-RESULTS.md) for measured AMD backend, native CachyOS, systemd and Kitty results on the initial desktop hardware.

The [beta review](BETA-REVIEW.md) records the current combined review and explicitly distinguishes verified scope from limitations.

The beta.3 Library launch menu uses a separate Kitty window for keyless agent sessions and waits for inference readiness. See [desktop results](DESKTOP-RESULTS.md). CUDA hardware validation and the original large-file checksum discrepancy remain open.
