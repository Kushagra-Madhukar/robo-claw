#!/usr/bin/env bash
# Run ARIA-X. Builds first unless --no-build. Pass config path as last arg.
#   ./run.sh                    # build + run with aria-x/config.toml
#   ./run.sh --no-build          # run without building
#   ./run.sh dist/.../config.toml
set -e
cd "$(dirname "$0")"

if [[ -f ".env" ]]; then
  set -a
  # shellcheck disable=SC1091
  source ".env"
  set +a
fi

ARGS=()
for arg in "$@"; do
  if [[ "$arg" == "--no-build" ]]; then NO_BUILD=1; else ARGS+=("$arg"); fi
done
[[ -z "$NO_BUILD" ]] && cargo build -p aria-x
if [[ ${#ARGS[@]} -eq 0 ]]; then ARGS=("aria-x/config.toml"); fi
exec ./target/debug/aria-x "${ARGS[@]}"
