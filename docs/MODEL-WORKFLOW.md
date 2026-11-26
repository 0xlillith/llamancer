# Model-first workflow

## Independent research and design decisions

- [Unsloth inference config](https://github.com/unslothai/unsloth/blob/main/studio/backend/utils/inference/inference_config.py)
  and [model defaults](https://github.com/unslothai/unsloth/blob/main/studio/backend/utils/models/model_config.py):
  use a reviewed, mode-specific defaults catalog instead of one global sampler.
  Do not copy AGPL implementation into the MIT codebase.
- [Ollama context length](https://github.com/ollama/ollama/blob/main/docs/context-length.mdx):
  long coding sessions need at least 64K; Llamancer aims for 128K where supported.
- [llama.cpp server](https://github.com/ggml-org/llama.cpp/blob/b12726/tools/server/README.md):
  Jinja templates, device fit, runtime props and router query routing.
  [Pinned common.cpp](https://github.com/ggml-org/llama.cpp/blob/b12726/common/common.cpp)
  consumes embedded sampling metadata when flags are not explicit.
- [Hugging Face generation config](https://huggingface.co/docs/transformers/main_classes/text_generation):
  prefer typed generation metadata over scraping free-text model cards at runtime.
  GGUF converters can embed these values.
- [Hub downloads](https://huggingface.co/docs/huggingface_hub/main/guides/download):
  Xet chunk deduplication is worth supporting as an optional provider; preserve
  the existing lightweight native HTTP path.
- [Ratatui widgets](https://ratatui.rs/examples/widgets/): real tables, gauge,
  sparkline, shared hit geometry, clear state colors, restrained labels.

## Resolution hierarchy

Manual per-model preferences > manual global context > reviewed mode-specific
sampling defaults > embedded GGUF metadata interpreted by llama.cpp > backend
defaults. No arbitrary remote command, template override, or model-card code
is executed. A known repository card is not proof that every future revision
supports the same tool protocol. At runtime, consult `/props` for actual context,
sampler and capabilities. A successful synthetic tool probe is not a guarantee. The reviewed catalog recognizes managed and standard Hub snapshot layouts, shows exact-revision versus repository-level guidance, and refuses mixed-root shards. The reviewed Qwen3.5-0.8B snapshot uses only the documented default non-thinking text settings; task-dependent vision and explicit thinking settings are not guessed.

## Implemented in beta.5

- Conventional attention estimation, plus bounded native `llama-fit-params`
  probing for supported hybrid models: the 128K coding-context target is
  capped by declared context and memory. Real Qwen3.5-0.8B inference at
  131072 tokens passed with CPU, Vulkan and ROCm.
- Optional official `hf` download transport, preserving the native default;
  verified local reuse and independent destination hashing. Hugging Face
  Hub 2.x shared-cache blobs and legacy layouts are supported. Interrupted HTTP Hub cache retention and verified destination-prefix resume have dedicated live tests; full-size invalid partials are retained.
- Explicit complete-model integrity audits and launch blocking on failures.
- Known MTP draft weights are auxiliary, not standalone launch candidates.
- Real terminal capture gallery and illustrated [user guide](USAGE.md).

## Next steps

- Extend reviewed model catalog and cache bounded, validated structured
  metadata for exact Hub revisions when explicitly accessing the repository.
- Extend measured memory validation to larger hybrid and MoE models, additional backends and explicit offload settings.
- Compare identical revisions, quantizations, templates, request fields and
  effective parameters with Unsloth before attributing tool-call corruption.
- Expand acceptance to very large interrupted multipart/Xet transfers and model-card-based variant recommendations; add speculative draft attachment only after compatibility checks.
- Continue UI visual acceptance with real terminal captures at multiple sizes.
