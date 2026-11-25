# Initial desktop validation

2026-11-25, baseline commit `9459e59`. CachyOS x86_64, KDE Wayland, Linux 7.3.5-2-cachyos, Ryzen 9 9950X3D, Radeon RX 9070 XT, 128 GB system memory. No existing inference service or agent configuration was altered.

## Native build and terminal

Rust 1.100.0: formatting, 60 executed offline unit/integration tests, strict Clippy, release build and terminal restoration passed. Median of three empty-library launches: 1.231 ms startup, 4976 KiB RSS, 0% CPU during the short idle sampling window; binary 5,913,144 bytes.

## AMD inference

Small 91,893,088-byte Llama-family instruction fixture (SHA-256 bcc3af2849ad6095af57e9b5cd43775256efdc66e306acb529172f92d0c04b03), context 2048, eight requested GPU layers, 32 requested generation tokens. Production controller, separate temporary credentials and loopback ports.

| Backend | Device | Single-model generation tok/s | Router generation tok/s | Actual offload |
| --- | --- | ---: | ---: | --- |
| llama.cpp b12683 Vulkan | Vulkan0 | 361.79 | 366.70 | 8/31 layers |
| Unsloth llama.cpp b12651 ROCm | ROCm0 | 305.56 | 302.23 | 8/31 layers |

Independent verbose backend log checks confirmed actual GPU offload. Model, KV and compute buffers on the dedicated GPU were approximately 42.01, 10.50 and 17.75 MiB respectively (ROCm model buffer 42.02 MiB). Global dedicated VRAM increased by approximately 83–85 MiB on Vulkan and 389 MiB on ROCm and returned near baseline after shutdown. These global observations include runtime/driver and desktop effects; they are not per-process allocation figures or predictions for larger models. Authentication, explicit model load/unload and shutdown passed in the harness.

## Agent and service

- OMP 19.1.1: authenticated streaming and controlled read-tool protocol round trip passed. Real authenticated Vulkan model completion passed. Independent model-selected tool use remains open.
- Kitty 0.51.1: actual graphical tab and separate OS-window launch of a custom test agent passed, with the expected project cwd, endpoint and model. Tested in a disposable instance with private socket-only remote control, not by changing normal Kitty configuration. This does not establish interactive OMP/Pi graphical compatibility.
- Temporary real systemd user unit: start, restart, stop, port release passed. No normal service was installed or enabled.

## Remaining checks at the initial baseline (historical)

Larger and additional architectures, GPU vision, multi-context memory measurements, independent real-model coding-tool selection, desktop Pi availability, ordinary interactive UI and agent sessions, authorized private/gated repository fixture, native package installation and final release review. No stable release implied.

## Qwen architecture and memory validation

2026-11-25, official managed llama.cpp b12726 Vulkan, RX 9070 XT, eight GPU layers and two CPU threads. Two checksum-pinned public fixtures were downloaded only into the ignored acceptance-test directory. All eight combinations of single-model/router and 2048/4096 context generated real output with authentication and clean shutdown.

| Fixture | Context | Estimated total KV MiB | Actual CPU KV MiB | Actual GPU KV MiB | GPU model MiB | GPU compute MiB |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Qwen2.5-0.5B-Instruct Q4_K_M | 2048 | 24 | 17 | 7 | 205.04 | 41 |
| Qwen2.5-0.5B-Instruct Q4_K_M | 4096 | 48 | 34 | 14 | 205.04 | 43 |
| Qwen3-0.6B Q4_K_M | 2048 | 224 | 168 | 56 | 186.00 | 32 |
| Qwen3-0.6B Q4_K_M | 4096 | 448 | 336 | 112 | 186.00 | 36 |

Independent verbose backend logs reported 8/25 layers offloaded for Qwen2.5 and 8/29 for Qwen3. Generation throughput ranged approximately 133–139 tok/s on these partial-offload, short-prompt tests. Total KV matches the estimator at both contexts; this does not mean disk weight size predicts VRAM or actual model buffer sizes. CPU-mapped model buffers, driver overhead, compute buffers and other applications remain outside that estimate. Qwen3 model buffers exceeded the disk size in this configuration. The estimate is not a capacity guarantee.

