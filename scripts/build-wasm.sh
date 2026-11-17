#!/usr/bin/env bash
# Builds the DARM reader core (crates/darm-wasm) for the browser and writes
# the wasm-bindgen "web" bundle to apps/reader/src/wasm-pkg.
#
# Requirements:
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli --version 0.2.130   (must match the crate's pinned version)
#
# Runnable from any directory.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
WASM="$TARGET_DIR/wasm32-unknown-unknown/wasm-release/darm_wasm.wasm"
OUT_DIR="apps/reader/src/wasm-pkg"
REQUIRED_BINDGEN="0.2.130"

if ! command -v wasm-bindgen >/dev/null 2>&1; then
  echo "error: wasm-bindgen CLI not found; install it with:" >&2
  echo "  cargo install wasm-bindgen-cli --version $REQUIRED_BINDGEN" >&2
  exit 1
fi
FOUND_BINDGEN="$(wasm-bindgen --version | awk '{print $2}')"
if [ "$FOUND_BINDGEN" != "$REQUIRED_BINDGEN" ]; then
  echo "error: wasm-bindgen CLI $FOUND_BINDGEN found, but crates/darm-wasm pins $REQUIRED_BINDGEN" >&2
  exit 1
fi

cargo build -p darm-wasm --target wasm32-unknown-unknown --profile wasm-release

mkdir -p "$OUT_DIR"
wasm-bindgen --target web --out-dir "$OUT_DIR" --out-name darm_wasm "$WASM"

SIZE="$(wc -c < "$OUT_DIR/darm_wasm_bg.wasm" | tr -d ' ')"
echo "wrote $OUT_DIR/darm_wasm_bg.wasm: $SIZE bytes ($((SIZE / 1024)) KiB)"
if command -v gzip >/dev/null 2>&1; then
  GZIPPED="$(gzip -9 -c "$OUT_DIR/darm_wasm_bg.wasm" | wc -c | tr -d ' ')"
  echo "gzip -9: $GZIPPED bytes ($((GZIPPED / 1024)) KiB)"
fi
