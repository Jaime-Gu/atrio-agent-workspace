#!/usr/bin/env bash
set -euo pipefail
APP_PROJECT_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$APP_PROJECT_ROOT"
source scripts/rust-env.sh
node scripts/check-version.mjs
exec npm run tauri -- dev --config src-tauri/tauri.dev.conf.json "$@"
