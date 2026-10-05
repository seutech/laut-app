#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
configuration="${1:-release}"
bash scripts/build-icons.sh
swift build -c "$configuration" --product Laut --jobs 4
binary_dir="$(swift build -c "$configuration" --show-bin-path)"
app_dir="$PWD/dist/Laut.app"
mkdir -p "$app_dir/Contents/MacOS" "$app_dir/Contents/Resources"
# Upstream license/resource files may have been copied read-only on a prior build.
chmod -R u+w "$app_dir/Contents/Resources"
cp "$binary_dir/Laut" "$app_dir/Contents/MacOS/Laut"
cp Resources/Info.plist "$app_dir/Contents/Info.plist"
cp Resources/mlx_worker.py "$app_dir/Contents/Resources/"
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
