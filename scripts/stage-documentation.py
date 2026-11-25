#!/usr/bin/env python3
"""Stage distribution documentation from the exact checked-out source revision."""
import argparse
import shutil
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

def stage(destination: Path):
    destination.mkdir(parents=True, exist_ok=True)
    for name in ("README.md", "LICENSE", "CONTRIBUTING.md", "CHANGELOG.md"):
        shutil.copyfile(ROOT / name, destination / name)
    for name in ("docs", "assets", "packaging"):
        # These are project-owned static files. Do not copy a build output or
        # a user's runtime environment into a distribution.
        for source in sorted((ROOT / name).rglob("*")):
            if source.is_symlink():
                raise ValueError(f"Unexpected documentation symlink: {source}")
            if source.is_file():
                target = destination / source.relative_to(ROOT)
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, target)
    # The optional host-native source builder is useful from a binary archive
    # too; include it when this revision provides it.
    helper = ROOT / "scripts/build-backend.py"
    if helper.is_file():
        target = destination / "scripts" / helper.name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(helper, target)

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path)
    stage(parser.parse_args().destination)
