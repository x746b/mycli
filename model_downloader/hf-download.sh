#!/bin/bash
# hf-download.sh — Download HuggingFace models to ~/AI/models
# Reads model repos from to-download.txt (one per line)
#
# A line may name a subfolder inside the repo with `::`, for repos that pack
# several quants into one repo:
#   orcarouter/Qwen3.8-27B-Uncensored-MLX::8-bit
# Only that subfolder is fetched, and its contents are flattened into the
# local model dir (…_MLX-8-bit/) so the server sees a normal model directory.
#
# Usage:
#   ./hf-download.sh                    # download all from to-download.txt
#   ./hf-download.sh my-list.txt        # download from custom list
#   ./hf-download.sh --dry-run          # show what would be downloaded
#   ./hf-download.sh --info             # show model sizes before downloading

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
HF="$SCRIPT_DIR/bin/hf"
MODELS_DIR="$HOME/AI/models"
LIST_FILE="${1:-$SCRIPT_DIR/to-download.txt}"
LOG_FILE="$MODELS_DIR/download.log"

# Fast transfers come from the hf-xet backend, which huggingface_hub uses by
# default when the hf-xet package is installed. Nothing to enable here.
export HF_HOME="$HOME/AI/models/.cache/huggingface"

# HuggingFace token — avoids rate limiting, required for gated repos.
# Get a free token at https://huggingface.co/settings/tokens and export it:
#   export HF_TOKEN=hf_your_token_here
export HF_TOKEN="${HF_TOKEN:-}"

# Parse flags
DRY_RUN=false
INFO_ONLY=false
if [[ "$1" == "--dry-run" ]]; then
    DRY_RUN=true
    LIST_FILE="${2:-$SCRIPT_DIR/to-download.txt}"
elif [[ "$1" == "--info" ]]; then
    INFO_ONLY=true
    LIST_FILE="${2:-$SCRIPT_DIR/to-download.txt}"
fi

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
NC='\033[0m'

header() {
    echo -e "${CYAN}========================================${NC}"
    echo -e "${CYAN}HuggingFace Model Downloader${NC}"
    echo -e "${CYAN}========================================${NC}"
    echo "List file: $LIST_FILE"
    echo "Target:    $MODELS_DIR"
    echo "Models:    $TOTAL"
    if [ -n "$HF_TOKEN" ]; then
        echo -e "Auth:      ${GREEN}token set${NC}"
    else
        echo -e "Auth:      ${YELLOW}no token (may be rate-limited)${NC}"
    fi
    echo ""
}

if [ ! -f "$LIST_FILE" ]; then
    echo -e "${RED}ERROR: $LIST_FILE not found!${NC}"
    echo "Create it with one HuggingFace repo per line, e.g.:"
    echo "  mlx-community/NVIDIA-Nemotron-3-Super-120B-A12B-5bit"
    exit 1
fi

# Split "org/repo::subfolder" into REPO_ID / SUBDIR / local DIR_NAME
parse_entry() {
    REPO_ID="${1%%::*}"
    if [[ "$1" == *"::"* ]]; then
        SUBDIR="${1##*::}"
        DIR_NAME="$(echo "$REPO_ID" | tr '/' '_')-$SUBDIR"
    else
        SUBDIR=""
        DIR_NAME="$(echo "$REPO_ID" | tr '/' '_')"
    fi
}

