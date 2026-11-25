#!/usr/bin/env python3
"""Collect original Cargo dependency notices, conservatively including all locked targets.

The result is an evidence bundle, not a legal compliance certification.
"""
import argparse
import json
import re
import shutil
import subprocess
from pathlib import Path

NOTICE = re.compile(r"^(license|licence|copying|copyright|notice|unlicense)(?:$|[-_.])", re.I)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path, help="New directory for dependency notices")
    args = parser.parse_args()
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--format-version", "1"], text=True))
    args.output.mkdir(parents=True, exist_ok=False)
    inventory = []
    for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
        if package["source"] is None:
            continue
        root = Path(package["manifest_path"]).resolve().parent
        dest = args.output / f'{package["name"]}-{package["version"]}'
        copied = set()
        for candidate in root.rglob("*"):
            if candidate.is_symlink() or not candidate.is_file():
                continue
            if NOTICE.match(candidate.name):
                copied.add(candidate.relative_to(root))
        declared = package.get("license_file")
        if declared:
            candidate = (root / declared).resolve()
            if not candidate.is_relative_to(root) or not candidate.is_file():
                raise SystemExit(f'Invalid declared license file: {package["name"]}')
            copied.add(candidate.relative_to(root))
        for relative in sorted(copied):
            target = dest / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(root / relative, target)
        inventory.append({
            "name": package["name"], "version": package["version"],
            "license": package["license"], "license_file": declared,
            "source": package["source"],
            "notices": [str(dest.relative_to(args.output) / p) for p in sorted(copied)]
        })
    (args.output / "inventory.json").write_text(json.dumps(inventory, indent=2) + "\n")
    missing = [f'{row["name"]} {row["version"]}' for row in inventory if not row["notices"]]
    (args.output / "README.txt").write_text(
        "Third-party source notices from the locked Cargo dependency set.\n"
        "Includes build, test and non-Linux dependencies conservatively.\n"
        "Original license texts are retained; this is not a legal certification.\n"
        "Review dual-license choices, embedded data, upstream notices and any\n"
        "dependencies with no packaged notice before public distribution.\n\n"
        "Packages without discovered notice files:\n" + ("\n".join(missing) or "None") + "\n")
    print(f"Collected notices for {len(inventory) - len(missing)}/{len(inventory)} packages.")
    if missing:
        print("Manual review required:", ", ".join(missing))


if __name__ == "__main__":
    main()
