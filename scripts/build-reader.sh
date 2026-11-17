#!/usr/bin/env bash
# Builds the DARM reader (web/PWA front end, also the UI of the desktop and
# mobile shells) into apps/reader/dist:
#
#   1. scripts/build-wasm.sh        the WebAssembly core -> apps/reader/src/wasm-pkg
#   2. npm ci                       exact dependencies from package-lock.json
#   3. npm run build                typecheck + vite build (+ precache.json for the service worker)
#
# Usage: scripts/build-reader.sh [--skip-wasm] [--skip-install]
#
#   --skip-wasm      reuse the existing apps/reader/src/wasm-pkg
#   --skip-install   reuse the existing apps/reader/node_modules (npm ci deletes
#                    and reinstalls it, which disturbs a running dev server)
#
# Environment:
#   DARM_MIN_FREE_GIB   refuse to start with less free disk space (default 2)
#
# Runnable from any directory.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SKIP_WASM=0
SKIP_INSTALL=0
for arg in "$@"; do
  case "$arg" in
    --skip-wasm) SKIP_WASM=1 ;;
    --skip-install) SKIP_INSTALL=1 ;;
    -h | --help)
      sed -n '2,19p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "error: unknown option: $arg (try --help)" >&2
      exit 2
      ;;
  esac
done

# Refuses to start a build when the disk holding the repository is nearly full.
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

# No debug info: it is the bulk of a Rust target directory and nothing here needs it.
export CARGO_PROFILE_RELEASE_DEBUG="${CARGO_PROFILE_RELEASE_DEBUG:-0}"
export CARGO_PROFILE_DEV_DEBUG="${CARGO_PROFILE_DEV_DEBUG:-0}"

if [ "$SKIP_WASM" -eq 0 ]; then
  require_free_space "the WebAssembly build"
  "$ROOT/scripts/build-wasm.sh"
elif [ ! -f "$ROOT/apps/reader/src/wasm-pkg/darm_wasm_bg.wasm" ]; then
  echo "error: --skip-wasm given but apps/reader/src/wasm-pkg/darm_wasm_bg.wasm does not exist" >&2
  exit 1
fi

require_free_space "the reader build"
if [ "$SKIP_INSTALL" -eq 0 ]; then
  npm --prefix "$ROOT/apps/reader" ci
elif [ ! -d "$ROOT/apps/reader/node_modules" ]; then
  echo "error: --skip-install given but apps/reader/node_modules does not exist" >&2
  exit 1
fi
npm --prefix "$ROOT/apps/reader" run build

echo "reader built: $ROOT/apps/reader/dist"
