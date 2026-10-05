#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p .build/branding
icon_workdir="$(mktemp -d "${TMPDIR:-/tmp}/laut-icons.XXXXXX")"
trap 'rm -rf "$icon_workdir"' EXIT
iconset="$icon_workdir/Laut.iconset"
mkdir -p "$iconset"
# Package the selected artwork at the native 1x/2x macOS icon resolutions.
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" Resources/Branding/app-icon.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" Resources/Branding/app-icon.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o .build/branding/Laut.icns
sips -z 128 128 Resources/Branding/app-icon.png --out .build/branding/LautIcon.png >/dev/null
sips -z 36 36 Resources/Branding/mark.png --out .build/branding/LautMark.png >/dev/null
