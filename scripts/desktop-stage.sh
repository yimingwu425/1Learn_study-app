#!/usr/bin/env bash
# Run on the target OS. This stages resources, not a finished installer.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
if [[ -x "$ROOT/.toolchain/cargo/bin/cargo" ]]; then
  export PATH="$ROOT/.toolchain/cargo/bin:$PATH"
  export CARGO_HOME="$ROOT/.toolchain/cargo" RUSTUP_HOME="$ROOT/.toolchain/rustup"
fi
TARGET="$(rustc -vV | sed -n 's/^host: //p')"
[[ -n "$TARGET" ]] || { echo 'Cannot identify native Rust target' >&2; exit 1; }
[[ "$TARGET" == *apple-darwin ]] || { echo 'This checked-in engine is macOS universal. Stage an official engine for the actual target first.' >&2; exit 1; }
ENGINE="$ROOT/.toolchain/pdfcraft-cli/bin/pdfcraft-cli-0.20.0-macos-universal"
[[ -x "$ENGINE/pdfcraft-cli" ]] || { echo 'Official PdfCraft runtime missing.' >&2; exit 1; }
(cd "$ROOT/web" && npm ci && npm run build)
cargo build --release --target "$TARGET" --manifest-path "$ROOT/server/Cargo.toml"
RUNTIME="$ROOT/desktop/runtime"
mkdir -p "$RUNTIME/bin" "$RUNTIME/web" "$RUNTIME/tools"
BIN=study-core
[[ "$TARGET" == *windows* ]] && BIN=study-core.exe
cp "$ROOT/server/target/$TARGET/release/$BIN" "$RUNTIME/bin/$BIN"
# Replace only generated staging content, never the user's data directory.
rm -rf "$RUNTIME/web/dist"
cp -R "$ROOT/web/dist" "$RUNTIME/web/dist"
mkdir -p "$RUNTIME/.toolchain/pdfcraft-cli/bin" "$RUNTIME/server"
cp -R "$ENGINE" "$RUNTIME/.toolchain/pdfcraft-cli/bin/"
cp -R "$ROOT/server/assets" "$RUNTIME/server/"
printf '%s\n' 'Rust core + PdfCraft 0.20.0 + OFL Noto font; no Python runtime. Local Tesseract is required only for damaged text-layer boundary location and is not bundled by this script.' > "$RUNTIME/RUNTIME-DEPENDENCIES.txt"
cp -R "$ROOT/tools/schemas" "$RUNTIME/tools/"
printf '%s\n' "$TARGET" > "$RUNTIME/target-triple.txt"
printf 'Staged native runtime for %s. No Python runtime; Tesseract OCR remains a native external prerequisite for damaged text layers.\n' "$TARGET"
