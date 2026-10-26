#!/usr/bin/env python3
"""Create a versioned Windows or Linux release archive from built Laut binaries."""

import argparse
import hashlib
import os
import pathlib
import shutil
import subprocess
import tarfile
import tempfile
import zipfile


ROOT = pathlib.Path(__file__).resolve().parents[2]
WORKSPACE = ROOT / "portable"
VERSION = (ROOT / "VERSION").read_text(encoding="utf-8").strip()
ZIP_TIMESTAMP = (1980, 1, 1, 0, 0, 0)


def sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def add_zip_file(
    target: zipfile.ZipFile, source: pathlib.Path, archive_name: pathlib.Path
) -> None:
    """Add a file with the oldest timestamp supported by the ZIP format.

    Release inputs may deliberately use the Unix epoch for reproducibility. Python's
    ZIP writer rejects those dates because the format starts in 1980, so normalize
    the archive metadata while retaining permissions from the source file.
    """
    info = zipfile.ZipInfo(archive_name.as_posix(), ZIP_TIMESTAMP)
    info.create_system = 3
    info.external_attr = (source.stat().st_mode & 0xFFFF) << 16
    info.compress_type = zipfile.ZIP_DEFLATED
    with source.open("rb") as input_file, target.open(info, "w", force_zip64=True) as output:
        shutil.copyfileobj(input_file, output, length=1024 * 1024)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--platform", choices=("windows", "linux"), required=True)
    parser.add_argument("--arch", default="X64")
    parser.add_argument("--out", type=pathlib.Path, default=ROOT / "dist")
    args = parser.parse_args()
    suffix = ".exe" if args.platform == "windows" else ""
    label = "Windows" if args.platform == "windows" else "Linux"
    package_name = f"Laut-{label}-{args.arch.upper()}"
    args.out.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="laut-release-") as temporary:
        stage = pathlib.Path(temporary) / package_name
        tools = stage / "tools"
        licenses = stage / "licenses"
        tools.mkdir(parents=True)
        licenses.mkdir()
        desktop = WORKSPACE / "target" / "release" / f"laut-portable{suffix}"
        cli = WORKSPACE / "target" / "release" / f"laut-cli{suffix}"
        for source, name in ((desktop, f"Laut{suffix}"), (cli, f"laut-cli{suffix}")):
            if not source.is_file():
                raise SystemExit(f"Missing release binary: {source}")
            shutil.copy2(source, stage / name)
        worker_bundle = WORKSPACE / "target" / "worker-bundle"
        if not worker_bundle.is_dir():
            raise SystemExit(f"Missing worker bundle: {worker_bundle}")
        for source in sorted(worker_bundle.iterdir()):
            destination = licenses if source.suffix.lower() == ".txt" else tools
            if source.is_file():
                shutil.copy2(source, destination / source.name)
        for name in ("README.md", "LICENSE", "THIRD_PARTY.md", "CHANGELOG.md", "VERSION"):
            shutil.copy2(ROOT / name, stage / name)
        if not VERSION:
            raise SystemExit("VERSION is empty")
        subprocess.run(
            [
                os.environ.get("PYTHON", "python3"),
                str(WORKSPACE / "scripts" / "collect-rust-licenses.py"),
                "--out",
                str(licenses / "rust"),
            ],
            check=True,
            cwd=WORKSPACE,
        )
        if args.platform == "windows":
            archive = args.out / f"{package_name}.zip"
            with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as target:
                for source in sorted(stage.rglob("*")):
                    if source.is_file():
                        add_zip_file(target, source, source.relative_to(stage.parent))
        else:
            archive = args.out / f"{package_name}.tar.gz"
            with tarfile.open(archive, "w:gz") as target:
                target.add(stage, arcname=package_name)
    checksum = args.out / f"{archive.name}.sha256"
    checksum.write_text(f"{sha256(archive)}  {archive.name}\n", encoding="ascii")
    print(archive.resolve())


if __name__ == "__main__":
    main()
