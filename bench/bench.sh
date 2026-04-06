#!/usr/bin/env bash
# MyCLI Model Benchmark Runner
# Usage: ./bench.sh [model-filter]
#   ./bench.sh                    # run all models
#   ./bench.sh WhiteRabbit        # only models matching "WhiteRabbit"
#   ./bench.sh --list             # list available models
#
# Results saved to bench/results/<model>/<test-id>.md

set -euo pipefail

# macOS-compatible timeout (GNU timeout not available by default)
run_with_timeout() {
    local secs=$1; shift
    "$@" &
    local pid=$!
    ( sleep "$secs"; kill "$pid" 2>/dev/null ) &
    local watchdog=$!
    wait "$pid" 2>/dev/null
    local ret=$?
    kill "$watchdog" 2>/dev/null
    wait "$watchdog" 2>/dev/null
    return $ret
}

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
MYCLI="${SCRIPT_DIR}/../mycli"
BENCH_FILE="${SCRIPT_DIR}/bench.toml"
RESULTS_DIR="${SCRIPT_DIR}/results"
TIMEOUT=120  # seconds per test

# oMLX endpoint
OMLX_BASE="${OMLX_BASE:-http://127.0.0.1:8000/v1}"
OMLX_KEY="${OMLX_KEY:-$(grep -m1 'api_key' ~/.mycli/config.toml 2>/dev/null | sed 's/.*= *"//;s/".*//' || echo 'mycli')}"

MODEL_FILTER="${1:-}"

# ── Fetch available models ──────────────────────────────────────────────────

fetch_models() {
    curl -s "${OMLX_BASE}/models" \
        -H "Authorization: Bearer ${OMLX_KEY}" 2>/dev/null \
    | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    for m in data.get('data', []):
        print(m['id'])
except: pass
" 2>/dev/null
}

# ── Parse bench.toml ────────────────────────────────────────────────────────

parse_tests() {
    python3 -c "
import sys
tests = []
current = {}
for line in open('${BENCH_FILE}'):
    line = line.strip()
    if line == '[[test]]':
        if current:
            tests.append(current)
        current = {}
    elif '=' in line and not line.startswith('#'):
        key, val = line.split('=', 1)
        key = key.strip()
        val = val.strip().strip('\"')
        current[key] = val
if current:
    tests.append(current)
for t in tests:
    print(f\"{t.get('id','?')}|{t.get('persona','code')}|{t.get('tier','simple')}|{t.get('prompt','')}\")
"
}

# ── Main ────────────────────────────────────────────────────────────────────

if [[ "${MODEL_FILTER}" == "--list" ]]; then
    echo "Available models on oMLX:"
    fetch_models | while read -r m; do echo "  - $m"; done
    exit 0
fi

MODELS=$(fetch_models)
if [[ -z "$MODELS" ]]; then
    echo "Error: No models found at ${OMLX_BASE}/models"
    exit 1
fi

# Apply filter
if [[ -n "$MODEL_FILTER" ]]; then
    MODELS=$(echo "$MODELS" | grep -i "$MODEL_FILTER" || true)
    if [[ -z "$MODELS" ]]; then
        echo "No models matching '${MODEL_FILTER}'"
        exit 1
    fi
fi

TESTS=$(parse_tests)
NUM_TESTS=$(echo "$TESTS" | wc -l | tr -d ' ')
NUM_MODELS=$(echo "$MODELS" | wc -l | tr -d ' ')

echo "╔══════════════════════════════════════════════════════════╗"
echo "║  MyCLI Model Benchmark                                  ║"
echo "╠══════════════════════════════════════════════════════════╣"
echo "║  Models: ${NUM_MODELS}                                            ║"
echo "║  Tests:  ${NUM_TESTS}                                           ║"
echo "║  Timeout: ${TIMEOUT}s per test                               ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""

echo "$MODELS" | while read -r MODEL; do
    MODEL_DIR="${RESULTS_DIR}/${MODEL}"
    mkdir -p "${MODEL_DIR}"

    echo "━━━ ${MODEL} ━━━"

    echo "$TESTS" | while IFS='|' read -r TEST_ID PERSONA TIER PROMPT; do
        OUTFILE="${MODEL_DIR}/${TEST_ID}.md"

        printf "  %-25s " "${TEST_ID}"

        START=$(date +%s)

        # Run mycli in single-shot mode with timeout
        # -y auto-approves tools to avoid interactive prompts blocking
        run_with_timeout "${TIMEOUT}" "${MYCLI}" \
            -m "${MODEL}" \
            -p "${PERSONA}" \
            -t "${TIER}" \
            -y \
            "${PROMPT}" \
            > "${OUTFILE}.tmp" 2>/dev/null || true

        END=$(date +%s)
        ELAPSED=$((END - START))

        # Strip ANSI escape codes from output
        if [[ -s "${OUTFILE}.tmp" ]]; then
            sed -i '' $'s/\x1b\\[[0-9;]*[mGKHJ]//g' "${OUTFILE}.tmp" 2>/dev/null || true
            TOKENS=$(wc -w < "${OUTFILE}.tmp" | tr -d ' ')

            # Write result with metadata
            cat > "${OUTFILE}" <<EOF
---
model: ${MODEL}
test: ${TEST_ID}
persona: ${PERSONA}
tier: ${TIER}
duration: ${ELAPSED}s
words: ${TOKENS}
---

# ${TEST_ID}

**Prompt:** ${PROMPT}

**Model:** ${MODEL} | **Persona:** ${PERSONA} | **Duration:** ${ELAPSED}s

## Response

$(cat "${OUTFILE}.tmp")
EOF
            rm -f "${OUTFILE}.tmp"
            printf "✓ %3ds %4d words\n" "${ELAPSED}" "${TOKENS}"
        else
            rm -f "${OUTFILE}.tmp"
            echo "FAIL" > "${OUTFILE}"
            printf "✗ timeout/error\n"
        fi
    done
    echo ""
done

# ── Generate summary ────────────────────────────────────────────────────────

SUMMARY="${RESULTS_DIR}/summary.md"
cat > "${SUMMARY}" <<EOF
# Benchmark Results — $(date '+%Y-%m-%d %H:%M')

| Model | Test | Persona | Duration | Words |
|-------|------|---------|----------|-------|
EOF

echo "$MODELS" | while read -r MODEL; do
    MODEL_DIR="${RESULTS_DIR}/${MODEL}"
    echo "$TESTS" | while IFS='|' read -r TEST_ID PERSONA TIER PROMPT; do
        OUTFILE="${MODEL_DIR}/${TEST_ID}.md"
        if [[ -f "$OUTFILE" ]] && [[ "$(cat "$OUTFILE")" != "FAIL" ]]; then
            DUR=$(grep "^duration:" "$OUTFILE" | sed 's/.*: //')
            WORDS=$(grep "^words:" "$OUTFILE" | sed 's/.*: //')
            echo "| ${MODEL} | ${TEST_ID} | ${PERSONA} | ${DUR} | ${WORDS} |"
        else
            echo "| ${MODEL} | ${TEST_ID} | ${PERSONA} | FAIL | - |"
        fi
    done
done >> "${SUMMARY}"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Results saved to: ${RESULTS_DIR}/"
echo "Summary: ${SUMMARY}"
