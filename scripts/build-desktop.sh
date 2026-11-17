#!/usr/bin/env bash
# Builds the DARM desktop app (Tauri 2 shell around the reader) for the
# platform this script runs on:
#
#   1. scripts/build-reader.sh      the front end -> apps/reader/dist
#   2. npm ci (apps/desktop)        the pinned Tauri CLI
#   3. tauri build                  the native shell and its bundles
#
# Usage: scripts/build-desktop.sh [--skip-reader] [--skip-wasm] [--skip-install] [-- <tauri build args>]
#
#   --skip-reader    reuse the existing apps/reader/dist
#   --skip-wasm      passed to build-reader.sh
#   --skip-install   passed to build-reader.sh; also reuses apps/desktop/node_modules
#   -- ...           everything after `--` goes to `tauri build`, for example
#                    `-- --bundles app` (macOS .app only) or `-- --bundles deb,appimage`
#
# Output: apps/desktop/src-tauri/target/release/bundle/
#
# The bundles are NOT signed for distribution. macOS gets an ad-hoc signature
# (bundle.macOS.signingIdentity is "-"); see docs/platforms.md for signing,
# notarization and the environment variables Tauri reads.
#
# Environment:
#   DARM_MIN_FREE_GIB   refuse to start with less free disk space (default 2)
#
# Runnable from any directory.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SKIP_READER=0
SKIP_INSTALL=0
READER_ARGS=()
TAURI_ARGS=()
while [ "$#" -gt 0 ]; do
  case "$1" in
    --skip-reader) SKIP_READER=1 ;;
    --skip-wasm) READER_ARGS+=("--skip-wasm") ;;
    --skip-install)
      SKIP_INSTALL=1
      READER_ARGS+=("--skip-install")
      ;;
    --)
      shift
      TAURI_ARGS=("$@")
      break
      ;;
    -h | --help)
      sed -n '2,25p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "error: unknown option: $1 (try --help)" >&2
      exit 2
      ;;
  esac
  shift
done

require_free_space() {
  local min_gib="${DARM_MIN_FREE_GIB:-2}"
  local free_kib
  free_kib="$(df -Pk "$ROOT" | awk 'NR == 2 { print $4 }')"
  if [ "$free_kib" -lt $((min_gib * 1024 * 1024)) ]; then
    echo "error: only $((free_kib / 1024)) MiB free on the disk holding $ROOT;" >&2
    echo "       $1 needs at least ${min_gib} GiB (set DARM_MIN_FREE_GIB to override)." >&2
    exit 1
  fi
}

# No debug info: a Tauri target directory is about 1 GiB without it and several with it.
export CARGO_PROFILE_RELEASE_DEBUG="${CARGO_PROFILE_RELEASE_DEBUG:-0}"
export CARGO_PROFILE_DEV_DEBUG="${CARGO_PROFILE_DEV_DEBUG:-0}"

if [ "$SKIP_READER" -eq 0 ]; then
  # ${arr[@]+...} keeps `set -u` happy with an empty array on bash 3.2 (macOS).
  "$ROOT/scripts/build-reader.sh" ${READER_ARGS[@]+"${READER_ARGS[@]}"}
elif [ ! -f "$ROOT/apps/reader/dist/index.html" ]; then
  echo "error: --skip-reader given but apps/reader/dist/index.html does not exist" >&2
  exit 1
fi

require_free_space "the desktop build"
cd "$ROOT/apps/desktop"
if [ "$SKIP_INSTALL" -eq 0 ] || [ ! -d node_modules ]; then
  npm ci
fi

# The reader was built (or deliberately reused) above, so tell Tauri not to
# build it a second time through its beforeBuildCommand hook.
npx tauri build --config '{"build":{"beforeBuildCommand":""}}' ${TAURI_ARGS[@]+"${TAURI_ARGS[@]}"}

echo "desktop bundles: $ROOT/apps/desktop/src-tauri/target/release/bundle"
