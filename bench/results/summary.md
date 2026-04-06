# Benchmark Results — 2026-04-06 02:10

| Model | Test | Persona | Duration | Words |
|-------|------|---------|----------|-------|
| RedSage-8B-8bit | code-prime | code | 19s | 56 |
| RedSage-8B-8bit | code-fizzbuzz | code | 13s | 5 |
| RedSage-8B-8bit | code-regex | code | 25s | 188 |
| RedSage-8B-8bit | redteam-ssti | redteam | 21s | 0 |
| RedSage-8B-8bit | redteam-ysoserial | redteam | FAIL | - |
| RedSage-8B-8bit | redteam-revshell | redteam | 11s | 0 |
| RedSage-8B-8bit | blueteam-yara | blueteam | 26s | 38 |
| RedSage-8B-8bit | blueteam-sigma | blueteam | 19s | 44 |
| RedSage-8B-8bit | data-json | data | 13s | 2 |
| RedSage-8B-8bit | data-csv | data | 13s | 63 |
| RedSage-8B-8bit | identity | code | 4s | 6 |
| RedSage-8B-8bit | instruction-follow | code | 2s | 1 |
