#!/usr/bin/env bash
# Regenerates every raster icon from the three SVG sources:
#
#   apps/reader/public/icon.svg            favicon / PWA icon (rounded, full bleed)
#   apps/reader/public/icon-maskable.svg   PWA maskable icon (square, mark in the safe zone)
#   apps/desktop/icon-source/app-icon.svg  app icon on Apple's 1024 grid (824 px plate)
#
# Needs Google Chrome (headless, used as the SVG rasteriser) and `sips` (macOS).
# Runnable from anywhere.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DESKTOP="$(cd "$HERE/.." && pwd)"
PUBLIC="$(cd "$DESKTOP/../reader/public" && pwd)"
CHROME="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

render() { # <svg> <size> <png>
  cp "$1" "$TMP/in.svg"
  printf '<!doctype html><html><body style="margin:0;background:transparent"><img src="in.svg" style="display:block;width:100vw;height:100vh"></body></html>' > "$TMP/page.html"
  "$CHROME" --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=1 \
    --default-background-color=00000000 --window-size="$2,$2" \
    --screenshot="$3" "file://$TMP/page.html" >/dev/null 2>&1
}

render "$HERE/app-icon.svg" 1024 "$HERE/app-icon.png"
render "$PUBLIC/icon.svg" 512 "$PUBLIC/icon-512.png"
render "$PUBLIC/icon-maskable.svg" 512 "$PUBLIC/icon-maskable-512.png"
sips -z 192 192 "$PUBLIC/icon-512.png" --out "$PUBLIC/icon-192.png" >/dev/null
sips -z 192 192 "$PUBLIC/icon-maskable-512.png" --out "$PUBLIC/icon-maskable-192.png" >/dev/null

cd "$DESKTOP"
npx tauri icon icon-source/app-icon.png --output src-tauri/icons --ios-color '#7a2e22'
