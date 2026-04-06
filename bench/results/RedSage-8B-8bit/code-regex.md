---
model: RedSage-8B-8bit
test: code-regex
persona: code
tier: simple
duration: 25s
words: 188
---

# code-regex

**Prompt:** write a regex that matches IPv4 addresses, explain edge cases

**Model:** RedSage-8B-8bit | **Persona:** code | **Duration:** 25s

## Response

Sure, here's a regex pattern to match IPv4 addresses:

\b((25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(25[0-5]|2[0-4][0-9]|[01]?[0-9][0
-9]?)\b

Explanation:
- \b: Word boundary to ensure the IP address is a standalone word.
- (25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?): Matches numbers from 0 to 255.
  - 25[0-5]: Matches 250-255.
  - 2[0-4][0-9]: Matches 200-249.
  - [01]?[0-9][0-9]?: Matches 0-199, allowing for optional leading zeros.
- \.: Matches the dot separator between octets.
- {3}: Ensures the preceding pattern (three octets with dots) repeats exactly 
three times.
- Final octet: Matches the last octet (0-255) without a trailing dot.

Edge Cases:
1. Leading Zeros: Numbers like 01 are allowed in the regex but are valid as long
as they are within the range of 0-255.
2. Single Octet: The regex does not match IPv4 addresses with fewer than four 
octets, like 192.168.1, as this is not a valid IPv4 format.
3. Non-Digit Characters: The regex will not match IP addresses containing 
letters, symbols, or spaces.
4. Overflow Values: Numbers greater than 255, such as 256 or 300, are not 
allowed.

This regex ensures that the IP address is in the standard dotted decimal format 
and adheres to the valid range for each octet.