Fixtures:

- Qwen/Qwen2.5-0.5B-Instruct-GGUF, revision 9217f5db79a29953eb74d5343926648285ec7e67, qwen2.5-0.5b-instruct-q4_k_m.gguf, 491400032 bytes, SHA-256 74a4da8c9fdbcd15bd1f6d01d621410d31c6fc00986f5eb687824e7b93d7a9db.
- unsloth/Qwen3-0.6B-GGUF, revision 50968a4468ef4233ed78cd7c3de230dd1d61a56b, Qwen3-0.6B-Q4_K_M.gguf, 396705472 bytes, SHA-256 ac2d97712095a558e31573f62f466a3f9d93990898b0ec79d7c974c1780d524a.

To reproduce, verify the pinned files and use the existing desktop_live harness with each file, Vulkan0, eight GPU layers and each context. Separately inspect verbose llama-server startup logs for the actual per-device model, KV and compute buffers. These fixture downloads and logs are not bundled in releases.

## Backend status after initial managed-archive checks

Managed official b12721 CPU and Vulkan and b12726 Vulkan passed real single/router inference. Official ROCm runtime validation now passed after installing the compatible system libraries; see the follow-up below. The initial graphical custom-agent launch predates the later interactive OMP/Pi and real tool-use evidence below.

## Managed ROCm and gated Hub acceptance

CachyOS, official b12726, ROCm 10.1 distribution runtime, RX 9070 XT, 2048 context, eight GPU layers. All six authenticated single/router cases passed with clean shutdown.

| Fixture | Single tok/s | Router tok/s | Actual offload | CPU/GPU KV MiB | GPU model / compute MiB |
|---|---:|---:|---|---|---|
| SmolLM2 135M | 281.79 | 280.57 | 8/31 | 34.5 / 10.5 | 42.02 / 17.75 |
| Qwen2.5 0.5B | 112.61 | 117.93 | 8/25 | 17 / 7 | 205.05 / 41 |
| Qwen3 0.6B | 115.61 | 110.43 | 8/29 | 168 / 56 | 186 / 32 |

Short-prompt, partial-offload measurements are not peak performance predictions. Actual GPU allocation was confirmed independently using verbose server logs. Both GPUs enumerate with b12721/b12726; the full inference matrix used b12726.

The actual authorized gated download test passed twice for google/gemma-3-1b-it-qat-q4_0-gguf, gemma-3-1b-it-q4_0.gguf (1,003,541,152 bytes), via a private cached fine-grained token. Metadata, authoritative checksum and index validation passed; temporary fixture storage was removed by the test. No credentials or model weights are committed or bundled.

## Desktop agent acceptance follow-up

OMP 19.2.0 and isolated Pi 1.6.0 (Node 22.23.2) passed authenticated streaming, controlled read-tool round trips and real ROCm generation. A 0.6B Qwen3 and a 4B Qwen3 Instruct fixture independently selected and executed the read tool, but the final-answer exact-marker assertion failed in OMP; Pi also failed that assertion on the 4B fixture. A loopback trace confirmed OMP sent the actual tool response in the second chat-completions request. A separate direct llama-server request with tool history and temperature zero reproduced the marker correctly. Thus execution and transport are demonstrated; full end-to-end answer fidelity and ordinary graphical interactive sessions remain open. Do not interpret this as a passed complete agent acceptance test.

## GPU vision and source-build follow-up

The same checksum-pinned SmolVLM-256M and projector used by CPU CI passed actual single-model and router image inference on both Vulkan0 and ROCm0, using official managed b12726, context 4096 and 99 requested GPU layers. All four runs returned "The image is red." See branch test/desktop-gpu-vision for the reproducible opt-in GPU test. Exact vision-tensor placement is not independently measured.

