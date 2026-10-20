#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ENGINE_PATH="$SCRIPT_DIR/repair-codex-startup.py"
if [[ ! -f "$ENGINE_PATH" ]]; then
  echo '缺少 repair-codex-startup.py，请先完整解压修复包。' >&2
  exit 1
fi

PYTHON_PATH=''
for candidate in python3 /opt/homebrew/bin/python3 /usr/local/bin/python3 /usr/bin/python3; do
  if command -v "$candidate" >/dev/null 2>&1; then
    if "$candidate" -c 'import sys, sqlite3; sys.exit(0 if sys.version_info >= (3, 9) else 1)' >/dev/null 2>&1; then
      PYTHON_PATH="$(command -v "$candidate")"
      break
    fi
  fi
done
if [[ -z "$PYTHON_PATH" ]]; then
  echo '需要 Python 3.9+（含 sqlite3）。脚本不会自动安装软件。' >&2
  exit 1
fi

if [[ $# -eq 0 && -t 0 ]]; then
  echo '1. 只检查（默认，不启动应用、不修改原库）'
  echo '2. 备份后修复已确认的状态库冲突'
  read -r -p '请选择 [1/2，回车为 1]：' choice
  if [[ "$choice" == '2' ]]; then
    set -- --repair
  elif [[ -z "$choice" || "$choice" == '1' ]]; then
    set -- --diagnose
  else
    echo '已取消。'
    exit 0
  fi
fi

set +e
"$PYTHON_PATH" "$ENGINE_PATH" "$@"
result=$?
set -e
if [[ -t 0 ]]; then
  read -r -p '按回车关闭窗口…' unused
fi
exit "$result"
