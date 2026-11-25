#!/usr/bin/env python3
"""Produce a dependency license inventory; human review remains required."""
import json
import subprocess

metadata = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--locked", "--format-version", "1"], text=True))
rows = [{"name": p["name"], "version": p["version"],
         "license": p["license"], "license_file": p["license_file"],
         "source": p["source"]} for p in metadata["packages"]]
rows.sort(key=lambda row: (row["name"], row["version"]))
print(json.dumps(rows, indent=2))
missing = [row["name"] for row in rows if not row["license"] and not row["license_file"]]
if missing:
    raise SystemExit("Missing license metadata: " + ", ".join(missing))
