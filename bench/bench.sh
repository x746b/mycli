#!/usr/bin/env bash
# MyCLI Model Benchmark Runner
# Usage: ./bench.sh [model-filter]
#   ./bench.sh                    # run all models
#   ./bench.sh WhiteRabbit        # only models matching "WhiteRabbit"
#   ./bench.sh --list             # list available models
#
# Second argument (or BENCH_TESTS) selects which tests to run:
#   ./bench.sh MODEL 'code-*,reason-river'   # ids / globs, comma-separated
#   ./bench.sh MODEL --multiline             # only multi-line prompts
#   ./bench.sh MODEL --failed                # only missing/FAIL results
#
# Results saved to bench/results/<model>/<test-id>.md   (human-readable)
#                  bench/results/<model>/<test-id>.raw  (verbatim model output)
# Per-test scratch dirs under bench/results/<model>/_work/<test-id>/
#
# Set MYCLI_RAW=1 (requires the render.rs raw-mode patch) to get unrendered
# output. Without it, markdown is rendered and `*` in payloads may be eaten.

set -euo pipefail

# Custom TMPDIR to avoid EDR triggers on /tmp (set BENCH_TMPDIR to override)
if [[ -n "${BENCH_TMPDIR:-}" ]]; then
    export TMPDIR="$BENCH_TMPDIR"
    mkdir -p "$TMPDIR"
fi

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
BENCH_FILE="${BENCH_FILE:-${SCRIPT_DIR}/bench.toml}"
RESULTS_DIR="${SCRIPT_DIR}/results"
TIMEOUT="${BENCH_TIMEOUT:-120}"  # seconds per test

# oMLX endpoint
OMLX_BASE="${OMLX_BASE:-http://127.0.0.1:8000/v1}"
OMLX_KEY="${OMLX_KEY:-$(grep -m1 'api_key' ~/.mycli/config.toml 2>/dev/null | sed 's/.*= *"//;s/".*//' || echo 'mycli')}"

MODEL_FILTER="${1:-}"
TEST_FILTER="${2:-${BENCH_TESTS:-}}"

# Ask mycli for verbatim output. Harmless on binaries built before the
# render.rs raw-mode patch — they simply ignore it.
export MYCLI_RAW="${MYCLI_RAW:-1}"

# Prefer a Python with tomllib (3.11+); parse_bench.py has a 3.9 fallback but
# the real parser is more trustworthy for the security prompts.
PYTHON="${BENCH_PYTHON:-}"
if [[ -z "$PYTHON" ]]; then
    for cand in python3.13 python3.12 python3.11 /opt/homebrew/bin/python3.11 python3; do
        if command -v "$cand" >/dev/null 2>&1 && "$cand" -c 'import tomllib' 2>/dev/null; then
            PYTHON="$(command -v "$cand")"; break
        fi
    done
fi
PYTHON="${PYTHON:-python3}"

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
except Exception: pass
" 2>/dev/null
}

# ── Parse bench toml with a real TOML parser ────────────────────────────────
# The previous line-by-line parser silently truncated every multi-line
# \"\"\"...\"\"\" prompt at its first newline, and would also split any prompt
# containing '|'. Prompts are now written to individual files; the manifest
# carries only single-line metadata.

PROMPT_DIR=""

parse_tests() {
    # Writes prompts to $PROMPT_DIR/<n>.txt, prints TSV: index<TAB>id<TAB>persona<TAB>tier
    "$PYTHON" "${SCRIPT_DIR}/parse_bench.py" "$BENCH_FILE" "$PROMPT_DIR"
}

