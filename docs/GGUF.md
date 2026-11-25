# GGUF indexing

Reference: https://github.com/ggml-org/ggml/blob/master/docs/gguf.md

The reader supports little-endian v2 and v3, bounded header traversal (512 MiB), metadata and tensor entry caps, checked arithmetic, cancellation, and selective string retention. It extracts name, architecture, file type ID, context length, layer count, tokenizer type, vocabulary count and tensor element count. The latter is a sum of declared tensor dimensions, not a guarantee of unique parameter count. Large arrays are skipped rather than loaded into memory.

Files matching the conventional five-digit shard pattern are grouped within their canonical parent directory. Expected shard numbers must be present and agree with split metadata. This is a conservative presence and metadata consistency check, not proof of model compatibility or shard identity. The separate tensor extent parser validates known GGML types against the file bounds; unfamiliar types remain explicitly unverified. Projectors are identified provisionally by filename. Invalid or unsupported metadata is skipped. Big-endian files are not supported yet.

Tests cover synthetic malformed/truncated data, known tensor types and actual Llama/SmolVLM fixtures. Actual architecture coverage and GPU allocation remain separate tasks. Weights are approximated from on-disk bytes; KV cache estimates cover conventional supported attention architectures, exclude compute buffers, projector and allocator overhead, and are not a VRAM fit guarantee.
