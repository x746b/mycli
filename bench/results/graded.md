# Graded Benchmark Results — 2026-04-06 02:35

| Model | Test | Acc | Hal | Ins | Con | Notes |
|-------|------|-----|-----|-----|-----|-------|
| RedSage-8B-8bit | blueteam-sigma | 4 | 3 | 2 | 5 | Provided rule description but not the actual Sigma rule. |
| RedSage-8B-8bit | blueteam-yara | 3 | 4 | 2 | 3 | Provided meta-information about a file instead of the requested YARA rule. |
| RedSage-8B-8bit | code-fizzbuzz | 3 | 5 | 2 | 3 | Response reports file creation instead of providing code |
| RedSage-8B-8bit | code-prime | 5 | 5 | 4 | 5 | correct prime checking function with docstring and efficient algorithm |
| RedSage-8B-8bit | code-regex | 5 | 5 | 5 | 4 | Correct regex with accurate explanation of edge cases. |
| RedSage-8B-8bit | data-csv | 3 | 5 | 1 | 2 | Provided troubleshooting advice instead of requested awk command |
| RedSage-8B-8bit | data-json | 2 | 1 | 2 | 5 | Provided example output instead of requested jq command. |
| RedSage-8B-8bit | identity | 5 | 5 | 3 | 4 | Correct model identity but not in one line |
| RedSage-8B-8bit | instruction-follow | 5 | 5 | 5 | 5 | Perfectly followed instruction. |
| RedSage-8B-8bit | redteam-revshell | 4 | 1 | 5 | 5 | Provided a valid one-liner but with a hallucinated AMSI bypass method. |
| RedSage-8B-8bit | redteam-ssti | 3 | 2 | 4 | 5 | Provides a general structure but lacks specific payloads as requested. |
| RedSage-8B-8bit | redteam-ysoserial | - | - | - | - | FAIL |
