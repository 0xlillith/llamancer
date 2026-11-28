# Backend Manager

The b12721/b12726 catalog pins the exact official release filenames, sizes and SHA-256 digests returned by the GitHub release API for ggml-org/llama.cpp. Archive contents are verified before extraction; traversal, unsupported entry types, duplicate entries, non-sibling symlinks and oversized expanded archives are rejected. Symlinks are created only after extracting regular files, and must resolve to regular files inside staging. No download credentials are used. Staging requires the archive size plus 8 GiB free; cancellation discards temporary files.

Installation is explicit and never replaces an existing version directory. A SHA-256 receipt identifies the verified source archive; it does not continuously rehash every extracted file. Treat managed storage as private and trusted. The selected version is saved as an absolute executable path. Select another backend before removal; Llamancer-owned server processes hold a shared storage lock, while install/remove requires the exclusive lock. Visible process executable checks provide additional protection. Do not run unmanaged executables from these directories, use concurrent alternate XDG roots pointing at the same binaries, or rely on procfs visibility across namespaces. Stop all inference before removal.

## Current desktop evidence

Andromeda, CachyOS x86_64, Radeon RX 9070 XT. Fresh official CPU, Vulkan and ROCm b12721 archives downloaded and installed through the manager. CPU and Vulkan single-model/router authenticated generation passed with SmolLM2-135M-Instruct Q4_0, context 2048; Vulkan advertised the discrete RX 9070 XT. The initial official ROCm archive lacked runtime libraries. After the user installed CachyOS hip-runtime-amd 10.1-2, hipblas 10.1-1 and rocblas 10.1-2, both managed b12721 and b12726 enumerated the discrete and integrated AMD GPUs. Managed b12726 passed six authenticated single/router inference cases on three model families, with independent verbose logs confirming eight GPU layers on ROCm0. System ROCm packages remain external prerequisites; Llamancer does not install drivers.

The real PTY test exercised CPU and Vulkan selection and benchmarks, selected-backend removal refusal, and terminal restoration. Run against already installed CPU/Vulkan builds with a small local GGUF:

```sh
python3 tests/backend_ui_live.py target/release/llamancer /path/to/small.gguf
```

The initial manager deliberately ships a reviewed, pinned catalog, not an auto-updater. Multiple pinned release selection and rollback are supported with v/u or an optional CLI version argument. The c key or backend check command queries eight recent official releases without credentials or redirects, reports newer compatible builds with published SHA-256 metadata, and never changes the reviewed install catalog. Reproducible source builds are separate work. No automatic driver changes, system services, PATH modification or model downloads.

Version management regression: fresh isolated b12721 and b12726 CPU installs, TUI switching and rollback, rejection of removal while a separately configured supervisor was running, and removal after stopping the supervisor passed on Andromeda. Run `python3 tests/backend_versions_live.py target/release/llamancer /path/to/small.gguf`. Both b12726 CPU and Vulkan archives also installed in the desktop managed store; Vulkan device discovery passed.
