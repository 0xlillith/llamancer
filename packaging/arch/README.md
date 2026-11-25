# Arch Linux packaging

The experimental VCS PKGBUILD builds the public main branch and runs its tests. Review the PKGBUILD, copy it into a clean directory and run makepkg -si as a regular user. The Arch package smoke workflow builds the checked-out revision with makepkg as an unprivileged user, installs it into an isolated x86_64 Arch container, checks the service path, license and icon, and tests terminal restoration. Real desktop installation and aarch64 Arch packaging remain separate checks. The package has not been published to the AUR. A stable package will follow a versioned release.
