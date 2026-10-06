#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
if ! command -v uv >/dev/null; then
  echo "Install uv first: https://docs.astral.sh/uv/"
  exit 1
fi
if [[ ! -x .runtime/download-venv/bin/python ]]; then
  uv venv .runtime/download-venv --python 3.12 --cache-dir .runtime/uv-cache
fi
uv pip install --python .runtime/download-venv/bin/python --cache-dir .runtime/uv-cache -r requirements-download.txt
echo "Downloader installed separately from the speech runtime."
echo "Deno (2.3+), FFmpeg and FFprobe must also be installed; select their executables in Laut Settings if needed."
