#!/usr/bin/env python3
"""Verify distribution README links and required offline reference files."""
import argparse
from pathlib import Path
import re
from urllib.parse import unquote, urlsplit

def verify(root: Path):
    root = root.resolve(strict=True)
    required = [
        "README.md", "LICENSE", "CONTRIBUTING.md", "CHANGELOG.md",
        "docs/BACKENDS.md", "docs/STATUS.md", "docs/RELEASE-VALIDATION.md",
        "docs/DESKTOP-ACCEPTANCE.md", "assets/hex-mascot.webp",
        "packaging/systemd/README.md", "packaging/systemd/llamancer.service",
        "packaging/arch/README.md", "packaging/arch/PKGBUILD",
    ]
    for name in required:
        assert (root / name).is_file(), f"Missing distribution document: {name}"
    readme = (root / "README.md").read_text()
    for raw in re.findall(r"\]\(([^)\s]+)\)|\bsrc=\"([^\"]+)\"", readme):
        link = urlsplit(next(x for x in raw if x))
        if link.scheme or link.netloc or not link.path:
            continue
        target = (root / unquote(link.path)).resolve()
        assert target.is_relative_to(root), f"Local link escapes distribution: {link.path}"
        assert target.exists(), f"Broken distribution README link: {link.path}"
    if (root / "docs/SOURCE-BUILDS.md").exists():
        assert (root / "scripts/build-backend.py").is_file(), "Missing documented source-build helper"

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    verify(parser.parse_args().root)
    print("Packaged documentation and README links verified.")
