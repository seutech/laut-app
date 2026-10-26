#!/usr/bin/env python3
"""Fail if portable core/engine acquire desktop dependencies (including transitive ones)."""
import json
import subprocess
from pathlib import Path

data = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--offline", "--format-version", "1"], cwd=Path(__file__).resolve().parent))
packages = {p["id"]: p for p in data["packages"]}
nodes = {n["id"]: n for n in data["resolve"]["nodes"]}
for root in ("laut-core", "laut-engine"):
    package = next(p for p in packages.values() if p["name"] == root)
    seen, stack = set(), [package["id"]]
    while stack:
        node = stack.pop()
        if node in seen:
            continue
        seen.add(node)
        name = packages[node]["name"]
        assert not name.startswith(("egui", "eframe", "winit", "rfd", "rodio", "cpal")), (root, name)
        assert root != "laut-core" or name not in ("laut-engine", "laut-portable", "laut-cli"), (root, name)
        stack.extend(nodes[node]["dependencies"])
    print(f"{root}: UI-independent ({len(seen)} packages checked)")
