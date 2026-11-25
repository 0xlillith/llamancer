# Integration contracts

Llamancer uses external executables and HTTP; it never embeds llama.cpp or overwrites agent files.

## llama.cpp

Reference: https://github.com/ggml-org/llama.cpp/tree/master/tools/server

Managed single-model sessions translate typed profiles into CLI arguments after probing --help. Managed routers generate an isolated temporary INI with stable model aliases, explicit model paths and per-model settings. Router starts with one active model maximum and explicit loading. External endpoints are inspected via health, props and models endpoints. Load and unload are explicit operations through the router API. Llamancer does not call internal Web UI tool endpoints.

Choose the installed backend build using the server executable setting. Managed sessions require local HTTP and stop on TUI exit. Existing servers can be attached to but are never signalled by the application. Restart an owned router after scanning new files or editing profiles. The server view can probe the version and available devices. The device setting and an explicit projector in the advanced profile are passed only when supported by the executable. The optional headless supervisor runs with llamancer serve and can be started through the provided systemd user unit. The unit is not enabled by default. Optional single-token API authentication uses a private key file. The HTTP client sends a bearer token and managed servers receive a private temporary key-file path. Authenticated agent launches use the foreground terminal, not Kitty remote argv.

## Oh My Pi

References: https://github.com/can1357/oh-my-pi/blob/main/docs/providers.md and https://github.com/can1357/oh-my-pi/blob/main/docs/cli-reference.md

The OMP launch adapter selects the native llama.cpp provider using the documented LLAMA_CPP_BASE_URL environment override and --model provider/model selector. It inherits the user's OMP configuration and uses the configured project directory.

## Pi

Reference: https://docs.pi.dev/latest/llama-cpp

The Pi adapter targets its native router integration. It passes LLAMA_BASE_URL and the model selector. A model must be loaded (or sleeping) before launch. Older versions without the native provider are not supported by this adapter.

## Custom agents

A custom executable and explicit argument array are configured in TOML. Llamancer passes LLAMANCER_ENDPOINT and LLAMANCER_MODEL through the environment, not shell interpolation. Agents can run in the current terminal or use Kitty remote-control launch plans for a tab or OS window; actual graphical launches require a supported terminal environment. Real agent smoke tests remain a release gate.

Pi 1.6.0 on ARM64 has not passed discovery or CLI login validation. Existing stored provider credentials can take precedence over the environment. Do not assume a successful launch proves endpoint selection; verify the active provider and model.
