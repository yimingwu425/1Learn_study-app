#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
if [[ -x "$ROOT/.toolchain/cargo/bin/cargo" ]]; then
  export PATH="$ROOT/.toolchain/cargo/bin:$PATH"
  export CARGO_HOME="$ROOT/.toolchain/cargo" RUSTUP_HOME="$ROOT/.toolchain/rustup"
fi
export STUDY_ROOT="$ROOT"
(cd "$ROOT/web" && { [[ -d node_modules ]] || npm ci; } && npm run build)
cargo build --manifest-path "$ROOT/server/Cargo.toml"
cargo run --manifest-path "$ROOT/desktop/Cargo.toml"
