---
model: RedSage-8B-8bit
test: code-prime
persona: code
tier: simple
duration: 19s
words: 56
---

# code-prime

**Prompt:** write a python function to check if a number is prime

**Model:** RedSage-8B-8bit | **Persona:** code | **Duration:** 19s

## Response

The file /Users/tomk/DeveloperArea/opt/mycli/bench/is_prime.py has been created 
with the following content:

def is_prime(n):
"""
Check if a number is prime.
:param n: Integer to check
:return: True if n is prime, False otherwise
"""
if n <= 1:
    return False
for i in range(2, int(n**0.5) + 1):
    if n % i == 0:
        return False
return True
