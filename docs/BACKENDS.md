# Backend Manager

The b12721 catalog pins the exact official release filenames, sizes and SHA-256 digests returned by the GitHub release API for ggml-org/llama.cpp. Archive contents are verified before extraction; traversal, unsupported entry types, duplicate entries, non-sibling symlinks and oversized expanded archives are rejected. Symlinks are created only after extracting regular files, and must resolve to regular files inside staging. No download credentials are used. Staging requires the archive size plus 8 GiB free; cancellation discards temporary files.

Installation is explicit and never replaces an existing version directory. A SHA-256 receipt identifies the verified source archive; it does not continuously rehash every extracted file. Treat managed storage as private and trusted. The selected version is saved as an absolute executable path. Select another backend before removal; Llamancer-owned server processes hold a shared storage lock, while install/remove requires the exclusive lock. Visible process executable checks provide additional protection. Do not run unmanaged executables from these directories, use concurrent alternate XDG roots pointing at the same binaries, or rely on procfs visibility across namespaces. Stop all inference before removal.

## Current desktop evidence

Andromeda, CachyOS x86_64, Radeon RX 9070 XT. Fresh official CPU, Vulkan and ROCm b12721 archives downloaded and installed through the manager. CPU and Vulkan single-model/router authenticated generation passed with SmolLM2-135M-Instruct Q4_0, context 2048; Vulkan advertised the discrete RX 9070 XT. The official ROCm 10.0 archive installed but exposed no devices: libggml-hip.so requires libhipblas.so.3, librocblas.so.5 and libamdhip64.so.7, which are missing on this host. Earlier Unsloth ROCm build results do not establish official archive compatibility. Install compatible distribution ROCm runtime packages separately and re-probe; administrator authorization is required for system package changes.

The real PTY test exercised CPU and Vulkan selection and benchmarks, selected-backend removal refusal, and terminal restoration. Run against already installed CPU/Vulkan builds with a small local GGUF:

```sh
python3 tests/backend_ui_live.py target/release/llamancer /path/to/small.gguf
```

The initial manager deliberately ships a reviewed, pinned catalog, not an auto-updater. Multiple release selection, upstream release discovery and reproducible source builds are follow-up work. No automatic driver changes, system services, PATH modification or model downloads.
