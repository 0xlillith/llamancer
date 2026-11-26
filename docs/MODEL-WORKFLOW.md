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
sampler and capabilities. A successful synthetic tool probe is not a guarantee.

## Next steps

- Extend reviewed model catalog and cache bounded, validated structured
  metadata for exact Hub revisions when explicitly accessing the repository.
- Use the pinned native `llama-fit-params` tool for architectures beyond the
  current conservative estimate; validate hybrid recurrent and MoE memory on
  CPU, Vulkan and ROCm, without auto-overwriting settings.
- Compare identical revisions, quantizations, templates, request fields and
  effective parameters with Unsloth before attributing tool-call corruption.
- Implement optional Xet provider, content-addressed verified reuse, and
  model-card-based variant recommendation; retain evidence for failed transfers.
- Continue UI visual acceptance with real terminal captures at multiple sizes.
