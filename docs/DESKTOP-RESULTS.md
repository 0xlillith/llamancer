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
