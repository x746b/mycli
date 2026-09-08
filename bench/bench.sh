#!/usr/bin/env bash
# Compatibility wrapper for the pre-1.2 benchmark command.
# Usage: ./bench.sh [model-filter] [test-glob[,test-glob...]]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PYTHON="${BENCH_PYTHON:-python3}"

if [[ "${1:-}" == "--list" ]]; then
    exec "$PYTHON" "$SCRIPT_DIR/bench.py" list
fi

MODEL_FILTER="${1:-}"
TEST_FILTER="${2:-${BENCH_TESTS:-}}"
SUITE="${BENCH_FILE:-$SCRIPT_DIR/prompts/smoke.toml}"
ARGS=(run --suite "$SUITE")

if [[ -n "$MODEL_FILTER" ]]; then
    ARGS+=(--models "$MODEL_FILTER")
fi

case "$TEST_FILTER" in
    "") ;;
    --failed) ARGS+=(--failed) ;;
    --multiline)
        echo "--multiline was removed in 1.2; select tests from ./bench.py instead" >&2
        exit 2
        ;;
    *)
        IFS=',' read -r -a FILTERS <<< "$TEST_FILTER"
        ARGS+=(--tests "${FILTERS[@]}")
        ;;
esac

exec "$PYTHON" "$SCRIPT_DIR/bench.py" "${ARGS[@]}"
