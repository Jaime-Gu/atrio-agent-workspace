#!/usr/bin/env bash
# Sourced by project scripts; use an existing Rust toolchain or the local fallback.
if ! command -v cargo >/dev/null 2>&1; then
  if [[ -x "$HOME/.cargo/bin/cargo" ]]; then
    export PATH="$HOME/.cargo/bin:$PATH"
  elif [[ -x "$APP_PROJECT_ROOT/../../work/toolchain/cargo/bin/cargo" ]]; then
    export CARGO_HOME="$(cd "$APP_PROJECT_ROOT/../../work/toolchain/cargo" && pwd)"
    export RUSTUP_HOME="$(cd "$APP_PROJECT_ROOT/../../work/toolchain/rustup" && pwd)"
    export PATH="$CARGO_HOME/bin:$PATH"
  else
    echo "Rust/Cargo was not found. Install Rust or configure its PATH." >&2
    return 1
  fi
fi
