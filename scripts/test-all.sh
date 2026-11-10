#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
CARGO="$(command -v cargo || true)"
if [[ -z "$CARGO" ]]; then
 export CARGO_HOME="$ROOT/.toolchain/cargo" RUSTUP_HOME="$ROOT/.toolchain/rustup"
 CARGO="$CARGO_HOME/bin/cargo"
fi
"$CARGO" test --locked --manifest-path server/Cargo.toml
"$CARGO" build --locked --manifest-path server/Cargo.toml
DEV_PYTHON="${PYTHON:-$ROOT/.venv/bin/python}"
[[ -x "$DEV_PYTHON" ]] || DEV_PYTHON=python3
"$DEV_PYTHON" -m unittest discover -s tests -v
npm --prefix web test
npm --prefix web run build
