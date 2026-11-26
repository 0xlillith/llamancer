# Desktop acceptance and release handoff

This is a contributor runbook, not a requirement for ordinary users. Run it on a Linux desktop with real GPU hardware (initial target: Andromeda, CachyOS, KDE Wayland and Kitty). Do not publish a stable release merely because CPU CI passes.

## 1. Establish a clean baseline

Use a fresh clone or a clean worktree. Do not overwrite an existing development checkout or install/update drivers automatically.

```bash
git clone https://github.com/selun3/llamancer.git
cd llamancer
git rev-parse HEAD
rustc --version
cargo fmt --all -- --check
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release
python3 tests/terminal_smoke.py target/release/llamancer
mkdir -p acceptance-evidence
python3 tests/benchmark.py target/release/llamancer > acceptance-evidence/startup.json
```

Record the commit, distribution, kernel, CPU/GPU model, driver/runtime versions and terminal version. Do not upload environment dumps, shell histories, paths containing personal data, tokens, prompts or original agent configurations. Review and redact evidence before publishing.

## 2. Backend and model matrix (#2, #3)

Use already installed, separately verified llama.cpp builds. Do not replace an existing server or expose a LAN port. Start with a small GGUF that fits comfortably in memory; avoid stress tests with a large model until ordinary inference is verified.

```bash
/path/to/vulkan/llama-server --version
/path/to/vulkan/llama-server --list-devices
/path/to/rocm/llama-server --version
/path/to/rocm/llama-server --list-devices
```

Record the exact advertised device ID (not a guessed name). Select a small instruction-tuned GGUF, preferably a conventional Llama/Qwen model. Run one backend at a time. Check active memory with btop or the GPU driver tools.

```bash
# CPU reference, then rerun with each actual GPU build and device ID.
LLAMANCER_TEST_SERVER=/absolute/path/to/llama-server \
LLAMANCER_TEST_MODEL=/absolute/path/to/small-model.gguf \
LLAMANCER_TEST_DEVICE=none \
LLAMANCER_TEST_GPU_LAYERS=0 \
LLAMANCER_TEST_CONTEXT=2048 \
cargo test --locked --test desktop_live -- --ignored --nocapture

# Example parameters only: replace device and number of layers with safe values.
LLAMANCER_TEST_SERVER=/absolute/path/to/vulkan/llama-server \
LLAMANCER_TEST_MODEL=/absolute/path/to/small-model.gguf \
LLAMANCER_TEST_DEVICE=VULKAN_DEVICE_ID \
LLAMANCER_TEST_GPU_LAYERS=8 \
LLAMANCER_TEST_CONTEXT=2048 \
cargo test --locked --test desktop_live -- --ignored --nocapture
```

Repeat with the ROCm build, its exact advertised device ID, and the same model, context, GPU layer count and prompt. The harness uses the production controller, a temporary bearer key and an ephemeral loopback port. It validates single-model and router generation and shutdown; it reports timings, approximate weights/KV bytes and global AMD VRAM observations. It does **not** by itself certify hardware offload: verify the backend's actual offloaded layers and GPU utilization independently. Global VRAM includes the desktop and other processes; it is not per-process usage. Compare measured memory to estimates at multiple contexts and record the difference, including compute buffers, allocator overhead, cache padding and partial offload. Do not promise a precise fit.

| Scenario | Evidence required |
| --- | --- |
| CPU, Vulkan, ROCm | Exact build/device, requested and actual offload, success/failure, load time, prompt and generation tokens/s, RAM/VRAM |
| Single model, router | Load/unload, switch between two models, restart, exit, port release, no orphan workers |
| Model families | Llama, Qwen2/Qwen3; additional hybrid/MoE/other real architectures as available; no unsupported architecture estimate |
| Vision | Known matching projector, actual image result in single and router modes; repeat with GPU if supported |
| Failure | Missing executable, unsupported device/flag, occupied port, invalid GGUF, insufficient memory handled without data loss |
| External server | Attach and detach; exit must not stop it; verify authentication and failed-key recovery |

A missing backend, driver or suitable model is a blocked test, not a pass. Do not deliberately force an out-of-memory event on the user's active desktop.

## 3. TUI and Kitty (#5, #6)

Use a disposable XDG config and a temporary project containing only a harmless fixture file. Do not copy secrets or personal configurations. In a fresh terminal:

```bash
scratch=$(mktemp -d)
mkdir -p "$scratch/config" "$scratch/project"
printf 'llamancer-desktop-fixture\n' > "$scratch/project/fixture.txt"
XDG_CONFIG_HOME="$scratch/config" target/release/llamancer
```

