#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
version="$(tr -d '\r\n' < "$root/VERSION")"
if [[ -z "$version" ]]; then
  echo "VERSION is empty." >&2
  exit 1
fi
short_version="${version%%-*}"
if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "The .app bundle can only be assembled on macOS." >&2
  exit 1
fi
mkdir -p "$root/.runtime/build-cache/clang" "$root/.runtime/build-cache/swift"
export CLANG_MODULE_CACHE_PATH="${CLANG_MODULE_CACHE_PATH:-$root/.runtime/build-cache/clang}"
export SWIFTPM_MODULECACHE_OVERRIDE="${SWIFTPM_MODULECACHE_OVERRIDE:-$root/.runtime/build-cache/swift}"
remap_flag="--remap-path-prefix=$root=/src/laut"
if [[ "${RUSTFLAGS:-}" != *"$remap_flag"* ]]; then
  export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }$remap_flag"
fi
if [[ -n "${LAUT_MACOS_SDK:-}" ]]; then
  if [[ ! -d "$LAUT_MACOS_SDK" ]]; then
    echo "LAUT_MACOS_SDK is not a directory: $LAUT_MACOS_SDK" >&2
    exit 1
  fi
  export SDKROOT="$LAUT_MACOS_SDK"
fi
bash "$root/scripts/portable-cargo.sh" build --release --workspace --locked --jobs 4
bash "$root/scripts/build-icons.sh"
app="$root/dist/Laut.app"
stage="$root/dist/.Laut.app.$$"
trap 'rm -rf "$stage"' EXIT
rm -rf "$stage"
mkdir -p "$stage/Contents/MacOS" "$stage/Contents/Resources/Licenses"
cp "$root/portable/target/release/laut-portable" "$stage/Contents/MacOS/Laut"
cp "$root/portable/target/release/laut-cli" "$stage/Contents/MacOS/laut-cli"
if [[ -f "$root/portable/target/worker-bundle/whisper-cli" ]]; then
  mkdir -p "$stage/Contents/MacOS/tools"
  cp "$root/portable/target/worker-bundle/whisper-cli" "$stage/Contents/MacOS/tools/whisper-cli"
  cp "$root/portable/target/worker-bundle/"*.txt "$stage/Contents/Resources/Licenses/"
  codesign --force --sign - "$stage/Contents/MacOS/tools/whisper-cli"
fi
for tool in llama-completion llama-embedding; do
  if [[ -f "$root/portable/target/worker-bundle/$tool" ]]; then
    mkdir -p "$stage/Contents/MacOS/tools"
    cp "$root/portable/target/worker-bundle/$tool" "$stage/Contents/MacOS/tools/$tool"
    codesign --force --sign - "$stage/Contents/MacOS/tools/$tool"
  fi
done
if [[ -f "$root/portable/target/worker-bundle/LLAMA-LICENSE.txt" ]]; then
  cp "$root/portable/target/worker-bundle/LLAMA-"*.txt "$stage/Contents/Resources/Licenses/"
fi
cp "$root/LICENSE" "$stage/Contents/Resources/Licenses/LAUT-LICENSE.txt"
cp "$root/THIRD_PARTY.md" "$stage/Contents/Resources/Licenses/THIRD_PARTY.md"
cp "$root/CHANGELOG.md" "$stage/Contents/Resources/CHANGELOG.md"
cp "$root/README.md" "$stage/Contents/Resources/README.md"
cp "$root/VERSION" "$stage/Contents/Resources/VERSION"
python3 "$root/portable/scripts/collect-rust-licenses.py" --out "$stage/Contents/Resources/Licenses/Rust"
cp "$root/.build/branding/Laut.icns" "$stage/Contents/Resources/AppIcon.icns"
cat > "$stage/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>io.github.seutech.laut</string>
<key>CFBundleName</key><string>Laut</string>
<key>CFBundleDisplayName</key><string>Laut</string>
<key>CFBundleExecutable</key><string>Laut</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$short_version</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleIconFile</key><string>AppIcon</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSMicrophoneUsageDescription</key><string>Laut records your microphone locally to transcribe interviews and meetings.</string>
<key>NSScreenCaptureUsageDescription</key><string>Laut captures meeting audio locally. No screen images are saved.</string>
<key>NSAudioCaptureUsageDescription</key><string>Laut records computer audio locally for meeting transcripts.</string>
<key>LSMinimumSystemVersion</key><string>13.0</string>
</dict></plist>
PLIST
codesign --force --sign - "$stage/Contents/MacOS/laut-cli"
codesign --force --sign - "$stage/Contents/MacOS/Laut"
codesign --force --deep --sign - "$stage"
codesign --verify --deep --strict "$stage"
rm -rf "$app"
mv "$stage" "$app"
trap - EXIT
codesign --verify --strict "$app"
printf '%s\n' "$app"
