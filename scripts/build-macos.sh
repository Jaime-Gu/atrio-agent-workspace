#!/usr/bin/env bash
set -euo pipefail
APP_PROJECT_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$APP_PROJECT_ROOT"
PIXEL_CHANNEL="${1:-beta}"
if [[ "$#" -gt 0 ]]; then shift; fi
case "$PIXEL_CHANNEL" in dev|beta) ;; *) echo "Usage: bash scripts/build-macos.sh [dev|beta] [Tauri build flags]" >&2; exit 1;; esac
if [[ "$(uname -s)" != Darwin ]]; then echo "Build macOS bundles on macOS." >&2; exit 1; fi
source scripts/rust-env.sh
command -v npm >/dev/null 2>&1
xcode-select -p >/dev/null
if [[ ! -d node_modules ]]; then npm ci; fi
node scripts/check-version.mjs
# Payload was prepared and signed before candidate freeze; builds never download.
node scripts/package-codex-runtime.mjs verify
if [[ -n "${PIXEL_CANDIDATE_MANIFEST:-}" ]]; then
  export PIXEL_BUILD_CHANNEL="$PIXEL_CHANNEL"
  node scripts/candidate.mjs verify-env
fi
export CARGO_HTTP_MULTIPLEXING="${CARGO_HTTP_MULTIPLEXING:-false}"
# Generate the app first. The DMG is created only after verifying all embedded
# Mach-O signatures and signing the outer app without re-signing its children.
if [[ "$PIXEL_CHANNEL" == dev ]]; then
  npm run tauri -- build --config src-tauri/tauri.dev.conf.json --target aarch64-apple-darwin --features custom-protocol --bundles app --ci "$@"
  ATRIO_APP_NAME="Atrio WorkSpace Dev"
else
  npm run tauri -- build --target aarch64-apple-darwin --features custom-protocol --bundles app --ci "$@"
  ATRIO_APP_NAME="Atrio WorkSpace Beta"
fi
if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
  case "$CARGO_TARGET_DIR" in
    /*) ATRIO_TARGET_ROOT="$CARGO_TARGET_DIR" ;;
    *) ATRIO_TARGET_ROOT="$APP_PROJECT_ROOT/src-tauri/$CARGO_TARGET_DIR" ;;
  esac
else
  ATRIO_TARGET_ROOT="$APP_PROJECT_ROOT/src-tauri/target"
fi
ATRIO_BUNDLE_ROOT="$ATRIO_TARGET_ROOT/aarch64-apple-darwin/release/bundle"
ATRIO_APP_PATH="$ATRIO_BUNDLE_ROOT/macos/$ATRIO_APP_NAME.app"
node scripts/package-codex-runtime.mjs verify-app "$ATRIO_APP_PATH"
/usr/bin/codesign --force --sign - --timestamp=none "$ATRIO_APP_PATH"
/usr/bin/codesign --verify --deep --strict "$ATRIO_APP_PATH"
node scripts/package-codex-runtime.mjs verify-app "$ATRIO_APP_PATH"
if [[ "$PIXEL_CHANNEL" == beta ]]; then
  ATRIO_VERSION="$(node -p 'JSON.parse(require("fs").readFileSync("package.json","utf8")).version')"
  ATRIO_DMG_PATH="$ATRIO_BUNDLE_ROOT/dmg/${ATRIO_APP_NAME}_${ATRIO_VERSION}_aarch64.dmg"
  # The existing target directory may contain an older generated image. Preserve
  # it under a unique name; frozen/staged candidates are never touched here.
  if [[ -e "$ATRIO_DMG_PATH" ]]; then
    mv "$ATRIO_DMG_PATH" "${ATRIO_DMG_PATH}.previous-$(date +%s)-$$"
  fi
  node scripts/package-codex-runtime.mjs create-dmg "$ATRIO_APP_PATH" "$ATRIO_DMG_PATH"
fi
node scripts/package-codex-runtime.mjs verify
node scripts/stage-macos.mjs "$PIXEL_CHANNEL"