Use Settings to select the backend, an existing model directory (read-only indexing), a free loopback port, and the disposable project. Do not use quarantine or downloads against existing model storage: use a separate temporary root for those checks. For authenticated testing, generate a disposable mode-0600 key file with a random local token, select it in Settings, and never paste the value into issues or screenshots.

- [ ] All five views at normal and narrow terminal sizes, resize during a scan, keyboard and mouse navigation, selection markers and high-contrast mode.
- [ ] Search, favorites, profile editing, invalid-file display, import by reference, hide/restore and restart persistence.
- [ ] Library/Downloads quarantine and restore using a newly downloaded disposable fixture. Cancel/confirm, reject open/mapped files and preserve existing files.
- [ ] Real desktop model load, model switch, restart, logs, Web UI, btop and graceful TUI exit.
- [ ] OMP and Pi current-terminal launch with the selected local model and disposable project. First run the installed-agent protocol fixtures below.
- [ ] Real model produces a text response, then independently selects the read tool to read fixture.txt and returns the exact marker. A mock protocol test is not proof of model tool selection. Restrict tools to read-only for this check.
- [ ] Repeat normal launches with existing desktop agent configuration only after confirming it cannot route prompts or credentials to an unintended remote endpoint. Inspect the selected provider/model and loopback traffic, not stored secrets. Do not rewrite upstream configuration.
- [ ] Unauthenticated Kitty tab/window launch using already authorized remote control; correct cwd, selected model, return to Llamancer. If remote control is disabled, record a blocked test instead of enabling it globally. Authenticated launch must be refused outside the foreground terminal.
- [ ] Current-terminal fallback (A), terminal attributes restored after agent exit/failure, no credentials in Kitty argv or diagnostic output.

```bash
python3 tests/omp_streaming.py "$(command -v omp)" --read-tool
python3 tests/pi_streaming.py "$(command -v pi)" --read-tool
```

These fixtures use isolated HOME directories and authenticated controlled loopback responses; real interactive agent tests are separate. Version incompatibilities must be recorded and fixed in a dedicated PR.

## 4. Authorized Hub access (#4)

Choose a small private/gated GGUF for which the account has already been granted read access. Accept upstream terms yourself. Use a local fine-grained read token or existing cached credentials; never put a token in a shell command, test log, issue, CI secret or repository file. The test downloads into a temporary directory and bounds the file size by default (100 MB). Review the chosen repository and file before running.

```bash
LLAMANCER_TEST_HF_REPO=owner/authorized-repo \
LLAMANCER_TEST_HF_FILE=small-fixture.gguf \
cargo test --locked --test hub_live authorized_repository -- --ignored --nocapture
```

Verify denial without authorization in a separate disposable credential context (metadata visibility alone does not prove access), then verify correct authorized download, revision pinning, checksum and indexing. Never request gated access or change repository permissions automatically. If there is no authorized small fixture, leave #4 open.

## 5. Packaging and release (#6)

- [ ] Build the Arch package in an isolated clean directory using makepkg as an ordinary user; inspect the package before installation. Do not replace the current package or enable a service without approval.
- [ ] Run the real systemd user-unit test using the installed binary, a compatible server and a tiny model: `python3 tests/systemd_smoke.py /absolute/path/to/llamancer /absolute/path/to/llama-server /absolute/path/to/tiny.gguf`. The unique temporary service is removed after testing.
- [ ] Review dependency audit, third-party notice inventory and actual license files, including dual-license choices, certificate data and Unicode notices. Review source/model distribution terms separately. Models and external executables are not included in release archives.
- [ ] Run final CI, live CPU+vision workflow, dependency audit, Arch package smoke and release artifact workflow on the exact release candidate commit. Verify SHA-256 for both architecture archives and inspect contents. Investigate performance target deviations; don't quietly change thresholds.
- [ ] Confirm issues #2–#5 are complete or explicitly document deferred, non-blocking scope with maintainer approval; finish #6 and recheck crates.io/AUR name availability.
- [ ] Review version, changelog, installation docs, compatibility limits and release notes. Create a tagged prerelease for final testing only if appropriate; publish a stable tag/registry package only after explicit release approval.

## Evidence and issue workflow

Record PASS / FAIL / BLOCKED / NOT RUN for each scenario, with commit, build, model family/quantization, relevant measurements and a redacted log. Use a new issue for each reproducible defect, link its small PR, and merge only after applicable checks pass. Update the corresponding phase checklist with actual evidence; do not close phases based on a planned test. Keep all project material in English.
