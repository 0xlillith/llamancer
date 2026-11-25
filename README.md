<div align="center">

<img src="assets/hex-mascot.webp" alt="Hex, the Llamancer mascot, wearing a purple wizard hat and holding a cyan flame staff." width="256">

# Llamancer

*Summon your models.*

[![CI](https://github.com/0xlillith/llamancer/actions/workflows/ci.yml/badge.svg)](https://github.com/0xlillith/llamancer/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-bd93f9.svg)](LICENSE)
[![Beta: 0.1.0-beta.3](https://img.shields.io/badge/beta-0.1.0--beta.3-bd93f9)](https://github.com/0xlillith/llamancer/releases/tag/v0.1.0-beta.3)

**A lightweight Rust TUI for llama.cpp model and inference management.**
**OMP-first. Not OMP-only.**

</div>

Llamancer brings local GGUF model discovery, inference preferences, server control, Hugging Face downloads and coding-agent launch into one terminal-native workspace. It does not bundle model weights, install GPU drivers, or rewrite your agent configuration.

## Status

Linux beta 0.1.0-beta.3. Native Linux x86_64 and aarch64 CI, managed CPU text/vision inference, checksum-verified public and authorized gated Hub downloads, systemd lifecycle, Arch package installation, AMD Vulkan/ROCm text and vision inference, pinned CPU/Vulkan/ROCm source builds, real OMP/Pi tool use and graphical agent exit have passed their documented acceptance tests. Authenticated OMP uses an isolated temporary profile. No general model compatibility, comprehensive security audit or stable-release certification is implied. See [verified status](docs/STATUS.md), [desktop results](docs/DESKTOP-RESULTS.md) and [release review](docs/BETA-REVIEW.md).

| Workspace | Current functionality |
| --- | --- |
| Library | Read-only GGUF indexing (including Hugging Face cache snapshot links), shard grouping, search, favorites, inspector, inference profiles, import, hide/restore and recoverable managed-file quarantine |
| Discover | Direct search, repository rankings, author/license/quantization filters, pinned revisions and complete variant selection |
| Downloads | Unified multipart model queue, automatic sequential resumable transfers, bounded retry, throughput and ETA, explicit per-bundle storage, free-space checks, pinned destination identity, file locking and integrity validation |
| Server | External attach, managed session, headless router supervisor, router presets, load/unload, logs, Web UI and btop |
| Settings | XDG TOML preferences, Eight built-in/custom themes, optional Nerd Font icons, model roots, agent, backend/device, workspace root and active project |

## Screenshots

![Llamancer in Kitty with optional Nerd Font icons and Andromeda theme](assets/screenshots/polished-library.png)

[More screenshots and terminal captures](docs/SCREENSHOTS.md): themes, project workspace, guided setup, memory advisory, benchmark comparison, inference profiles, discovery, downloads and the active router.

## Quick start

Requirements: Linux, a terminal, Rust 1.88 or newer to build from source, and a compatible llama-server for inference (installed through Backend Manager or separately). OMP or Pi is optional. Install external software from its official source; Llamancer does not install or upgrade it.

```sh
git clone https://github.com/0xlillith/llamancer.git
cd llamancer
cargo run --locked --release
```

1. Press G for guided setup; confirm a model directory and use B to select a backend.
2. Open Discover (2), type a query and press Enter. Open a repository, select a model variant and confirm its download destination once. All shards download automatically and the Library refreshes when complete. For existing models, open Library (1), inspect with Enter and edit the inference profile with p.
3. Select a complete model in Library and press Enter twice (or double-click, then click **Run model + open coding agent**). Llamancer starts inference, waits for readiness and opens the configured agent in an independent Kitty window. Choose **Run model only** for inference without an agent.
4. For advanced router operation, use Server (4). Press Enter on a ready server model to open the agent window. Existing shortcuts remain available.
5. Press x in Server to stop an owned session. External servers are never stopped by Llamancer.

Keyless OMP launches use the documented LLAMA_CPP_BASE_URL runtime override; check for conflicting native provider settings before using it. Authenticated OMP launches use a temporary private agent profile and daemon directory, with an explicit selected provider. Personal OMP settings are not read into this profile or rewritten. The temporary profile and its daemon are cleaned up on exit; do not use this mode for persistent agent history or background jobs. Authenticated Pi launches require the scoped pi adapter rather than pi-native. Pi uses a Llamancer-owned session extension with the selected endpoint and model; choose pi-native for the upstream router provider (LLAMA_BASE_URL). Existing agent settings remain untouched. The compatibility extension supports text only and requires an advertised or configured context size. The configured custom agent receives LLAMANCER_ENDPOINT and LLAMANCER_MODEL.

## Installation packages

Beta packages are available for Linux x86_64 and aarch64 on the [GitHub release page](https://github.com/0xlillith/llamancer/releases/tag/v0.1.0-beta.3). Download the matching archive and its SHA-256 file, then run:

```sh
# Choose x86_64 or aarch64 to match uname -m
arch=$(uname -m)
sha256sum -c "llamancer-linux-${arch}.tar.gz.sha256"
tar -xzf "llamancer-linux-${arch}.tar.gz"
mkdir -p ~/.local/bin
install -m755 "llamancer-linux-${arch}/llamancer" ~/.local/bin/llamancer
~/.local/bin/llamancer --version
```

Keep the extracted documentation and third-party notices. The release tag identifies the source revision. Add ~/.local/bin to PATH if necessary. Archives contain Llamancer, not model weights or GPU runtimes.

For Arch/CachyOS, download the x86_64 package and SHA256SUMS from the release page, verify with `sha256sum -c SHA256SUMS --ignore-missing`, then install the matching non-debug package with `sudo pacman -U ./llamancer-git-VERSION-x86_64.pkg.tar.zst` (replace the placeholder with the actual filename). Alternatively build from the [VCS package recipe](packaging/arch/README.md). Not yet published to AUR or crates.io.

Run `llamancer doctor`, then `llamancer` and press G for guided setup. On Linux distributions without compatible system libraries, build from source instead.

## Model fit advisory
Press `m` in Library to see the selected model's disk size, conventional attention KV estimate, requested GPU offload and available system RAM. Probe the configured backend with `i` in Server first to include its last reported per-device memory. Full-offload/CPU comparisons flag likely capacity pressure; partial/automatic offload and unsupported hybrid architectures remain explicitly uncertain. This is not a guarantee and does not change settings automatically.

## Discover models
Discover opens with the search field focused: type immediately and press Enter. Click the search/filter field or press F3 (Ctrl+F or / also works) to focus it again. Tab/Esc returns focus to results; mouse tabs and rows remain active. Double-click or press Enter on a repository to open it, then on a variant to confirm storage and start downloading. Press `s` to cycle real Hugging Face rankings: most downloaded, trending, newest, recently updated and most liked. Search supports `author:unsloth`, `license:mit`, `quant:Q4_K_M` and exact `@owner/repo`. Inside a repository, `/` filters variant names and `s` sorts balanced Q4/Q5 first, smallest, largest or alphabetically. Multipart variants are grouped into one row with aggregate size and completeness. Confirm once to queue every shard of the selected quantization. Downloads shows one model job with combined progress; Enter/r resumes all unfinished parts of that model, skipping completed parts. Per-part checksums and ownership receipts remain separate. Optional mmproj projectors are separate downloads; unrelated quantizations are never mixed. Balanced ordering is a filename heuristic, not measured per-variant popularity; the Hub does not supply file download counts through this API.

## Everyday workflow
Use G for guided setup, B to choose a backend, Discover to select a variant, then confirm its storage. Downloads start automatically, and the Library refreshes on completion. Select a complete model in Library, press Enter, then Enter again for model + agent in a separate window. The alternate Run model only and Inspect actions are available in the same menu. Context for this model and Default context (all models) are editable directly in the menu: arrow left/right cycles Auto, 8K, 16K, 32K, 64K, 128K and 256K; Enter accepts a custom token count. Changes persist immediately. Auto on a model inherits the global preference, otherwise the backend default. Relaunching the same owned single model with changed context restarts it; router presets require a router restart. High context can exceed available memory and the model training context; estimates are advisory. If no executable is configured the Backend Manager opens first. Server provides `w` Web UI, `a` Launch agent and `x` Stop. Settings groups storage, inference, agent, workspace and appearance, and editing shows a contextual hint. The first model-folder editor offers `default` (creates the per-user XDG data model directory only on confirmation), or an existing absolute path of your choice. Defaults keep the local loopback endpoint, automatic device and independent Kitty window. Existing configurations are preserved.

## Model storage
Add multiple model roots in Settings and optionally choose a Default download root. In Discover, `d` uses that default (or opens the picker when multiple roots exist without a default), and `D` always opens the destination picker. Newly added downloads start automatically and continue through queued files; paused and failed entries require explicit resume. Completed downloads trigger a Library rescan. Recent transfer speed history and byte counts appear in Downloads. The picker shows total bundle size and free space per root. Queue entries retain their original destination across restart and default changes. New entries also pin the directory device/inode identity; a replaced directory, symlink or changed mount identity is refused. Restore the original mount/path before resuming. Legacy entries lack the recorded identity, but redirected symlink roots are refused. New bundles require space for the full file(s); resume of already queued files still checks the actual remaining bytes. A full checksum-invalid partial is retained on repeated resume; move it aside manually before deliberately starting a fresh transfer. The last failure is recorded per queue entry.

## Project workspaces
Set **Workspace parent directory** in Settings to a directory containing your projects (for example `/home/user/src`). Press `P` in Server or Settings to select an immediate project folder. The header shows ACTIVE PROJECT, not the workspace root. The selected project becomes the working directory for OMP, Pi or custom agents. Hidden folders and symlink entries are excluded; discovery is bounded. You can also set an explicit Active project directory independently. If a workspace is configured but no project is selected, agent launch asks you to select one. Existing agent settings are untouched.

## Guided setup and benchmark comparison
Press uppercase `G` in any normal view for a resumable setup checklist: model storage, reviewed backend, device probe, optional agent and workspace root. Enter opens the appropriate existing setting or manager; Esc skips setup and G reopens it. Nothing is installed implicitly.

In Backend Manager, run the quick benchmark with `b`, then `h` to compare saved results for the most recently measured model SHA-256. Only the latest comparable run per backend is shown (PP128/TG32, two threads, two repeats, zero CPU or eight GPU layers). The bounded local history stores hashes, measured throughput and backend/device labels, not model contents or paths. Missing results remain unmeasured.

## Troubleshooting

Run `llamancer doctor` for an offline local setup report. It checks configured executables, model paths, optional agent/terminal tools and managed backend receipts without starting processes, making network requests, or printing paths, endpoint addresses or tokens. Use Backend Manager or `llamancer backend probe KIND` to check actual runtime device compatibility.

## Managed inference backends

Llamancer can install official llama.cpp distributions itself. No Unsloth, package manager, sudo, shell configuration or service is required. In **Server** or **Settings**, press `B` to open Backend Manager:

| Action | Key |
|---|---|
| Select CPU, Vulkan, ROCm or CUDA | Up / Down |
| Browse newer/older reviewed release (upgrade/rollback) | v / u |
| Check recent upstream releases (read-only) | c |
| Download and install the pinned official build | i, then y |
| Probe the backend and list real devices | p |
| Select the installed backend for inference | Enter |
| Benchmark a selected small local model | b, then y |
| Remove an unused managed backend | d, then y |
| Cancel a running operation / return | x / Esc |

The reviewed catalog pins llama.cpp **b12721 and b12726**, with SHA-256 and archive sizes obtained from official GitHub release metadata. CPU and Vulkan are available on x86_64 and aarch64; ROCm is available on x86_64. Experimental managed CUDA pairs the official backend with its separately checksum-pinned runtime companion (CUDA 12.8 x86_64, CUDA 13.5 aarch64); an appropriate NVIDIA driver is still required. GPU inference on NVIDIA has not yet been hardware-validated. Installations live under `$XDG_DATA_HOME/llamancer/backends` (default `~/.local/share/llamancer/backends`). Downloads are verified before extraction and published as private versioned directories. GPU selection refuses backends that expose no compatible device. Vulkan, ROCm and CUDA still require compatible system GPU runtimes/drivers; no drivers are installed automatically.

Select a complete single-file GGUF of at most 512 MiB in Library before benchmarking. The quick test uses 128 prompt tokens, 32 generation tokens, two threads, two repetitions and eight GPU layers (zero for CPU). Stop all inference first. Confirm the selected GPU device in Settings, particularly on multi-GPU systems. Do not start unmanaged programs from Llamancer-owned backend directories during removal. External executable paths remain supported. Versions install side-by-side; select an installed version explicitly to switch or roll back. The reviewed catalog is updated through Llamancer releases; The read-only upstream check reports compatible newer builds with published SHA-256 metadata; unreviewed builds are not installable. Source compilation is a separate workflow.

Headless installation and diagnostics:

```sh
llamancer backend list
llamancer backend check
llamancer backend install vulkan
llamancer backend probe vulkan
llamancer backend install vulkan b12726
llamancer backend probe vulkan b12726
```

See [Backend Manager](docs/BACKENDS.md) for verification, platform requirements and current limits. For optional host-native compilation, see [pinned source builds](docs/SOURCE-BUILDS.md).

## Controls

| View | Keys |
| --- | --- |
| Global | 1–5 or Tab / Shift+Tab, mouse tabs and rows, G setup, ? help, q / Ctrl+C quit |
| Library | r scan, x cancel, / search, s sort, f favorite, v favorites only, Enter launch menu, j details, PgUp/PgDn scroll, p profile, m memory advisory, l load, i invalid files, h hide, t quarantine |
| Discover | / search, Enter submit/open repo, Backspace back, n next page, d queue file or shard bundle, D choose destination |
| Downloads | r resume, R run queue, x pause, z remove queue entry (keeps files), t quarantine, T restore |
| Server | r refresh, D detach, i probe backend, s router, x stop owned, l/u load/unload, a agent, A current terminal, g logs, w Web UI, b btop, B backends, P project |
| Settings | T theme, I icons, B backends, P project |
| Profile | Up/Down, Enter edit, a advanced/basic, s save, Esc discard, Ctrl+U clear input |

## Themes
The default Dracula theme is joined by **Andromeda** (pastel violet, pink and cyan on a deep navy background), Catppuccin Mocha, Tokyo Night, Nord, Rosé Pine, Gruvbox Dark and High Contrast. Press uppercase `T` in Settings to cycle built-ins instantly, or edit the Theme field by name. Custom TOML themes remain supported. The header shows the active agent/project and theme; selected rows retain a textual marker and colored emphasis. Nerd Fonts are optional: in Settings, press uppercase `I` to cycle `auto → nerd → plain`, or edit the Icons field. Auto checks a compatible local graphical terminal and optional Fontconfig glyph coverage once at startup, then uses portable Unicode symbols if the result is uncertain (including SSH and terminal multiplexers). The terminal's configured font ultimately determines glyph rendering; force `nerd` when appropriate, or `plain` for a portable fallback. No font is downloaded, no terminal configuration is changed, and no new runtime dependency is required.

## Responsive interface
The TUI uses accent-colored navigation, model and project status chips, contextual shortcut bars, an expandable model inspector, and adaptive side panels for discovery, transfers, backend management, preferences and guided setup. Wide terminals show extra context; narrower terminals retain the main list. Press Enter in Library for the full inspector and PageUp/PageDown to scroll. Live activity indicators run only while work is active; the idle event loop remains non-redrawing.

## Terminal accessibility

Click a tab or a visible list row to select it; use the mouse wheel or Up/Down to move the selection. Actions retain their contextual keyboard shortcuts. Selected entries have a textual marker as well as a color highlight. Choose the built-in `high-contrast` theme in Settings for increased contrast.

## Local-first by design

- No telemetry or mandatory accounts. Network access is user-triggered discovery, transfers, or explicitly configured inference endpoints.
- Models remain in place; indexing never reads tensor weights or deletes files.
- Configuration and downloads use versioned or typed TOML beneath the XDG config directory. Custom themes live in its themes subdirectory.
- Managed servers bind to loopback and are session-scoped. They stop when Llamancer exits. External servers remain independent.
- Downloaded files are stored under the chosen model root, by repository and pinned revision. Partial files use a .part suffix and are not indexed. Set HF_TOKEN in the environment if a repository requires authentication. Llamancer never sends that token to redirect hosts outside huggingface.co.
- Profiles separate engine options from generation defaults. Auto inherits backend defaults; unsupported CLI flags are rejected during launch. Advanced profiles accept an explicit multimodal projector path; Settings accepts a supported device identifier or automatic selection.

## Hub discovery and private repositories

In Discover, use a query such as `qwen license:apache-2.0 quant:Q4_K_M`, or `@owner/repository quant:Q4_K_M` to open an exact repository (including a private one). Quantization filtering is filename-based; projector candidates remain visible. For gated repositories, accept the license/access conditions on Hugging Face first. Llamancer reads HF_TOKEN or the standard local Hub token file (HF_TOKEN_PATH, HF_HOME, XDG_CACHE_HOME, or the default cache). Credentials are sent only to the exact HTTPS Hub origin, never to CDN redirects. If no authoritative SHA-256 is provided, a partial download restarts instead of trusting a previous prefix; a complete file can be checked only by its declared size. No credentials are stored in the download queue.

Cached Hugging Face credentials must be a regular, owner-only file (mode 0600), not a symlink or hard link. If needed, use `chmod 600 ~/.cache/huggingface/token`. Explicit `HF_TOKEN` is supported; `HF_HUB_DISABLE_IMPLICIT_TOKEN=1` disables automatic cached-token discovery. Credentials are never added to the download queue or sent outside the Hub origin.

## Library and endpoint safety

- Import a single GGUF by absolute path in Settings without copying weights; matching multipart siblings are indexed automatically. The Library inspector estimates weight and KV-cache sizes for conventional supported attention architectures. This is not a VRAM fit guarantee.
- Press i to inspect invalid GGUF paths, or h to hide a selected model after confirmation. Hidden models are excluded from future indexing and managed router presets. Restore by exact identity path in Settings. Files are never deleted by this operation.
- For authenticated inference, create a private file (mode 0600) containing one API token and set its absolute path in Settings. The token is not written to TOML or command-line arguments. Remote authenticated endpoints require HTTPS; managed servers bind to loopback. Use A for authenticated agent launches in the current terminal; Kitty remote launch is deliberately unavailable for these credentials.
- One TUI at a time may write a configuration directory. Local bounded diagnostics.jsonl records event codes and counters, not prompts, paths or credentials.

### Recoverable model quarantine

Press `t` in Library or Downloads to quarantine a complete managed model bundle, and `T` in Downloads to restore it. Confirm explicitly after stopping all inference servers. Only files newly published by Llamancer with an authoritative SHA-256 and persisted inode receipt are eligible; imported files, legacy queue entries, changed files and files without a checksum are refused. Linux write leases and readable same-user procfs mapping checks reject in-use files; local mapping inspection and a write lease are mandatory. Managed files must remain owner-only. Other processes may be hidden by OS access controls, and this is not protection from privileged processes, hidden PID namespaces or hostile same-user mutation. Stop all inference before confirming. The configured local endpoint must be idle. Llamancer cannot detect unloaded paths registered in arbitrary external servers, so stop them before confirming.

Quarantine renames the files in place with a `.llamancer-quarantine` suffix; no bytes are deleted and no disk space is reclaimed. Restoring never overwrites an existing path. If interrupted during a multipart move, queue state is reconciled on restart and the same operation or restore can resume. Ownership records are retained. Do not manually rename or replace quarantined files unless recovering outside Llamancer.

## Persistent router (optional)

Run `llamancer serve` to supervise a router using your saved model roots and profiles without a terminal UI. Stop it with Ctrl+C when running interactively. A sample systemd user unit is included in [packaging/systemd](packaging/systemd/README.md); installation and enablement are explicit user actions. The TUI can attach to that endpoint with r. Restart the supervisor when changing model roots, profiles or device settings.

## Agent terminal modes

Select new-window (default), current, kitty-tab or kitty-window in Settings. Library launch and Server Enter use an independent Kitty process without remote control; Llamancer stays responsive. The a shortcut uses the configured mode, while A explicitly uses the current terminal. Legacy kitty-tab/kitty-window modes require existing authorized Kitty remote control and kitten. Authenticated agent launches remain current-terminal only; no secrets in terminal command-line arguments. Existing configurations are not rewritten. Closing the last independent agent window releases inference created solely for that agent (stops a single-model server or unloads its newly loaded router model). Independently started inference and external servers are retained. Closing Llamancer stops its owned inference server. Server has clickable Stop server, Disconnect and router Unload model actions.

## Development

The [desktop acceptance runbook](docs/DESKTOP-ACCEPTANCE.md) describes the remaining GPU, agent, terminal, authorized Hub and release checks, with isolated test commands and evidence requirements.

```sh
cargo fmt --all -- --check
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release
python3 tests/terminal_smoke.py target/release/llamancer
```

CI runs on native Linux x86_64 and aarch64. The optional Live CPU smoke tests workflow checks a pinned upstream llama-server and a tiny checksum-verified GGUF fixture, plus live Hub download/resume and managed quarantine/restore. Its optional multimodal input runs real single-model and router image inference with a checksum-pinned SmolVLM model and projector. The Arch package workflow builds and installs the VCS package in an isolated Arch x86_64 container. Release builds produce tarballs, third-party notice inventories and SHA-256 files as workflow artifacts; no registry publication or stable release is automatic.

Read [Architecture](docs/ARCHITECTURE.md), [Implementation specification](docs/IMPLEMENTATION.md), [GGUF indexing](docs/GGUF.md), [Integrations](docs/INTEGRATIONS.md), [Roadmap](docs/ROADMAP.md), [Status](docs/STATUS.md), [Release validation](docs/RELEASE-VALIDATION.md) and [Contributing](CONTRIBUTING.md).

Meet [Hex](assets/hex-mascot.webp), our llama sorcerer and project mascot. The default palette is Dracula-inspired.

## License

MIT. Llamancer is an independent project and is not affiliated with llama.cpp, Oh My Pi, Pi or Hugging Face.

Local TOML settings and download queues are bounded to 16 MiB; custom themes to 64 KiB. Use regular user-owned state files that are not writable by other users. Final-component symlinks and special files are rejected to prevent startup hangs or unintended reads.
