#!/usr/bin/env bash
# Dev workflow: build + run with debug logs. Rebuilds on each run.
set -euo pipefail
cd "$(dirname "$0")"

if [[ -f ".env" ]]; then
  set -a
  # shellcheck disable=SC1091
  source ".env"
  set +a
fi

RUN_ID="$(date +"%Y%m%d-%H%M%S")"
LOG_DIR="${ARIA_LOG_DIR:-./logs/dev}"
mkdir -p "$LOG_DIR"
LOG_FILE="$LOG_DIR/aria-x-$RUN_ID.log"
ln -sfn "$(basename "$LOG_FILE")" "$LOG_DIR/latest.log"
echo "[dev.sh] logging to $LOG_FILE"

cargo build -p aria-x 2>&1 | tee "$LOG_FILE"

# Cap ONNX Runtime threads to avoid thermal throttle on Apple Silicon.
# Without these, fastembed/MiniLM-L6 will consume all CPU cores during inference.
export ORT_NUM_THREADS="${ORT_NUM_THREADS:-2}"
export RAYON_NUM_THREADS="${RAYON_NUM_THREADS:-2}"
export RUST_LOG="${RUST_LOG:-debug}"

./target/debug/aria-x "${1:-aria-x/config.toml}" "${@:2}" 2>&1 | tee -a "$LOG_FILE"