strip_ansi() {
    # Portable ANSI/OSC scrubber (BSD sed can't do this reliably)
    "$PYTHON" -c "
import sys, re
data = sys.stdin.buffer.read().decode('utf-8', 'replace')
data = re.sub(r'\x1b\][^\x07\x1b]*(?:\x07|\x1b\\\\)', '', data)  # OSC
data = re.sub(r'\x1b[@-Z\\\\-_]|\x1b\[[0-?]*[ -/]*[@-~]', '', data)  # CSI/ESC
sys.stdout.write(data)
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

PROMPT_DIR="$(mktemp -d "${TMPDIR:-/tmp}/bench-prompts.XXXXXX")"
trap 'rm -rf "$PROMPT_DIR"' EXIT

MANIFEST_ALL="${PROMPT_DIR}/manifest-all.tsv"
MANIFEST="${PROMPT_DIR}/manifest.tsv"
parse_tests > "$MANIFEST_ALL"
TOTAL_TESTS=$(wc -l < "$MANIFEST_ALL" | tr -d ' ')

# ── Test selection ──────────────────────────────────────────────────────────
# TEST_FILTER: comma-separated ids/globs, or one of:
#   --multiline  tests whose prompt spans >1 line (truncated by the old parser)
#   --failed     tests with no result yet, or whose result is FAIL
select_tests() {
    local filter="$1" first_model="$2"
    case "$filter" in
        "")
            cat "$MANIFEST_ALL" ;;
        --multiline)
            awk -F'\t' '$5 > 1' "$MANIFEST_ALL" ;;
        --failed)
            local dir="${RESULTS_DIR}/${first_model}"
            while IFS=$'\t' read -r IDX TID REST; do
                local f="${dir}/${TID}.md"
                if [[ ! -f "$f" ]] || [[ "$(head -c 4 "$f" 2>/dev/null)" == "FAIL" ]]; then
                    printf '%s\t%s\t%s\n' "$IDX" "$TID" "$REST"
                fi
            done < "$MANIFEST_ALL" ;;
        *)
            local pat
            : > "${PROMPT_DIR}/sel.tsv"
            while IFS=$'\t' read -r IDX TID REST; do
                IFS=',' read -ra PATS <<< "$filter"
                for pat in "${PATS[@]}"; do
                    # shellcheck disable=SC2053  # glob match is intentional
                    if [[ "$TID" == $pat ]]; then
                        printf '%s\t%s\t%s\n' "$IDX" "$TID" "$REST" >> "${PROMPT_DIR}/sel.tsv"
                        break
                    fi
                done
            done < "$MANIFEST_ALL"
            cat "${PROMPT_DIR}/sel.tsv" ;;
    esac
}

FIRST_MODEL="$(echo "$MODELS" | head -1)"
select_tests "$TEST_FILTER" "$FIRST_MODEL" > "$MANIFEST"

if [[ ! -s "$MANIFEST" ]]; then
    echo "No tests matched filter '${TEST_FILTER}'"
    exit 1
fi

NUM_TESTS=$(wc -l < "$MANIFEST" | tr -d ' ')
NUM_MODELS=$(echo "$MODELS" | wc -l | tr -d ' ')

printf '╔══════════════════════════════════════════════════════════╗\n'
printf '║  MyCLI Model Benchmark                                   ║\n'
printf '╠══════════════════════════════════════════════════════════╣\n'
printf '║  Models: %-47s║\n' "${NUM_MODELS}"
printf '║  Tests:  %-47s║\n' "${NUM_TESTS} of ${TOTAL_TESTS}${TEST_FILTER:+  (filter: ${TEST_FILTER})}"
printf '║  Timeout: %-46s║\n' "${TIMEOUT}s per test"
printf '║  Raw mode: %-45s║\n' "${MYCLI_RAW:-0}"
printf '╚══════════════════════════════════════════════════════════╝\n'
echo ""

