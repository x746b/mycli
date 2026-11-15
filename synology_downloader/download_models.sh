#!/bin/bash
# Entries: org/repo[@revision][::subfolder][##include,include]
set -euo pipefail

# Use the environment token when provided; otherwise replace this placeholder.
export HF_TOKEN="${HF_TOKEN:-hf_XXXXXX}"
MODELS_DIR="${MODELS_DIR:-/models}"
LIST_FILE="${LIST_FILE:-$MODELS_DIR/to-download.txt}"
LOG_FILE="${LOG_FILE:-$MODELS_DIR/download.log}"
[[ -f "$LIST_FILE" ]] || { echo "Missing list: $LIST_FILE" >&2; exit 1; }

# Container-local isolated environment; requires the uv Python image in Compose.
mkdir -p /tmp/hf-model-downloader
uv venv --python /usr/local/bin/python3 /tmp/hf-model-downloader/.venv
uv pip install --python /tmp/hf-model-downloader/.venv/bin/python3 'huggingface_hub>=1,<2'
HF=/tmp/hf-model-downloader/.venv/bin/hf
FAILED=0
while IFS= read -r line || [[ -n "$line" ]]; do
    line="${line%$'\r'}"
    line="${line#"${line%%[![:space:]]*}"}"
    line="${line%"${line##*[![:space:]]}"}"
    [[ -z "$line" || "$line" == \#* ]] && continue
    INCLUDES=()
    if [[ "$line" == *'##'* ]]; then
        IFS=',' read -r -a INCLUDES <<< "${line#*##}"
        line="${line%%##*}"
    fi
    SUBDIR=""
    if [[ "$line" == *'::'* ]]; then
        SUBDIR="${line#*::}"
        line="${line%%::*}"
    fi
    REVISION=""
    if [[ "$line" == *'@'* ]]; then
        REVISION="${line##*@}"
        line="${line%@*}"
    fi
    REPO="$line"
    DEST="$MODELS_DIR/${REPO//\//_}"
    ARGS=(download "$REPO" --local-dir "$DEST")
    [[ -z "$REVISION" ]] || ARGS+=(--revision "$REVISION")
    if [[ ${#INCLUDES[@]} -gt 0 ]]; then
        for pattern in "${INCLUDES[@]}"; do ARGS+=(--include "$pattern"); done
    elif [[ -n "$SUBDIR" ]]; then
        ARGS+=(--include "$SUBDIR/*")
    fi
    echo "Downloading $REPO ${REVISION:+at $REVISION} to $DEST" | tee -a "$LOG_FILE"
    if "$HF" "${ARGS[@]}" 2>&1 | tee -a "$LOG_FILE"; then
        echo "Completed: $REPO" | tee -a "$LOG_FILE"
    else
        echo "FAILED: $REPO" | tee -a "$LOG_FILE"
        FAILED=$((FAILED + 1))
    fi
done < "$LIST_FILE"
[[ "$FAILED" -eq 0 ]]
