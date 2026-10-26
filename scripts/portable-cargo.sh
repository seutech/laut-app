#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
if [[ -x "$root/.runtime/rust/cargo/bin/cargo" ]]; then
  export CARGO_HOME="$root/.runtime/rust/cargo"
  export RUSTUP_HOME="$root/.runtime/rust/toolchains"
  export PATH="$CARGO_HOME/bin:$PATH"
fi
cd "$root/portable"
exec cargo "$@"
