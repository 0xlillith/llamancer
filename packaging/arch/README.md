# Arch Linux packaging

For Arch or CachyOS x86_64, download the versioned package and `SHA256SUMS` from the [release page](https://github.com/selun3/llamancer/releases/tag/v0.1.0). Verify the downloaded file with `sha256sum -c SHA256SUMS --ignore-missing`, then install the matching non-debug package with `sudo pacman -U ./llamancer-git-VERSION-x86_64.pkg.tar.zst`, using its actual filename.

The VCS PKGBUILD builds the public main branch. To use it, review the recipe, copy it into an empty build directory and run `makepkg -si` as a regular user. GitHub Actions uses the same recipe with the exact tagged revision and tests package installation in a fresh x86_64 Arch container. Aarch64 users can use the Linux release archive. The package is distributed through GitHub Releases.
