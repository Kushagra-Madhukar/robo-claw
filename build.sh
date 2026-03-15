#!/usr/bin/env bash
# Build ARIA-X (debug by default, use --release for production)
set -e
cd "$(dirname "$0")"

if [[ -f ".env" ]]; then
  set -a
  # shellcheck disable=SC1091
  source ".env"
  set +a
fi

if [[ "$1" == "--release" ]]; then
  cargo build --release -p aria-x
  echo "[build] Release binary: target/release/aria-x"
else
  cargo build -p aria-x
  echo "[build] Debug binary: target/debug/aria-x"
fi
