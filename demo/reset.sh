#!/usr/bin/env bash
# Reset disposable visitor state while keeping verified sample EPUBs.
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

exec 9>demo/data/.reset.lock
flock -n 9 || exit 0
python3 demo/prepare.py --verify

export BOKHYLLE_UID="${BOKHYLLE_UID:-$(id -u)}"
export BOKHYLLE_GID="${BOKHYLLE_GID:-$(id -g)}"
compose=(docker compose)
if [[ -f demo/.env ]]; then
  compose+=(--env-file demo/.env)
fi
compose+=(-f demo/compose.yaml)
"${compose[@]}" down
python3 demo/prepare.py --reset
"${compose[@]}" up -d --wait
