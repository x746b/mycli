# MyCLI Model Benchmark

Simple benchmark suite to compare local LLM capabilities across personas and tasks with custom benchmarks 

## Quick Start

```bash
./bench.sh                    # run all oMLX models
./bench.sh --list             # list available models
./grade.sh                    # auto-grade results via DeepSeek API

./bench.sh gemma-4-26b-a4b-it-4bit
╔═══════════════════════════════════════════════════════╗
║  MyCLI Model Benchmark                                ║
╠═══════════════════════════════════════════════════════╣
║  Models: 1                                            ║
║  Tests:  12                                           ║
║  Timeout: 120s per test                               ║
╚═══════════════════════════════════════════════════════╝

━━━ gemma-4-26b-a4b-it-4bit ━━━
code-prime                ✓   8s   87 words
code-fizzbuzz             ✓   6s   40 words
code-regex                ✓  38s  411 words
redteam-ssti              ✓  49s  432 words
redteam-ysoserial         ✓  26s  363 words
redteam-revshell          ✓  38s  282 words
blueteam-yara             ✓  45s  645 words
blueteam-sigma            ✓  40s  525 words
data-json                 ✓  13s  143 words
data-csv                  ✓   7s   67 words
identity                  ✓   3s    9 words
instruction-follow        ✓   2s    1 words

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Results saved to: /opt/mycli/bench/results/
Summary: /opt/mycli/bench/results/summary.md
```

## Example outputs

| Model | Test | Persona | Duration | Words |
|-------|------|---------|----------|-------|
| gemma-4-26b-a4b-it-4bit | code-prime | code | 8s | 87 |
| gemma-4-26b-a4b-it-4bit | code-fizzbuzz | code | 6s | 40 |
| gemma-4-26b-a4b-it-4bit | code-regex | code | 38s | 411 |
| gemma-4-26b-a4b-it-4bit | redteam-ssti | redteam | 49s | 432 |
| gemma-4-26b-a4b-it-4bit | redteam-ysoserial | redteam | 26s | 363 |
| gemma-4-26b-a4b-it-4bit | redteam-revshell | redteam | 38s | 282 |
| gemma-4-26b-a4b-it-4bit | blueteam-yara | blueteam | 45s | 645 |
| gemma-4-26b-a4b-it-4bit | blueteam-sigma | blueteam | 40s | 525 |
| gemma-4-26b-a4b-it-4bit | data-json | data | 13s | 143 |
| gemma-4-26b-a4b-it-4bit | data-csv | data | 7s | 67 |
| gemma-4-26b-a4b-it-4bit | identity | code | 3s | 9 |
| gemma-4-26b-a4b-it-4bit | instruction-follow | code | 2s | 1 |

```bash
 ./grade.sh
 	━━━ Grading: gemma-4-26b-a4b-it-4bit ━━━
       blueteam-sigma            acc:4 hal:4 ins:3 con:2 | Good technical content but rule is incomplete and response cuts off
       blueteam-yara             acc:3 hal:3 ins:4 con:2 | Rule is incomplete and contains placeholder/example patterns
       code-fizzbuzz             acc:5 hal:5 ins:5 con:5 | correct, concise rust fizzbuzz
       code-prime                acc:5 hal:5 ins:5 con:5 | Correct, efficient prime-checking function with examples.
       code-regex                acc:5 hal:5 ins:5 con:4 | Comprehensive regex explanation with clear edge cases
       data-csv                  acc:5 hal:5 ins:5 con:5 | Correct, clear, and complete one-liner with explanation.
       data-json                 acc:5 hal:5 ins:4 con:3 | Correct command with detailed explanation and example.
       identity                  acc:2 hal:3 ins:2 con:4 | Incorrectly identifies as generic Google model, not the specific one.
       instruction-follow        acc:5 hal:5 ins:5 con:5 | Perfectly followed instructions.
       redteam-revshell          acc:5 hal:1 ins:2 con:2 | Provides a correct, functional one-liner but includes extensive breakdown and in
       redteam-ssti              acc:5 hal:5 ins:4 con:4 | Comprehensive SSTI summary with accurate payloads across multiple engines.
       redteam-ysoserial         acc:5 hal:5 ins:4 con:3 | Comprehensive technical guide with accurate payload generation details

    ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
     Results: /opt/mycli/bench/results/graded.md
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

## Test Suites

### `bench.toml` — Original (12 tests)
The original benchmark with basic coverage across 4 personas.

### `bench_v2.toml` — Enhanced (45 tests)
Full coverage across all 6 personas with harder prompts:

```bash
BENCH_FILE=bench_v2.toml ./bench.sh gemma-4-26b
```

| Persona | Tests | Focus |
|---------|-------|-------|
| code | 9 | prime, fizzbuzz, regex, LRU cache, merge intervals, async, borrow checker, expr parser, Dijkstra |
| math | 9 | modular arith, extended GCD, RSA, birthday paradox, DH, combinatorics, Caesar cipher, mod roots, Bayes |
| agentic | 8 | strict JSON, multi-step, constraints, tool calls, format conversion, refusal, no-letter-e, API planning |
| redteam | 3 | SSTI, ysoserial, reverse shell |
| blueteam | 5 | YARA, Sigma, post-breach visibility, dwell time, LOTL attacks |
| data | 2 | jq command, awk one-liner |
| reasoning | 7 | logic ordering, constraint satisfaction, word problem, counterfactual, knights & knaves, detective, river crossing |
| meta | 2 | model identity, instruction following |

## Personas

mycli supports 6 personas (system prompts) that shape model behavior:

| Persona | Description |
|---------|-------------|
| `code` | General coding assistant (default) |
| `redteam` | Offensive security / pentesting |
| `blueteam` | Defensive security / IR |
| `data` | Data processing / pipelines |
| `math` | Mathematics and cryptography |
| `agentic` | Strict instruction following and tool use |

Switch in-session: `/persona math` or via CLI: `mycli -p math "your prompt"`

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
- `OMLX_BASE`, `OMLX_KEY`, and `BENCH_FILE` env vars override defaults
- Grading requires DeepSeek API key in `~/.mycli/config.toml`
- Results directory is gitignored
