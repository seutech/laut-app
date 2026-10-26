#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
arch="$(uname -m)"
case "$arch" in
  arm64) label="ARM64" ;;
  x86_64) label="X64" ;;
  *) echo "Unsupported macOS architecture: $arch" >&2; exit 1 ;;
esac
bash "$root/scripts/build-portable-app.sh"
archive="$root/dist/Laut-macOS-$label.zip"
rm -f "$archive" "$archive.sha256"
ditto -c -k --sequesterRsrc --keepParent "$root/dist/Laut.app" "$archive"
(
  cd "$root/dist"
  shasum -a 256 "$(basename "$archive")" > "$(basename "$archive").sha256"
)
printf '%s\n' "$archive"
