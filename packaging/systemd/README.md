# Optional systemd user service

First configure model roots, the server binary, the endpoint and inference profiles in Llamancer. Select an absolute llama-server executable path if it is not on the systemd user manager's PATH.

The sample unit assumes the executable is installed at ~/.cargo/bin/llamancer. Adjust ExecStart if your installation differs. The Arch packaging draft installs a unit using /usr/bin/llamancer instead.

To install the sample manually:

    mkdir -p ~/.config/systemd/user
    install -m644 packaging/systemd/llamancer.service ~/.config/systemd/user/llamancer.service
    systemctl --user daemon-reload
    systemctl --user start llamancer.service

Start the TUI separately and refresh Server to attach to the configured endpoint. The TUI will not stop a server it did not start. To apply changed profiles or roots, restart the service explicitly.

    systemctl --user restart llamancer.service
    systemctl --user stop llamancer.service

To opt into startup at login, use systemctl --user enable llamancer.service. No service is installed or enabled by Llamancer. The supervisor is tested independently of systemd; the actual user-manager unit must still be validated on a desktop with an active user bus.
