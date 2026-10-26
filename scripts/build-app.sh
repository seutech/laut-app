#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
configuration="${1:-release}"
build_arguments=(-c "$configuration")
if [[ -n "${LAUT_SDK:-}" ]]; then
  build_arguments+=(--sdk "$LAUT_SDK")
fi
bash scripts/build-icons.sh
swift build "${build_arguments[@]}" --product Laut --jobs 4
binary_dir="$(swift build "${build_arguments[@]}" --show-bin-path)"
app_dir="$PWD/dist/Laut Swift Legacy.app"
mkdir -p "$app_dir/Contents/MacOS" "$app_dir/Contents/Resources"
# Upstream license/resource files may have been copied read-only on a prior build.
chmod -R u+w "$app_dir/Contents/Resources"
cp "$binary_dir/Laut" "$app_dir/Contents/MacOS/Laut"
cp Resources/Info.plist "$app_dir/Contents/Info.plist"
plutil -replace CFBundleIdentifier -string "io.github.seutech.laut.swift-legacy" "$app_dir/Contents/Info.plist"
plutil -replace CFBundleName -string "Laut Swift Legacy" "$app_dir/Contents/Info.plist"
plutil -replace CFBundleDisplayName -string "Laut Swift Legacy" "$app_dir/Contents/Info.plist"
cp -R Resources/de.lproj Resources/en.lproj "$app_dir/Contents/Resources/"
cp Resources/embedding_worker.py Resources/model_download.py Resources/mlx_worker.py Resources/media_download.py "$app_dir/Contents/Resources/"
cp .build/branding/Laut.icns .build/branding/LautIcon.png .build/branding/LautMark.png "$app_dir/Contents/Resources/"
cp LICENSE THIRD_PARTY.md "$app_dir/Contents/Resources/"
mkdir -p "$app_dir/Contents/Resources/Licenses"
cp ThirdParty/*.txt "$app_dir/Contents/Resources/Licenses/"
for resource in "$binary_dir"/*.bundle; do
  if [[ -e "$resource" ]]; then
    destination="$app_dir/Contents/Resources/$(basename "$resource")"
    [[ -d "$destination" ]] && chmod -R u+w "$destination"
    cp -R "$resource" "$app_dir/Contents/Resources/"
  fi
done
# A local development pointer, never committed or included in source releases.
python3 - "$PWD/.runtime" "$app_dir/Contents/Resources/development.json" <<'PY'
import json, sys
from pathlib import Path
Path(sys.argv[2]).write_text(json.dumps({"runtime": sys.argv[1]}))
PY
codesign --force --deep --sign - "$app_dir"
echo "Built $app_dir"