# Collect non-empty, non-comment lines
REPOS=()
while IFS= read -r line; do
    [[ "$line" =~ ^[[:space:]]*# ]] && continue
    [[ -z "${line// }" ]] && continue
    REPOS+=("$line")
done < "$LIST_FILE"

TOTAL=${#REPOS[@]}
if [ "$TOTAL" -eq 0 ]; then
    echo -e "${YELLOW}No models found in $LIST_FILE${NC}"
    exit 0
fi

header

# --info mode: show sizes and exit
if $INFO_ONLY; then
    echo -e "${CYAN}Model sizes (querying HuggingFace...):${NC}"
    echo ""
    TOTAL_SIZE=0
    for REPO in "${REPOS[@]}"; do
        parse_entry "$REPO"
        if [ -n "$SUBDIR" ]; then
            SIZE_BYTES=$(curl -sL ${HF_TOKEN:+-H "Authorization: Bearer $HF_TOKEN"} \
                "https://huggingface.co/api/models/$REPO_ID/tree/main/$SUBDIR" \
                | python3 -c "import sys,json; print(sum((f.get('lfs') or {}).get('size') or f.get('size',0) for f in json.load(sys.stdin)))" 2>/dev/null)
        else
            SIZE_BYTES=$("$HF" models info "$REPO_ID" 2>/dev/null | python3 -c "import sys,json; print(json.load(sys.stdin).get('used_storage',0))" 2>/dev/null)
        fi
        if [ -n "$SIZE_BYTES" ] && [ "$SIZE_BYTES" -gt 0 ] 2>/dev/null; then
            SIZE_GB=$(python3 -c "print(f'{$SIZE_BYTES/1e9:.1f}')")
            TOTAL_SIZE=$((TOTAL_SIZE + SIZE_BYTES))
            echo -e "  ${GREEN}$SIZE_GB GB${NC}  $REPO"
        else
            echo -e "  ${RED}???${NC}     $REPO"
        fi
    done
    echo ""
    TOTAL_GB=$(python3 -c "print(f'{$TOTAL_SIZE/1e9:.1f}')")
    echo -e "${CYAN}Total: $TOTAL_GB GB${NC}"
    echo ""
    DISK_FREE=$(df -h "$MODELS_DIR" | awk 'NR==2{print $4}')
    echo "Disk free: $DISK_FREE"
    exit 0
fi

# --dry-run mode
if $DRY_RUN; then
    echo -e "${YELLOW}DRY RUN — nothing will be downloaded${NC}"
    echo ""
    for REPO in "${REPOS[@]}"; do
        parse_entry "$REPO"
        DEST="$MODELS_DIR/$DIR_NAME"
        if [ -d "$DEST" ]; then
            echo -e "  ${YELLOW}[EXISTS]${NC}  $REPO → $DIR_NAME"
        else
            echo -e "  ${GREEN}[NEW]${NC}     $REPO → $DIR_NAME"
        fi
    done
    exit 0
fi

# Download mode
echo "========================================" | tee -a "$LOG_FILE"
echo "Download started at $(date)" | tee -a "$LOG_FILE"
echo "========================================" | tee -a "$LOG_FILE"

CURRENT=0
FAILED=0
SKIPPED=0

for REPO in "${REPOS[@]}"; do
    CURRENT=$((CURRENT + 1))

    # Local dir name: org/model → org_model  (org/model::sub → org_model-sub)
    parse_entry "$REPO"
    DEST="$MODELS_DIR/$DIR_NAME"

    echo -e "${CYAN}----------------------------------------${NC}" | tee -a "$LOG_FILE"
    echo -e "[$CURRENT/$TOTAL] ${CYAN}$REPO${NC}" | tee -a "$LOG_FILE"

    # Skip if complete — check that index file exists and all shards are present
    if [ -d "$DEST" ] && [ "$(ls -A "$DEST" 2>/dev/null)" ]; then
        INCOMPLETE=false
        INDEX="$DEST/model.safetensors.index.json"
        if [ -f "$INDEX" ]; then
            EXPECTED=$(python3 -c "import json; d=json.load(open('$INDEX')); print(len(set(d.get('weight_map',{}).values())))" 2>/dev/null)
            ACTUAL=$(ls "$DEST"/model-*.safetensors 2>/dev/null | wc -l | tr -d ' ')
            if [ -n "$EXPECTED" ] && [ "$ACTUAL" -lt "$EXPECTED" ]; then
                INCOMPLETE=true
                echo -e "  ${YELLOW}INCOMPLETE${NC} — $ACTUAL/$EXPECTED shards, resuming..." | tee -a "$LOG_FILE"
            fi
        elif ls "$DEST"/model-*.safetensors >/dev/null 2>&1; then
            # Has shards but no index — likely incomplete
            INCOMPLETE=true
            echo -e "  ${YELLOW}INCOMPLETE${NC} — missing index file, resuming..." | tee -a "$LOG_FILE"
        fi
        if ! $INCOMPLETE; then
            SIZE=$(du -sh "$DEST" 2>/dev/null | cut -f1)
            echo -e "  ${YELLOW}SKIP${NC} — already complete ($SIZE)" | tee -a "$LOG_FILE"
            SKIPPED=$((SKIPPED + 1))
            continue
        fi
    fi

    echo "  → $DEST" | tee -a "$LOG_FILE"
    echo "  Started: $(date)" | tee -a "$LOG_FILE"

    if [ -n "$SUBDIR" ]; then
        "$HF" download "$REPO_ID" --include "$SUBDIR/*" --local-dir "$DEST" 2>&1 | tee -a "$LOG_FILE"
        DL_OK=${PIPESTATUS[0]}
    else
        "$HF" download "$REPO_ID" --local-dir "$DEST" 2>&1 | tee -a "$LOG_FILE"
        DL_OK=${PIPESTATUS[0]}
    fi

    if [ "$DL_OK" -eq 0 ]; then
        # Flatten repo subfolder into the model dir so it looks like a normal model
        if [ -n "$SUBDIR" ] && [ -d "$DEST/$SUBDIR" ]; then
            mv "$DEST/$SUBDIR"/* "$DEST"/ 2>/dev/null
            rmdir "$DEST/$SUBDIR" 2>/dev/null
        fi
        SIZE=$(du -sh "$DEST" 2>/dev/null | cut -f1)
        echo -e "  ${GREEN}✓ Done${NC} ($SIZE) at $(date)" | tee -a "$LOG_FILE"
    else
        echo -e "  ${RED}✗ FAILED${NC} at $(date)" | tee -a "$LOG_FILE"
        FAILED=$((FAILED + 1))
    fi
    echo "" | tee -a "$LOG_FILE"
done

SUCCEEDED=$((TOTAL - FAILED - SKIPPED))
echo -e "${CYAN}========================================${NC}" | tee -a "$LOG_FILE"
echo "Completed at $(date)" | tee -a "$LOG_FILE"
echo -e "Total: $TOTAL | Downloaded: ${GREEN}$SUCCEEDED${NC} | Skipped: ${YELLOW}$SKIPPED${NC} | Failed: ${RED}$FAILED${NC}" | tee -a "$LOG_FILE"
echo -e "${CYAN}========================================${NC}" | tee -a "$LOG_FILE"
