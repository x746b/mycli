#!/usr/bin/env python3
"""Parse a bench TOML file into per-test prompt files plus a TSV manifest.

Usage: parse_bench.py <bench.toml> <prompt_dir>

Writes <prompt_dir>/<index>.txt for each test's full prompt and prints
"index<TAB>id<TAB>persona<TAB>tier" per test on stdout.

Uses tomllib when available (Python >= 3.11). Falls back to a small parser
covering the subset this bench file uses: [[test]] tables with single-line
"..." values and multi-line \"\"\"...\"\"\" values. The fallback exists because
macOS ships Python 3.9, which has no tomllib.
"""

import os
import re
import sys


def load_with_tomllib(path):
    try:
        import tomllib
    except ImportError:
        return None
    with open(path, "rb") as fh:
        return tomllib.load(fh).get("test", [])


def load_fallback(path):
    """Minimal [[test]] parser that understands triple-quoted strings."""
    with open(path, "r", encoding="utf-8") as fh:
        text = fh.read()

    tests = []
    current = None
    i = 0
    line_start = True
    # Walk line by line, but consume triple-quoted values greedily so that
    # embedded newlines never terminate a value (the original bug).
    lines = text.splitlines(keepends=True)
    idx = 0
    while idx < len(lines):
        line = lines[idx]
        stripped = line.strip()
        idx += 1

        if stripped == "[[test]]":
            if current is not None:
                tests.append(current)
            current = {}
            continue
        if current is None or not stripped or stripped.startswith("#"):
            continue
        if "=" not in stripped:
            continue

        key, _, val = stripped.partition("=")
        key = key.strip()
        val = val.strip()

        if val.startswith('"""'):
            rest = val[3:]
            if rest.endswith('"""') and len(rest) >= 3:
                current[key] = rest[:-3]
                continue
            # `stripped` dropped the newline that followed the opening quotes,
            # so put it back; TOML trims only a newline immediately after \"\"\".
            chunks = [rest + "\n"] if rest else []
            while idx < len(lines):
                nxt = lines[idx]
                idx += 1
                end = nxt.find('"""')
                if end != -1:
                    chunks.append(nxt[:end])
                    break
                chunks.append(nxt)
            current[key] = "".join(chunks).rstrip("\n")
        else:
            current[key] = val.strip().strip('"').strip("'")

    if current is not None:
        tests.append(current)
    return tests


def main():
    if len(sys.argv) != 3:
        sys.exit("usage: parse_bench.py <bench.toml> <prompt_dir>")
    bench_file, prompt_dir = sys.argv[1], sys.argv[2]

    tests = load_with_tomllib(bench_file)
    if tests is None:
        tests = load_fallback(bench_file)

    if not tests:
        sys.exit("no [[test]] entries found in " + bench_file)

    for i, t in enumerate(tests):
        tid = t.get("id", "test-%d" % i)
        prompt = t.get("prompt", "")
        if not prompt.strip():
            print("warning: %s has an empty prompt" % tid, file=sys.stderr)
        with open(os.path.join(prompt_dir, "%d.txt" % i), "w", encoding="utf-8") as fh:
            fh.write(prompt)
        # These metadata fields are short identifiers; tabs/newlines cannot occur.
        # `lines` lets bench.sh select the multi-line prompts that the old
        # parser used to truncate.
        lines = prompt.strip().count("\n") + 1 if prompt.strip() else 0
        print("\t".join([str(i), tid, t.get("persona", "code"),
                         t.get("tier", "simple"), str(lines)]))


if __name__ == "__main__":
    main()
