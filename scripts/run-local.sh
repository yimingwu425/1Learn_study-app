#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
if command -v cargo >/dev/null; then CARGO="$(command -v cargo)"
elif [[ -x "$ROOT/.toolchain/cargo/bin/cargo" ]]; then
  export CARGO_HOME="$ROOT/.toolchain/cargo" RUSTUP_HOME="$ROOT/.toolchain/rustup"
  CARGO="$CARGO_HOME/bin/cargo"
else echo 'Rust 未安装。请从 https://rustup.rs 安装后再运行。' >&2; exit 1; fi
[[ -x "$ROOT/.toolchain/pdfcraft-cli/bin/pdfcraft-cli-0.20.0-macos-universal/pdfcraft-cli" ]] || { echo 'Project-local official PdfCraft engine is missing.' >&2; exit 1; }
if [[ ! -d web/node_modules ]]; then npm --prefix web ci --cache "${TMPDIR:-/tmp}/study-npm"; fi
npm --prefix web run build
"$CARGO" build --manifest-path server/Cargo.toml
export STUDY_ROOT="$ROOT"
export STUDY_DATA_DIR="${STUDY_DATA_DIR:-$ROOT/data}"
export STUDY_PORT="${STUDY_PORT:-4317}"
echo "打开 http://127.0.0.1:$STUDY_PORT 。按 Ctrl+C 退出。数据：$STUDY_DATA_DIR"
exec "$ROOT/server/target/debug/study-core"