The pinned source helper on branch feature/pinned-source-build fetched official commit 81ff0cab06cf97b445c647d278594c7f97b39740 and built native CPU and gfx1201 ROCm versions on Andromeda. Both passed authenticated single-model/router inference after removal of the intermediate build tree. At this stage Vulkan source compilation was blocked by missing development headers; subsequent [PR #58](https://github.com/0xlillith/llamancer/pull/58) verified a private SDK-prefix build and authenticated single/router inference with Vulkan0, without sudo or global header installation. Managed archive Vulkan inference also remains verified.

Read-only upstream release discovery on branch feature/backend-update-check identified b12728 prerelease CPU/Vulkan/ROCm assets with published SHA-256 digests; no unreviewed archive was installed.

Manual dependency inventory review: 174/177 external packages in the conservative all-target locked graph had discovered source notices. Both Linux target graphs contain 159 packages including Llamancer, and every external package in each has a discovered notice. The three missing all-target notices (r-efi 6.0.0, winapi-i686-pc-windows-gnu 0.4.0 and winapi-x86_64-pc-windows-gnu 0.4.0) are outside both Linux graphs. Ring's original Apache/ISC/BoringSSL and other notices, Unicode data notices and CDLA-Permissive webpki-roots texts are retained by the existing release collector. This is inventory evidence, not final legal certification or a comprehensive security audit.

Agent follow-up: Pi passed one constrained-system-prompt read-tool exact-marker test, but a later randomized fixture failed. Both agents independently executed the tool and transported the actual result. These exact-marker trials were unreliable. Later [PR #53](https://github.com/0xlillith/llamancer/pull/53) passed two independent randomized seven-word read-tool trials per agent, requiring the actual tool result and exact answer; arbitrary model/tool fidelity remains unguaranteed.

## Existing agent configuration and ordinary exit

A separate actual Kitty instance (KDE Wayland, socket-only remote control) launched OMP 19.2.0 and Pi 1.6.0 from the foreground A action of Llamancer. Each used an isolated home with a pre-existing synthetic model default that differed from the selected local model, plus an unrelated preference. The real agent composer showed the selected local model; the stored preference survived, the normal quit path returned to the TUI, and shutdown released the router port. The initial OMP Ctrl-D and slash-quit trials failed because the first-run provider/model wizard intercepted input; OMP's documented bare `quit` in an empty session passed after onboarding was skipped. The fixture disables OMP's onboarding via its own `startup.setupWizard` setting. Pi Ctrl-D passed. No personal settings or real account credentials were read or changed. Agents may update their own stored defaults during normal operation; this is distinct from Llamancer rewriting their configuration.

Reproduce with `python3 tests/agent_kitty_live.py target/release/llamancer /path/to/llama-server /path/to/small.gguf omp` (or `pi`). Requires a real graphical Kitty session and installed agent. Prior real authenticated read-tool tests used Qwen3 4B and randomized ordinary words; arbitrary-model answer fidelity is not guaranteed.

## Authenticated profile isolation and terminal shutdown

[PR #60](https://github.com/0xlillith/llamancer/pull/60) verified owned server cleanup when a real controlling PTY closes and on explicit SIGTERM/SIGHUP. [PR #61](https://github.com/0xlillith/llamancer/pull/61) verified an authenticated OMP graphical session with a stale synthetic stored provider and broker: a real ROCm Qwen3-4B response, normal exit, unchanged stored provider, and no remaining scoped daemon. The private profile is not a sandbox for agent or project code. No personal configuration compatibility is claimed.


## Workspace and backend expansion

The graphical Kitty fixture on Andromeda validated the new workspace selector (active synthetic project persisted), theme cycling across all eight palettes, guided setup, real Vulkan device-memory advisory and the same-model CPU/Vulkan/ROCm benchmark comparison. The original user configuration was not changed.

The Qwen3-4B-Instruct-2507 Q4_K_M fixture (2,497,280,736 bytes) was tested at context 16,384, 99 requested GPU layers and two CPU threads, with official b12726. Four authenticated single-model/router scenarios generated output and released the port on shutdown. Conventional KV estimate: 2,415,919,104 bytes (2.25 GiB). Global dedicated GPU usage after model load was approximately 5.67 GiB on Vulkan and 5.95 GiB on ROCm, including other processes and runtime allocations. These observations do not prove per-process allocation or a universal capacity threshold.

| Backend | Single generation tok/s | Router generation tok/s |
| --- | ---: | ---: |
| Vulkan0 | 175.83 | 175.28 |
| ROCm0 | 139.32 | 138.25 |

The fixed quick benchmark used the 91,893,088-byte public SmolLM2 fixture (SHA-256 bcc3af2849ad6095af57e9b5cd43775256efdc66e306acb529172f92d0c04b03), b12726, PP128/TG32, two threads and two repeats:

| Backend | GPU layers | PP tok/s | TG tok/s |
| --- | ---: | ---: | ---: |
| CPU | 0 | 2648.64 | 437.40 |
| Vulkan0 | 8 | 4477.31 | 430.11 |
| ROCm0 | 8 | 5349.62 | 348.65 |
| CUDA | — | not measured | not measured |

The GUI persisted all three results under the identical model checksum. These small-model partial-offload results are not peak full-GPU comparisons.

The official x86_64 CUDA 12.8 primary and runtime companion archives (814,939,138 combined bytes) installed successfully in isolated storage, passed staged version verification and reported no compatible device on AMD-only Andromeda. No driver installation, NVIDIA inference or aarch64 hardware validation is claimed. The isolated CUDA installation was removed. Broader hybrid architecture and controlled memory-pressure scenarios remain outstanding.


## Additional scoped beta acceptance

On Andromeda, a real public Qwen3.5-0.8B Q4_K_M GGUF (532,517,120 bytes, pinned Hub revision 6ab461498e2023f6e3c1baea90a8f0fe38ab64d0, SHA-256 bd258782e35f7f458f8aced1adc053e6e92e89bc735ba3be89d38a06121dc517) was paused after 4,403,461 bytes and resumed after reopening the persistent queue. The default/root configuration was changed before resume. The original destination was preserved and its SHA-256/ownership receipt verified. A second 1,185,376-byte public fixture downloaded to a separate tmpfs root; both indexed correctly. Missing/read-only roots and insufficient-space preflight were rejected. The offline multipart regression requires every shard before a group becomes complete/loadable; individual files publish separately, not as one filesystem transaction. New queue entries pin directory identity and reject root replacement/redirect on restart.

The same hybrid qwen35 model ran on official b12726 Vulkan0 and ROCm0 with 4,096 context, 99 requested GPU layers and two threads. Authenticated single/router generation and port cleanup passed. KV estimates remained unavailable as intended. Observed single/router generation: Vulkan 348.24/392.27 tok/s; ROCm 225.28/222.08 tok/s. Global dedicated GPU memory increases at load were approximately 0.67 GiB and 0.95 GiB respectively; this includes other allocations and does not prove model/KV/compute placement. The prior 4B/16k conventional-model tests remain separate.

A real CPU Qwen3-4B server was constrained with prlimit to a 1 GiB process virtual address space, producing a genuine mmap allocation failure. The controller observed exit, discarded the session, released its port, accepted a startup cancellation, and recovered with a normal real CPU completion on the same port. No system-wide memory pressure or GPU OOM was induced. Dedicated GPU OOM, larger hybrid architectures and broader hardware remain outside the validated matrix.

Reproduce with the ignored tests in tests/storage_live.rs and tests/memory_pressure_live.rs; run the existing desktop_live test with the downloaded hybrid fixture. No user model or configuration is modified.


## Simplified Library launch acceptance

On Andromeda, the isolated real Kitty test in tests/easy_launch_kitty_live.py verified two Enter presses from Library, readiness of an owned CPU llama-server, a real OMP process with the selected loopback endpoint and temporary project working directory, and navigation in Llamancer while the separate independent Kitty window remained open. Quitting Llamancer released the owned server port. The real public Hub download test exercised the new Run model only action after automatic indexing; no manual Library scan was required. Native tests retain the explicit current-terminal authenticated agent boundary.

## Agent-window lifecycle and context

The isolated real Kitty/OMP test saved a 4096-token global default and a 2048-token model override; the model and agent launched successfully, and closing OMP released the owned single-model listener while Llamancer remained open. An independently started single-model server survived a subsequent agent close and was stopped explicitly. Router automatic release uses the exact owned server generation and model ID; real router close still requires separate acceptance.
