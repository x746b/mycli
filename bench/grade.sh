#!/usr/bin/env bash
# Compatibility wrapper. Set BENCH_GRADER or use the interactive menu.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PYTHON="${BENCH_PYTHON:-python3}"
RESULTS_DIR="${1:-$SCRIPT_DIR/results}"

if [[ -z "${BENCH_GRADER:-}" ]]; then
    echo "Select a grader in ./bench.py, or set BENCH_GRADER (for example: deepseek)." >&2
    exec "$PYTHON" "$SCRIPT_DIR/bench.py" grade-menu
fi

exec "$PYTHON" "$SCRIPT_DIR/bench.py" grade --provider "$BENCH_GRADER" --results "$RESULTS_DIR"
