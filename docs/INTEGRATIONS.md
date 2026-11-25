# Integration contracts

Llamancer uses external executables and HTTP; it never embeds llama.cpp or overwrites agent files.

## llama.cpp

Reference: https://github.com/ggml-org/llama.cpp/tree/master/tools/server

Managed single-model sessions translate typed profiles into CLI arguments after probing --help. Managed routers generate an isolated temporary INI with stable model aliases, explicit model paths and per-model settings. Router starts with one active model maximum and explicit loading. External endpoints are inspected via health, props and models endpoints. Load and unload are explicit operations through the router API. Llamancer does not call internal Web UI tool endpoints.

Choose the installed backend build using the server executable setting. Managed sessions require local HTTP and stop on TUI exit. Existing servers can be attached to but are never signalled by the application. Restart an owned router after scanning new files or editing profiles. Backend device selection, authentication and long-lived service supervision are follow-up items.

## Oh My Pi

References: https://github.com/can1357/oh-my-pi/blob/main/docs/providers.md and https://github.com/can1357/oh-my-pi/blob/main/docs/cli-reference.md

The OMP launch adapter selects the native llama.cpp provider using the documented LLAMA_CPP_BASE_URL environment override and --model provider/model selector. It inherits the user's OMP configuration and uses the configured project directory.

## Pi

Reference: https://docs.pi.dev/latest/llama-cpp

The Pi adapter targets its native router integration. It passes LLAMA_BASE_URL and the model selector. A model must be loaded (or sleeping) before launch. Older versions without the native provider are not supported by this adapter.

## Custom agents

A custom executable and explicit argument array are configured in TOML. Llamancer passes LLAMANCER_ENDPOINT and LLAMANCER_MODEL through the environment, not shell interpolation. All agents currently run in the foreground terminal. Real agent smoke tests remain a release gate.
