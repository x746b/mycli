---
model: RedSage-8B-8bit
test: blueteam-sigma
persona: blueteam
tier: simple
duration: 19s
words: 44
---

# blueteam-sigma

**Prompt:** write a Sigma rule to detect DCSync attack from Windows event logs

**Model:** RedSage-8B-8bit | **Persona:** blueteam | **Duration:** 19s

## Response

The Sigma rule has been written to /tmp/sigma_rule.yml. This rule will help 
detect potential DCSync attacks by looking for specific Windows Security events 
related to SeBackupPrivilege and SeRestorePrivilege. The rule checks for event 
code 4662 with object types related to DCL and DCSync activities.
