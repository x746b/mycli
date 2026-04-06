#!/usr/bin/env bash
# Grade benchmark results using API directly
# Usage: ./grade.sh [results-dir]

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
RESULTS_DIR="${1:-${SCRIPT_DIR}/results}"
GRADED="${RESULTS_DIR}/graded.md"

API_KEY=$(grep -A2 '\[cloud.deepseek\]' ~/.mycli/config.toml | grep api_key | head -1 | sed 's/.*= *"//;s/".*//')
if [[ -z "$API_KEY" ]]; then
    echo "Error: No DeepSeek API key found in ~/.mycli/config.toml"
    exit 1
fi

SYSTEM_PROMPT='You grade LLM responses. Score 1-5 on each criterion. Respond ONLY in this exact format, nothing else:
accuracy: N
hallucination: N
instruction_following: N
conciseness: N
notes: one line summary'

cat > "${GRADED}" <<EOF
# Graded Benchmark Results — $(date '+%Y-%m-%d %H:%M')

| Model | Test | Acc | Hal | Ins | Con | Notes |
|-------|------|-----|-----|-----|-----|-------|
EOF

for MODEL_DIR in "${RESULTS_DIR}"/*/; do
    [[ -d "$MODEL_DIR" ]] || continue
    MODEL=$(basename "$MODEL_DIR")
    echo "━━━ Grading: ${MODEL} ━━━"

    for RESULT_FILE in "${MODEL_DIR}"/*.md; do
        [[ -f "$RESULT_FILE" ]] || continue
        TEST_ID=$(basename "$RESULT_FILE" .md)

        [[ "$(cat "$RESULT_FILE")" == "FAIL" ]] && {
            echo "| ${MODEL} | ${TEST_ID} | - | - | - | - | FAIL |" >> "${GRADED}"
            printf "  %-25s SKIP\n" "${TEST_ID}"
            continue
        }

        printf "  %-25s " "${TEST_ID}"

        # Build JSON request via python using file input (avoids shell escaping issues)
        TMPJSON=$(mktemp ~/DeveloperArea/tmp/grade.XXXXXX)
        python3 - "$RESULT_FILE" "$SYSTEM_PROMPT" <<'PYEOF' > "$TMPJSON"
import json, sys

result_file = sys.argv[1]
system_prompt = sys.argv[2]

with open(result_file) as f:
    content = f.read()[:3000]  # cap content size

req = {
    "model": "deepseek-chat",
    "messages": [
        {"role": "system", "content": system_prompt},
        {"role": "user", "content": content}
    ],
    "max_tokens": 1024
}
print(json.dumps(req))
PYEOF

        TMPRES=$(mktemp ~/DeveloperArea/tmp/grade.XXXXXX)
        curl -s --max-time 60 \
            -H "Authorization: Bearer ${API_KEY}" \
            -H "Content-Type: application/json" \
            -d @"$TMPJSON" \
            "https://api.deepseek.com/chat/completions" > "$TMPRES"
        rm -f "$TMPJSON"

        GRADE=$(python3 - "$TMPRES" <<'PYEOF'
import json, sys
try:
    with open(sys.argv[1]) as f:
        r = json.load(f)
    msg = r['choices'][0]['message']
    print(msg.get('content') or msg.get('reasoning_content') or 'no content')
except Exception as e:
    print(f'accuracy: 0\nhallucination: 0\ninstruction_following: 0\nconciseness: 0\nnotes: API error: {e}')
PYEOF
        )
        rm -f "$TMPRES"

        ACC=$(echo "$GRADE" | grep -i "^accuracy:" | head -1 | sed 's/.*: *//' | tr -dc '0-5' | head -c1)
        HAL=$(echo "$GRADE" | grep -i "^hallucination:" | head -1 | sed 's/.*: *//' | tr -dc '0-5' | head -c1)
        INS=$(echo "$GRADE" | grep -i "^instruction" | head -1 | sed 's/.*: *//' | tr -dc '0-5' | head -c1)
        CON=$(echo "$GRADE" | grep -i "^conciseness:" | head -1 | sed 's/.*: *//' | tr -dc '0-5' | head -c1)
        NOTES=$(echo "$GRADE" | grep -i "^notes:" | head -1 | sed 's/.*: *//' | cut -c1-80)

        printf "acc:%s hal:%s ins:%s con:%s | %s\n" "${ACC:-?}" "${HAL:-?}" "${INS:-?}" "${CON:-?}" "${NOTES:-}"
        echo "| ${MODEL} | ${TEST_ID} | ${ACC:-?} | ${HAL:-?} | ${INS:-?} | ${CON:-?} | ${NOTES:-?} |" >> "${GRADED}"
    done
    echo ""
done

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Results: ${GRADED}"
