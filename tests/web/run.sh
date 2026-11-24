#!/usr/bin/env bash
# Runs the Playwright web test with a Node >= 18 (the default `node` on PATH may be older).
set -euo pipefail
cd "$(dirname "$0")"

node_ok() { [ -x "$1/node" ] && [ "$("$1/node" -p 'process.versions.node.split(".")[0]')" -ge 18 ] 2>/dev/null; }

pick_node_dir() {
  [ -n "${NODE_BIN_DIR:-}" ] && { echo "$NODE_BIN_DIR"; return; }
  local d
  if command -v node >/dev/null 2>&1; then
    d="$(dirname "$(command -v node)")"; node_ok "$d" && { echo "$d"; return; }
  fi
  # newest nvm-installed node first, then common locations
  for d in $(ls -d "${NVM_DIR:-$HOME/.nvm}"/versions/node/*/bin 2>/dev/null | sort -V -r) \
           /opt/homebrew/opt/node/bin /opt/homebrew/bin /usr/local/bin; do
    node_ok "$d" && { echo "$d"; return; }
  done
  return 1
}

dir="$(pick_node_dir)" || { echo "No Node >= 18 found. Install one (nvm install 22) or set NODE_BIN_DIR." >&2; exit 2; }
export PATH="$dir:$PATH"
echo "Using node $(node -v) from $dir"
npm install --no-audit --no-fund --silent
exec node test.mjs
