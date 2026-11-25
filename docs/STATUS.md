# Development status

This is a pre-release development snapshot, not a production-ready claim.

| Area | Implemented | Remaining validation or work |
| --- | --- | --- |
| Foundation | Typed navigation, runtime/controller separation, bounded cancellable workers, XDG configuration, custom themes, keyboard/mouse tabs, CI | Full terminal usability and accessibility review; persistent structured diagnostics |
| Library | Metadata indexing, conventional shard grouping, stable identities, search, sorting, favorites, inspector, Basic/Advanced profiles, explicit projector, known tensor extent checks | Broader real-model coverage, automatic projector matching, full tensor type coverage, safe physical removal |
| Inference | CLI capability probing, single model and router sessions, generated presets, external attach, load/unload, bounded logs, device probing and selection, Web UI, btop | GPU/device matrix, actual backend smoke tests, persistent user service, authentication, lifecycle stress testing |
| Hub | Search, revision-pinned GGUF listing, shard bundles, persistent queue, resumable transfer, size and SHA-256 validation, free-space checks, file locking, sequential queue | Pagination, full retry policy, disk-space and concurrency stress testing, gated-repository UX, ETA |
| Agents | Native OMP/Pi launch plans, custom argv, current-terminal handoff, project directory, no config changes | Installed-agent end-to-end tests, Kitty tab/window launch and compatibility matrix |
| Release | Native Linux CI, release artifacts, preliminary Arch package, original vector identity, README | Release benchmarks, tested Arch package, stable tag, registry publishing |

The next release gate is end-to-end validation on a machine with a real GPU, recent llama.cpp, OMP and Pi. Do not mark issues complete solely because an adapter exists.

The live Hub smoke test downloads and indexes a small real GGUF with its published checksum. Offline tests cover the router HTTP contract; GPU inference and installed agent launches are not yet certified.
