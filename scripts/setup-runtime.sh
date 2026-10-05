#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ "$(uname -m)" != "arm64" ]]; then
  echo "Laut currently requires Apple Silicon (arm64)." >&2
  exit 1
fi
if ! command -v uv >/dev/null; then
  echo "Install uv first: https://docs.astral.sh/uv/getting-started/installation/" >&2
  exit 1
fi
uv venv .runtime/venv --python 3.12 --cache-dir .runtime/uv-cache
uv pip install --python .runtime/venv/bin/python --cache-dir .runtime/uv-cache -r requirements-lock.txt
echo "Runtime ready. Launch Laut and download a model in Models."
