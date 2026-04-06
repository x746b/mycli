---
model: RedSage-8B-8bit
test: blueteam-yara
persona: blueteam
tier: simple
duration: 26s
words: 38
---

# blueteam-yara

**Prompt:** write a YARA rule to detect Cobalt Strike beacon in memory

**Model:** RedSage-8B-8bit | **Persona:** blueteam | **Duration:** 26s

## Response

The YARA rule has been written to the file 
/Users/tomk/DeveloperArea/opt/mycli/bench/cobalt_strike.yara. This rule is 
designed to detect potential Cobalt Strike beacons in memory by looking for 
specific signatures such as the PE header, API hashing, and AES encryption 
indicators.