while read -r MODEL; do
    [[ -z "$MODEL" ]] && continue
    MODEL_DIR="${RESULTS_DIR}/${MODEL}"
    WORK_ROOT="${MODEL_DIR}/_work"
    mkdir -p "${MODEL_DIR}" "${WORK_ROOT}"

    echo "━━━ ${MODEL} ━━━"

    while IFS=$'\t' read -r IDX TEST_ID PERSONA TIER LINES; do
        [[ -z "${TEST_ID:-}" ]] && continue
        OUTFILE="${MODEL_DIR}/${TEST_ID}.md"
        RAWFILE="${MODEL_DIR}/${TEST_ID}.raw"
        PROMPT_FILE="${PROMPT_DIR}/${IDX}.txt"
        PROMPT="$(cat "$PROMPT_FILE")"

        # Each test gets a clean working directory so tools that write files
        # neither pollute bench/ nor leak state into the next test.
        RUN_DIR="${WORK_ROOT}/${TEST_ID}"
        rm -rf "$RUN_DIR"; mkdir -p "$RUN_DIR"

        printf "  %-25s " "${TEST_ID}"

        START=$(date +%s)

        run_with_timeout "${TIMEOUT}" "${MYCLI}" \
            -m "${MODEL}" \
            -p "${PERSONA}" \
            -t "${TIER}" \
            -C "${RUN_DIR}" \
            -y \
            "${PROMPT}" \
            > "${RAWFILE}.tmp" 2>/dev/null || true

        END=$(date +%s)
        ELAPSED=$((END - START))

        if [[ -s "${RAWFILE}.tmp" ]]; then
            strip_ansi < "${RAWFILE}.tmp" > "${RAWFILE}"
            rm -f "${RAWFILE}.tmp"
            WORDS=$(wc -w < "${RAWFILE}" | tr -d ' ')
            ARTIFACTS=$(find "$RUN_DIR" -type f 2>/dev/null | wc -l | tr -d ' ')

            {
                echo "---"
                echo "model: ${MODEL}"
                echo "test: ${TEST_ID}"
                echo "persona: ${PERSONA}"
                echo "tier: ${TIER}"
                echo "duration: ${ELAPSED}s"
                echo "words: ${WORDS}"
                echo "artifacts: ${ARTIFACTS}"
                echo "raw: ${TEST_ID}.raw"
                echo "---"
                echo ""
                echo "# ${TEST_ID}"
                echo ""
                echo "**Model:** ${MODEL} | **Persona:** ${PERSONA} | **Duration:** ${ELAPSED}s | **Artifacts:** ${ARTIFACTS}"
                echo ""
                echo "## Prompt"
                echo ""
                echo '```text'
                cat "$PROMPT_FILE"
                echo ""
                echo '```'
                echo ""
                echo "## Response"
                echo ""
                echo '```text'
                cat "${RAWFILE}"
                echo ""
                echo '```'
            } > "${OUTFILE}"

            printf "✓ %3ds %5d words  %2d files\n" "${ELAPSED}" "${WORDS}" "${ARTIFACTS}"
        else
            rm -f "${RAWFILE}.tmp"
            printf 'FAIL\n' > "${OUTFILE}"
            printf "✗ timeout/error\n"
        fi
    done < "$MANIFEST"
    echo ""
done <<< "$MODELS"

# ── Generate summary ────────────────────────────────────────────────────────

SUMMARY="${RESULTS_DIR}/summary.md"
{
    echo "# Benchmark Results — $(date '+%Y-%m-%d %H:%M')"
    echo ""
    echo "| Model | Test | Persona | Duration | Words | Artifacts |"
    echo "|-------|------|---------|----------|-------|-----------|"
} > "${SUMMARY}"

while read -r MODEL; do
    [[ -z "$MODEL" ]] && continue
    MODEL_DIR="${RESULTS_DIR}/${MODEL}"
    while IFS=$'\t' read -r IDX TEST_ID PERSONA TIER LINES; do
        [[ -z "${TEST_ID:-}" ]] && continue
        OUTFILE="${MODEL_DIR}/${TEST_ID}.md"
        if [[ -f "$OUTFILE" ]] && [[ "$(head -c 4 "$OUTFILE")" != "FAIL" ]]; then
            DUR=$(sed -n 's/^duration: //p' "$OUTFILE" | head -1)
            WORDS=$(sed -n 's/^words: //p' "$OUTFILE" | head -1)
            ARTS=$(sed -n 's/^artifacts: //p' "$OUTFILE" | head -1)
            echo "| ${MODEL} | ${TEST_ID} | ${PERSONA} | ${DUR} | ${WORDS} | ${ARTS} |"
        else
            echo "| ${MODEL} | ${TEST_ID} | ${PERSONA} | FAIL | - | - |"
        fi
    done < "$MANIFEST_ALL"
done <<< "$MODELS" >> "${SUMMARY}"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Results saved to: ${RESULTS_DIR}/"
echo "Summary: ${SUMMARY}"
