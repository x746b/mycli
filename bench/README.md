# MyCLI Model Benchmark

Simple benchmark suite to compare local LLM capabilities across personas and tasks.

## Quick Start

```bash
./bench.sh                    # run all oMLX models
./bench.sh WhiteRabbit        # filter by model name
./bench.sh --list             # list available models
./grade.sh                    # auto-grade results via DeepSeek API
```

## How It Works

**`bench.sh`** runs mycli in single-shot mode for each model × test combination:
- Discovers models from oMLX (`/v1/models`)
- Runs each prompt from `bench.toml` with the specified persona and tool tier
- Captures output with metadata (duration, word count)
- Saves per-model results to `results/<model>/<test-id>.md`
- Generates `results/summary.md`

**`grade.sh`** sends each result to DeepSeek for automated scoring (1-5):
- **accuracy** — is the technical content correct?
- **hallucination** — does it make things up?
- **instruction_following** — did it answer what was asked?
- **conciseness** — is it appropriately brief?

Outputs a graded table to `results/graded.md`.

## Test Prompts

Defined in `bench.toml`. 12 tests across all 4 personas:

| Persona | Tests |
|---------|-------|
| code | prime function, fizzbuzz, regex |
| redteam | SSTI payloads, ysoserial, reverse shell |
| blueteam | YARA rule, Sigma rule |
| data | jq command, awk one-liner |
| meta | model identity, instruction following |

## Adding Tests

```toml
[[test]]
id = "my-test"
persona = "redteam"
tier = "simple"
prompt = "your prompt here"
```

## Environment

- Requires oMLX running locally (default `http://127.0.0.1:8000/v1`)
- `OMLX_BASE` and `OMLX_KEY` env vars override defaults
- Grading requires DeepSeek API key in `~/.mycli/config.toml`
- Results directory is gitignored
