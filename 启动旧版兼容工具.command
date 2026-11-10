#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
export PATH="/opt/homebrew/bin:/usr/local/bin:$PATH"
export PYTHON_BIN="$ROOT/.venv/bin/python"
if /usr/bin/curl --noproxy '*' --fail --silent --max-time 3 http://127.0.0.1:4318/api/agent/tools >/dev/null; then
  open http://127.0.0.1:4318
  exit 0
fi
cd "$ROOT/legacy-agent"
echo '旧版独立副本：http://127.0.0.1:4318。保留此窗口，Ctrl+C 结束。'
echo '旧版工具仍保留历史 Python 适配；新版日常运行不依赖该服务。'
exec node node_modules/next/dist/bin/next dev --webpack -H 127.0.0.1 -p 4318
