#!/usr/bin/env python3
"""Collect resolved Rust dependency notices for a distributable Laut package."""

import argparse
import json
import os
import pathlib
import re
import shutil
import subprocess


ROOT = pathlib.Path(__file__).resolve().parents[2]
WORKSPACE = ROOT / "portable"
NOTICE_PREFIXES = ("LICENSE", "LICENCE", "COPYING", "NOTICE", "AUTHORS")


def safe(value: str) -> str:
    return re.sub(r"[^A-Za-z0-9._+-]", "_", value)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=pathlib.Path, required=True)
    args = parser.parse_args()
    cargo = os.environ.get("CARGO") or shutil.which("cargo")
    cargo_environment = os.environ.copy()
    if cargo is None:
        local_cargo_home = ROOT / ".runtime" / "rust" / "cargo"
        local_rustup_home = ROOT / ".runtime" / "rust" / "toolchains"
        local_cargo = local_cargo_home / "bin" / "cargo"
        if not local_cargo.is_file():
            raise SystemExit("Cargo was not found in PATH or the local Laut toolchain.")
        cargo = str(local_cargo)
        cargo_environment["CARGO_HOME"] = str(local_cargo_home)
        cargo_environment["RUSTUP_HOME"] = str(local_rustup_home)
        cargo_environment["PATH"] = str(local_cargo_home / "bin") + os.pathsep + cargo_environment.get("PATH", "")
    metadata = json.loads(
        subprocess.check_output(
            [cargo, "metadata", "--locked", "--format-version", "1"],
            cwd=WORKSPACE,
            env=cargo_environment,
        )
    )
    members = set(metadata["workspace_members"])
    packages = sorted(
        (package for package in metadata["packages"] if package["id"] not in members),
        key=lambda package: (package["name"].lower(), package["version"], package["id"]),
    )
    output = args.out.resolve()
    if output.exists():
        shutil.rmtree(output)
    output.mkdir(parents=True)
    rows = []
    missing = []
    for package in packages:
        folder = pathlib.Path(package["manifest_path"]).parent
        candidates = []
        license_file = package.get("license_file")
        if license_file:
            path = pathlib.Path(license_file)
            candidates.append(path if path.is_absolute() else folder / path)
        for path in sorted(folder.iterdir(), key=lambda item: item.name.lower()):
            if path.is_file() and path.name.upper().startswith(NOTICE_PREFIXES):
                candidates.append(path)
        unique = []
        seen = set()
        for path in candidates:
            try:
                resolved = path.resolve(strict=True)
            except OSError:
                continue
            if resolved in seen or resolved.stat().st_size > 2 * 1024 * 1024:
                continue
            seen.add(resolved)
            unique.append(resolved)
        destination = output / safe(f"{package['name']}-{package['version']}")
        copied = []
        for index, source in enumerate(unique):
            destination.mkdir(exist_ok=True)
            name = source.name if index == 0 else f"{index + 1}-{source.name}"
            shutil.copy2(source, destination / name)
            copied.append(name)
        declared = package.get("license") or "not declared"
        source_url = package.get("repository") or package.get("source") or ""
        rows.append(
            f"| `{package['name']}` | `{package['version']}` | {declared} | "
            f"{source_url} | {', '.join(copied) or 'no separate file in package'} |"
        )
        if declared == "not declared" and not copied:
            missing.append(f"{package['name']} {package['version']}")
    summary = [
        "# Resolved Rust dependencies",
        "",
        "Generated from the locked Cargo dependency graph for this Laut build.",
        "The copied files remain under their original terms.",
        "",
        "| Package | Version | Declared license | Source | Included files |",
        "| --- | --- | --- | --- | --- |",
        *rows,
        "",
    ]
    (output / "RUST-DEPENDENCIES.md").write_text("\n".join(summary), encoding="utf-8")
    if missing:
        raise SystemExit("Dependencies without license metadata: " + ", ".join(missing))
    print(f"Collected notices for {len(packages)} resolved Rust dependencies in {output}")


if __name__ == "__main__":
    main()
