#!/bin/bash
# download_models.sh — reads model repos from to-download.txt and downloads them
# Runs inside the Docker container, paths are container-internal

# Install dependencies (in case running standalone)
pip install -q huggingface_hub hf_transfer 2>/dev/null

# Enable fast transfers
export HF_HUB_ENABLE_HF_TRANSFER=2
# HuggingFace token for authenticated downloads (avoids rate limiting)
export HF_TOKEN="hf_your-token"

MODELS_DIR="/models"
LIST_FILE="/models/to-download.txt"
LOG_FILE="/models/download.log"

echo "========================================" | tee -a "$LOG_FILE"
echo "Model download started at $(date)" | tee -a "$LOG_FILE"
echo "========================================" | tee -a "$LOG_FILE"

# Check HF_TOKEN
if [ -n "$HF_TOKEN" ]; then
    echo "HF_TOKEN is set — authenticated downloads enabled" | tee -a "$LOG_FILE"
else
    echo "WARNING: HF_TOKEN not set — downloads will be rate-limited and slow!" | tee -a "$LOG_FILE"
    echo "  Get a free token at https://huggingface.co/settings/tokens" | tee -a "$LOG_FILE"
fi

if [ ! -f "$LIST_FILE" ]; then
    echo "ERROR: $LIST_FILE not found!" | tee -a "$LOG_FILE"
    exit 1
fi

# Count non-empty, non-comment lines
TOTAL=$(grep -v '^\s*#' "$LIST_FILE" | grep -v '^\s*$' | wc -l)
CURRENT=0
FAILED=0

echo "Found $TOTAL models to download" | tee -a "$LOG_FILE"
echo "" | tee -a "$LOG_FILE"

while IFS= read -r line; do
    # Skip comments and empty lines
    [[ "$line" =~ ^[[:space:]]*# ]] && continue
    [[ -z "${line// }" ]] && continue

    REPO="$line"
    CURRENT=$((CURRENT + 1))

    # Create local dir name from repo (org/model → org__model)
    DIR_NAME=$(echo "$REPO" | tr '/' '__')
    DEST="$MODELS_DIR/$DIR_NAME"

    echo "----------------------------------------" | tee -a "$LOG_FILE"
    echo "[$CURRENT/$TOTAL] Downloading: $REPO" | tee -a "$LOG_FILE"
    echo "  → $DEST" | tee -a "$LOG_FILE"
    echo "  Started: $(date)" | tee -a "$LOG_FILE"

    if hf download "$REPO" --local-dir "$DEST" 2>&1 | tee -a "$LOG_FILE"; then
        echo "  ✓ Completed: $(date)" | tee -a "$LOG_FILE"
        SIZE=$(du -sh "$DEST" 2>/dev/null | cut -f1)
        echo "  Size: $SIZE" | tee -a "$LOG_FILE"
    else
        echo "  ✗ FAILED: $(date)" | tee -a "$LOG_FILE"
        FAILED=$((FAILED + 1))
    fi

    echo "" | tee -a "$LOG_FILE"

done < "$LIST_FILE"

echo "========================================" | tee -a "$LOG_FILE"
echo "Download complete at $(date)" | tee -a "$LOG_FILE"
echo "Total: $TOTAL | Succeeded: $((TOTAL - FAILED)) | Failed: $FAILED" | tee -a "$LOG_FILE"
echo "========================================" | tee -a "$LOG_FILE"

# Summary of downloaded models
echo "" | tee -a "$LOG_FILE"
echo "Downloaded models:" | tee -a "$LOG_FILE"
du -sh "$MODELS_DIR"/*/ 2>/dev/null | tee -a "$LOG_FILE"
