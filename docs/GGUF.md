# GGUF indexing

Reference: https://github.com/ggml-org/ggml/blob/master/docs/gguf.md

The reader supports little-endian v2 and v3, bounded header traversal (512 MiB), metadata and tensor entry caps, checked arithmetic, cancellation, and selective string retention. It extracts name, architecture, file type ID, context length, layer count, tokenizer type, vocabulary count and tensor element count. The latter is a sum of declared tensor dimensions, not a guarantee of unique parameter count. Large arrays are skipped rather than loaded into memory.

Files matching the conventional five-digit shard pattern are grouped within their canonical parent directory. Expected shard numbers must be present and agree with split metadata. This is a conservative presence check, not verification of tensor payload sizes, model compatibility, or shard identity. Projectors are identified provisionally by filename. Invalid or unsupported metadata is skipped. Big-endian files are not supported yet.

Tests use synthetic fixtures. Real model compatibility and payload validation remain separate tasks.
