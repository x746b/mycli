# HF Tools

HuggingFace CLI tools for searching and downloading models.

## Setup

Needs the HuggingFace CLI:

```bash
pip install huggingface_hub        # provides the `hf` command
export HF_TOKEN=hf_your_token      # optional; required for gated repos
```

## Download Models

Edit `to-download.txt` with one HuggingFace repo per line:

```
mlx-community/NVIDIA-Nemotron-3-Super-120B-A12B-5bit
mlx-community/some-other-model-4bit
```

Some repos pack several quants into one repo as subfolders. Append `::<subfolder>`
to fetch only that one instead of the whole repo:

```
orcarouter/Qwen3.8-27B-Uncensored-MLX::8-bit
```

The subfolder's contents are flattened into `~/AI/models/<org>_<model>-<subfolder>/`,
so the result is a normal model directory your inference server can load directly.
`--info` reports the subfolder's size, not the full repo's.

Then run:

```bash
./hf-download.sh                  # download all from to-download.txt
./hf-download.sh --info           # show sizes and disk free before downloading
./hf-download.sh --dry-run        # preview what would be downloaded (no changes)
./hf-download.sh mylist.txt       # use a custom list file
./hf-download.sh --info mylist.txt
```

Models are saved to `~/AI/models/<org>_<model>/` (slash replaced with underscore).
Existing models are skipped automatically. Log is written to `~/AI/models/download.log`.

## Search & Info

Use the `hf` CLI directly:

```bash
# Search for models
hf models list --search "Nemotron MLX" --sort downloads --limit 10

# Get model info (size, downloads, config)
hf models info mlx-community/NVIDIA-Nemotron-3-Super-120B-A12B-5bit

# Download a single model manually
hf download mlx-community/some-model --local-dir ~/AI/models/some-model
```

## Notes

- Fast parallel transfers come from the `hf-xet` backend, used automatically by
  `huggingface_hub` (it superseded `hf_transfer`; no env var needed)
- HF cache is stored at `~/AI/models/.cache/huggingface`
- Set `HF_TOKEN` in your environment; the script never hardcodes a token
