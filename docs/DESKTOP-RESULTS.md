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

## Remaining checks

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

## Current backend status

Managed official b12721 CPU and Vulkan and b12726 Vulkan passed real single/router inference. Official ROCm runtime validation now passed after installing the compatible system libraries; see the follow-up below. The graphical custom-agent launch was verified before the user removed the old agents; interactive OMP/Pi and real tool use remain separate acceptance tests.

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
