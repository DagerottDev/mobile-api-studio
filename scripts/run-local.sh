#!/usr/bin/env bash
set -euo pipefail
repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_dir"
npm run build --prefix apps/desktop
cargo build -p mobile-api-studio-script-worker
cargo run -p mobile-api-studio-server -- "$@"
