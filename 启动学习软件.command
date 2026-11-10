#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT"
export PATH="/opt/homebrew/bin:/usr/local/bin:$PATH"
export STUDY_ROOT="$ROOT" STUDY_DATA_DIR="${STUDY_DATA_DIR:-$ROOT/data}" STUDY_PORT=4317
mkdir -p "$ROOT/verification"
core_pid=""
cleanup() {
  trap - EXIT INT TERM HUP
  [[ -z "$core_pid" ]] || kill "$core_pid" 2>/dev/null || true
  [[ -z "$core_pid" ]] || wait "$core_pid" 2>/dev/null || true
}
trap cleanup EXIT INT TERM HUP
probe() {
  local reply
  reply=$(/usr/bin/curl --noproxy '*' --fail --silent --max-time 3 http://127.0.0.1:4317/api/health) || return 1
  [[ "$reply" == *'"core":"rust"'* && "$reply" == *'"mode":"local"'* ]]
}
if [[ "${1:-}" != '--desktop' ]] && probe; then
  echo '学习软件已运行：http://127.0.0.1:4317'
  if [[ "${1:-}" != '--no-browser' ]]; then open http://127.0.0.1:4317; fi
  exit 0
fi
if [[ ! -x "$ROOT/server/target/debug/study-core" || ! -f "$ROOT/web/dist/index.html" ]]; then
  echo '首次构建学习软件…'
  export CARGO_HOME="$ROOT/.toolchain/cargo" RUSTUP_HOME="$ROOT/.toolchain/rustup"
  npm --prefix web run build
  "$CARGO_HOME/bin/cargo" build --locked --manifest-path server/Cargo.toml
fi
[[ -x "$ROOT/.toolchain/pdfcraft-cli/bin/pdfcraft-cli-0.20.0-macos-universal/pdfcraft-cli" ]] || { echo '本地 PDF 引擎缺失，请恢复项目中的官方 PdfCraft 工具。'; exit 1; }
if [[ "${1:-}" == '--desktop' ]]; then
  [[ -x "$ROOT/desktop/target/debug/study-desktop" ]] || { echo '桌面程序尚未编译，请查看本地运行说明。'; exit 1; }
  echo '正在启动 Tauri 学习窗口（Rust + PdfCraft，无需 Python）。关闭桌面程序结束本次会话。'
  runner="$ROOT/desktop/target/debug/study-desktop"
  if [[ -x "$ROOT/Study Local.app/Contents/MacOS/StudyBin" ]]; then runner="$ROOT/Study Local.app/Contents/MacOS/StudyBin"; fi
  "$runner" > "$ROOT/verification/desktop-session.log" 2>&1 &
  core_pid=$!
  wait "$core_pid"
  exit 0
fi
"$ROOT/server/target/debug/study-core" > "$ROOT/verification/local-server.log" 2>&1 &
core_pid=$!
core_ready=0
for ((i=0;i<30;i++)); do
  if probe; then core_ready=1; break; fi
  kill -0 "$core_pid" 2>/dev/null || break
  sleep 1
done
[[ "$core_ready" == 1 ]] || { echo '启动失败，请查看 verification/local-server.log'; exit 1; }
echo '学习软件已启动：http://127.0.0.1:4317'
echo '保留此窗口。按 Ctrl+C 或关闭窗口结束本次本地服务。'
echo "数据保存在：$STUDY_DATA_DIR"
if [[ "${1:-}" != '--no-browser' ]]; then open http://127.0.0.1:4317; fi
wait "$core_pid"
