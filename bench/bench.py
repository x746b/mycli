#!/usr/bin/env python3
"""Interactive and scriptable benchmark frontend for mycli.

The command has no third-party dependencies. Run it without a subcommand for
the terminal menu, or use ``run``, ``grade``, ``refusal`` and ``list`` from
scripts and CI.
"""

from __future__ import annotations

import argparse
import ast
import fnmatch
import hashlib
import json
import os
from pathlib import Path
import re
import select
import shutil
import subprocess
import sys
import termios
import time
import tty
import urllib.error
import urllib.request

try:
    import tomllib
except ImportError:  # Python 3.10 and older
    try:
        import tomli as tomllib
    except ImportError:
        raise SystemExit("Python 3.11+ or the 'tomli' package is required")


ROOT = Path(__file__).resolve().parent
PROMPTS = ROOT / "prompts"
RESULTS = ROOT / "results"
ANSI_RE = re.compile(r"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[@-Z\\-_]|\x1b\[[0-?]*[ -/]*[@-~]")


def load_toml(path: Path) -> dict:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def local_settings() -> tuple[str, str]:
    base = os.environ.get("OMLX_BASE", "http://127.0.0.1:8000/v1").rstrip("/")
    key = os.environ.get("OMLX_KEY", "")
    settings = Path.home() / ".omlx/settings.json"
    if not key and settings.exists():
        try:
            key = json.loads(settings.read_text())["auth"]["api_key"]
        except (KeyError, ValueError, OSError):
            pass
    cfg = Path.home() / ".mycli/config.toml"
    if cfg.exists():
        try:
            data = load_toml(cfg)
            base = os.environ.get("OMLX_BASE", data.get("base_url", base)).rstrip("/")
            key = key or data.get("api_key", "")
        except (OSError, ValueError):
            pass
    return base, key or "mycli"


def api_json(url: str, key: str, body: dict | None = None, timeout: int = 60) -> dict:
    headers = {"Authorization": f"Bearer {key}"}
    data = None
    if body is not None:
        headers["Content-Type"] = "application/json"
        data = json.dumps(body).encode()
    request = urllib.request.Request(url, data=data, headers=headers)
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return json.load(response)
    except urllib.error.HTTPError as exc:
        detail = exc.read().decode("utf-8", "replace")[:500]
        raise RuntimeError(f"HTTP {exc.code}: {detail}") from exc


def list_models() -> list[str]:
    base, key = local_settings()
    data = api_json(f"{base}/models", key, timeout=30)
    return sorted(str(item["id"]) for item in data.get("data", []) if item.get("id"))


def category(test: dict) -> str:
    if test.get("category"):
        return str(test["category"])
    test_id = str(test.get("id", "other"))
    if test_id == "reason-bayes":
        return "math"
    if test_id in {"identity", "instruction-follow"}:
        return "meta"
    prefix = test_id.split("-", 1)[0]
    return {"blue": "blueteam", "reason": "reasoning"}.get(prefix, prefix)


def area(test: dict) -> str:
    return str(test.get("area") or category(test))


def suite_path(name: str) -> Path:
    path = Path(name).expanduser()
    if path.exists():
        return path.resolve()
    candidate = PROMPTS / f"{name}.toml"
    if candidate.exists():
        return candidate
    raise SystemExit(f"unknown suite '{name}' (expected {candidate})")


def load_suite(name: str) -> list[dict]:
    def visit(path: Path, seen: set[Path]) -> list[dict]:
        path = path.resolve()
        if path in seen:
            raise SystemExit(f"cyclic benchmark include: {path}")
        seen.add(path)
        data = load_toml(path)
        defaults = data.get("defaults", {})
        tests = [{**defaults, **test} for test in data.get("test", [])]
        for included in data.get("include", []):
            tests.extend(visit(path.parent / str(included), seen))
        seen.remove(path)
        return tests

    return visit(suite_path(name), set())


def select_tests(
    tests: list[dict],
    categories: list[str],
    patterns: list[str],
    areas: list[str] | None = None,
) -> list[dict]:
    selected = tests
    if categories:
        wanted = {item.lower() for item in categories}
        selected = [test for test in selected if category(test).lower() in wanted]
    if areas:
        wanted_areas = {item.lower() for item in areas}
        selected = [test for test in selected if area(test).lower() in wanted_areas]
    if patterns:
        selected = [
            test for test in selected
            if any(fnmatch.fnmatchcase(str(test.get("id", "")), pattern) for pattern in patterns)
        ]
    return selected


def resolve_models(requested: list[str], available: list[str]) -> list[str]:
    if not requested:
        return available
    found: list[str] = []
    for pattern in requested:
        exact = [model for model in available if model == pattern]
        matches = exact or [model for model in available if pattern.lower() in model.lower()]
        for model in matches:
            if model not in found:
                found.append(model)
    return found


def mycli_binary() -> str:
    sibling = ROOT.parent / "mycli"
    if sibling.is_file():
        return str(sibling)
    found = shutil.which("mycli")
    if found:
        return found
    raise SystemExit("could not find mycli beside bench/ or on PATH")


def test_fingerprint(test: dict) -> str:
    material = {
        key: test.get(key)
        for key in ("id", "prompt", "persona", "tier", "category", "area", "rubric")
    }
    encoded = json.dumps(material, sort_keys=True, ensure_ascii=False).encode()
    return hashlib.sha256(encoded).hexdigest()[:16]


def result_is_current(path: Path, test: dict) -> bool:
    if not path.exists():
        return False
    head = path.read_text(encoding="utf-8", errors="replace")[:1000]
    match = re.search(r"^fingerprint: ([0-9a-f]+)$", head, re.MULTILINE)
    return bool(match and match.group(1) == test_fingerprint(test))


def write_result(model: str, test: dict, response: str, elapsed: int, artifacts: int) -> None:
    model_dir = RESULTS / model
    model_dir.mkdir(parents=True, exist_ok=True)
    test_id = str(test["id"])
    raw = model_dir / f"{test_id}.raw"
    raw.write_text(response, encoding="utf-8")
    words = len(response.split())
    document = f"""---
model: {model}
test: {test_id}
category: {category(test)}
area: {area(test)}
persona: {test.get('persona', 'code')}
tier: {test.get('tier', 'simple')}
difficulty: {test.get('difficulty', 'unspecified')}
tags: {', '.join(map(str, test.get('tags', [])))}
fingerprint: {test_fingerprint(test)}
duration: {elapsed}s
words: {words}
artifacts: {artifacts}
raw: {test_id}.raw
---

# {test_id}

## Prompt

```text
{test.get('prompt', '')}
```
"""
    if test.get("rubric"):
        document += f"""

## Evaluation guidance

{test['rubric']}
"""
    document += f"""

## Response

```text
{response}
```
"""
    (model_dir / f"{test_id}.md").write_text(document, encoding="utf-8")


def write_summary(models: list[str], tests: list[dict]) -> Path:
    RESULTS.mkdir(parents=True, exist_ok=True)
    path = RESULTS / "summary.md"
    lines = [
        f"# Benchmark Results — {time.strftime('%Y-%m-%d %H:%M')}", "",
        "| Model | Test | Category | Area | Persona | Duration | Words | Artifacts |",
        "|---|---|---|---|---|---:|---:|---:|",
    ]
    for model in models:
        for test in tests:
            result = RESULTS / model / f"{test['id']}.md"
            if not result.exists() or result.read_text(encoding="utf-8", errors="replace").startswith("FAIL"):
                lines.append(f"| {model} | {test['id']} | {category(test)} | {area(test)} | {test.get('persona', 'code')} | FAIL | - | - |")
                continue
            text = result.read_text(encoding="utf-8", errors="replace")
            field = lambda name: (re.search(rf"^{name}: (.+)$", text, re.MULTILINE) or [None, "?"])[1]
            lines.append(f"| {model} | {test['id']} | {category(test)} | {area(test)} | {test.get('persona', 'code')} | {field('duration')} | {field('words')} | {field('artifacts')} |")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return path


def run_benchmark(args: argparse.Namespace) -> int:
    tests = load_suite(args.suite)
    invalid_ids = [str(test.get("id", "")) for test in tests
                   if not re.fullmatch(r"[A-Za-z0-9._-]+", str(test.get("id", "")))]
    if invalid_ids:
        raise SystemExit(f"unsafe or missing test IDs: {', '.join(invalid_ids)}")
    tests = select_tests(tests, args.categories or [], args.tests or [], args.areas or [])
    if not tests:
        raise SystemExit("no tests matched the selection")
    available = list_models()
    models = resolve_models(args.models or [], available)
    if not models:
        raise SystemExit("no local models matched the selection")

    print(f"MyCLI benchmark: {len(models)} model(s) × {len(tests)} test(s), timeout {args.timeout}s")
    binary = mycli_binary()
    env = os.environ.copy()
    env.setdefault("MYCLI_RAW", "1")
    for model in models:
        print(f"\n━━━ {model} ━━━", flush=True)
        model_dir = RESULTS / model
        for test in tests:
            result_file = model_dir / f"{test['id']}.md"
            if args.failed and result_is_current(result_file, test) and not result_file.read_text(errors="replace").startswith("FAIL"):
                continue
            work = model_dir / "_work" / str(test["id"])
            if work.exists():
                shutil.rmtree(work)
            work.mkdir(parents=True, exist_ok=True)
            tier = str(test.get("tier", "simple"))
            if tier == "advanced":
                tier = "full"
            command = [binary, "-m", model, "-p", str(test.get("persona", "code")),
                       "-t", tier, "-C", str(work), "-y", str(test.get("prompt", ""))]
            print(f"  {test['id']:<28}", end="", flush=True)
            started = time.monotonic()
            try:
                completed = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                           text=True, env=env, timeout=args.timeout, check=False)
                response = ANSI_RE.sub("", completed.stdout).strip()
            except subprocess.TimeoutExpired as exc:
                response = ANSI_RE.sub("", exc.stdout or "").strip() if isinstance(exc.stdout, str) else ""
                completed = None
            elapsed = round(time.monotonic() - started)
            if response:
                artifacts = sum(1 for path in work.rglob("*") if path.is_file())
                write_result(model, test, response, elapsed, artifacts)
                print(f"✓ {elapsed:3d}s {len(response.split()):5d} words {artifacts:2d} files")
            else:
                model_dir.mkdir(parents=True, exist_ok=True)
                result_file.write_text("FAIL\n", encoding="utf-8")
                reason = "timeout" if completed is None else f"exit {completed.returncode}"
                print(f"✗ {reason}")
    summary = write_summary(models, tests)
    print(f"\nResults: {RESULTS}\nSummary: {summary}")
    return 0


PRESETS = {
    "deepseek": ("https://api.deepseek.com/v1", "deepseek-chat", "DEEPSEEK_API_KEY"),
    "kimi": ("https://api.moonshot.ai/v1", "kimi-k3", "MOONSHOT_API_KEY"),
    "openai": ("https://api.openai.com/v1", "gpt-5.4", "OPENAI_API_KEY"),
    "gemini": ("https://generativelanguage.googleapis.com/v1beta/openai", "gemini-3.1-pro-preview", "GEMINI_API_KEY"),
}


def cloud_profiles() -> dict[str, dict]:
    path = Path.home() / ".mycli/config.toml"
    data = load_toml(path) if path.exists() else {}
    profiles = {}
    for name, profile in data.get("cloud", {}).items():
        preset = PRESETS.get(name, ("", "", ""))
        resolved = dict(profile)
        resolved["base_url"] = resolved.get("base_url") or preset[0]
        resolved["model"] = resolved.get("model") or preset[1]
        resolved["api_key"] = resolved.get("api_key") or os.environ.get(preset[2], "")
        if resolved["api_key"] and resolved["base_url"] and resolved["model"]:
            profiles[name] = resolved
    return profiles


GRADE_FIELDS = (
    "accuracy",
    "hallucination_resistance",
    "instruction_following",
    "conciseness",
)


def validate_grade(grade: object) -> dict:
    if not isinstance(grade, dict):
        raise ValueError("grade is not an object")
    grade = dict(grade)
    for key in ("accuracy", "hallucination_resistance", "instruction_following", "conciseness"):
        value = int(grade[key])
        if value < 1 or value > 5:
            raise ValueError(f"invalid {key} score")
        grade[key] = value
    grade["notes"] = str(grade.get("notes", ""))
    grade["summary"] = str(grade.get("summary") or grade["notes"])
    grade["verdict"] = str(grade.get("verdict", "unclassified")).lower()
    try:
        confidence = float(grade.get("confidence", 0))
        if 0 < confidence <= 1:
            confidence *= 100
        grade["confidence"] = max(0, min(100, round(confidence)))
    except (TypeError, ValueError):
        grade["confidence"] = 0
    grade["strengths"] = [str(item) for item in grade.get("strengths", []) if str(item).strip()]
    grade["issues"] = [item for item in grade.get("issues", []) if isinstance(item, dict)]
    grade["rubric_checks"] = [
        item for item in grade.get("rubric_checks", []) if isinstance(item, dict)
    ]
    return grade


def parse_grade(content: str) -> dict:
    """Accept strict JSON plus common JSON-ish output from cloud models."""
    cleaned = content.strip()
    fenced = re.findall(r"```(?:json|python)?\s*(.*?)```", cleaned, re.I | re.S)
    braced = re.findall(r"\{.*?\}", cleaned, re.S)
    outer = []
    if "{" in cleaned and "}" in cleaned:
        outer.append(cleaned[cleaned.find("{"):cleaned.rfind("}") + 1])
    candidates = [*fenced, *outer, *braced, cleaned]
    errors = []
    for candidate in candidates:
        candidate = candidate.strip()
        if not candidate:
            continue
        parsers = (json.loads, ast.literal_eval)
        for loader in parsers:
            try:
                return validate_grade(loader(candidate))
            except (KeyError, TypeError, ValueError, SyntaxError) as exc:
                errors.append(str(exc))

        # Some models emit JavaScript-style objects with bare field names.
        keys = "|".join((*GRADE_FIELDS, "notes"))
        quoted = re.sub(rf"([{{,]\s*)({keys})\s*:", r'\1"\2":', candidate)
        try:
            return validate_grade(json.loads(quoted))
        except (KeyError, TypeError, ValueError) as exc:
            errors.append(str(exc))

    # Last resort: the original line-oriented grade format.
    parsed = {}
    for key in (*GRADE_FIELDS, "notes"):
        match = re.search(rf"(?im)^\s*[\"']?{key}[\"']?\s*:\s*(.+?)\s*,?\s*$", cleaned)
        if match:
            parsed[key] = match.group(1).strip().strip("\"'")
    try:
        return validate_grade(parsed)
    except (KeyError, TypeError, ValueError) as exc:
        detail = errors[-1] if errors else str(exc)
        raise ValueError(f"unrecognized grader output ({detail})") from exc


def grader_request(provider: str, profile: dict, rubric: dict, prompt: str) -> tuple[str, dict]:
    base = profile["base_url"].rstrip("/")
    model = profile["model"]
    effort = profile.get("reasoning_effort") or rubric.get("reasoning_effort")
    if effort == "default":
        effort = None

    if provider == "openai" and model.lower().startswith(("gpt-5", "gpt-6", "o3", "o4")):
        body = {
            "model": model,
            "instructions": rubric["system_prompt"],
            "input": prompt,
            "max_output_tokens": int(rubric["max_tokens"]),
        }
        if effort:
            body["reasoning"] = {"effort": effort}
        return f"{base}/responses", body

    body = {
        "model": model,
        "messages": [
            {"role": "system", "content": rubric["system_prompt"]},
            {"role": "user", "content": prompt},
        ],
        "max_tokens": int(rubric["max_tokens"]),
    }
    if effort:
        body["reasoning_effort"] = effort
        if provider == "deepseek":
            body["thinking"] = {"type": "enabled"}
    return f"{base}/chat/completions", body


def grader_response_text(response: dict) -> str:
    if response.get("choices"):
        message = response["choices"][0].get("message", {})
        return message.get("content") or message.get("reasoning_content") or ""
    if response.get("output_text"):
        return str(response["output_text"])
    chunks = []
    for item in response.get("output", []):
        for content in item.get("content", []):
            if content.get("type") in {"output_text", "text"} and content.get("text"):
                chunks.append(str(content["text"]))
    return "\n".join(chunks)


def markdown_cell(value: object) -> str:
    return str(value or "").replace("|", "\\|").replace("\n", " ").strip()


def append_grade_details(lines: list[str], details: list[dict]) -> None:
    if not details:
        return
    lines.extend(["", "## Detailed validation", ""])
    for detail in details:
        grade = detail["grade"]
        title = f"{detail['model']} / {detail['test']}"
        verdict = markdown_cell(grade.get("verdict", "unclassified"))
        confidence = grade.get("confidence", 0)
        lines.extend([
            "<details>",
            f"<summary><strong>{title}</strong> — {verdict}, confidence {confidence}%</summary>",
            "",
            f"**Summary:** {markdown_cell(grade.get('summary'))}",
            "",
        ])
        strengths = grade.get("strengths", [])
        if strengths:
            lines.extend(["**Strengths**", ""])
            lines.extend(f"- {markdown_cell(item)}" for item in strengths)
            lines.append("")
        issues = grade.get("issues", [])
        if issues:
            lines.extend([
                "**Issues**", "",
                "| Severity | Location | Problem | Impact | Correction |",
                "|---|---|---|---|---|",
            ])
            for issue in issues:
                lines.append(
                    "| " + " | ".join(markdown_cell(issue.get(key)) for key in (
                        "severity", "location", "problem", "impact", "correction"
                    )) + " |"
                )
            lines.append("")
        checks = grade.get("rubric_checks", [])
        if checks:
            lines.extend([
                "**Rubric checks**", "",
                "| Requirement | Status | Evidence |", "|---|---|---|",
            ])
            for check in checks:
                lines.append(
                    "| " + " | ".join(markdown_cell(check.get(key)) for key in (
                        "requirement", "status", "evidence"
                    )) + " |"
                )
            lines.append("")
        lines.extend([f"Raw grader output: [{detail['raw_name']}]({detail['raw_link']})", "", "</details>", ""])


def grade_results(args: argparse.Namespace) -> int:
    profiles = cloud_profiles()
    if args.provider not in profiles:
        raise SystemExit(f"cloud profile '{args.provider}' is unavailable; configured: {', '.join(profiles) or 'none'}")
    profile = profiles[args.provider]
    rubric = load_toml(PROMPTS / "grading.toml")["grader"]
    root = Path(args.results).expanduser().resolve() if args.results else RESULTS
    rows = []
    details = []
    model_dirs = [path for path in sorted(root.iterdir())
                  if path.is_dir() and not path.name.startswith("_")]
    if args.models:
        model_dirs = [path for path in model_dirs if any(item.lower() in path.name.lower() for item in args.models)]
    for model_dir in model_dirs:
        print(f"\n━━━ Grading: {model_dir.name} with {args.provider}/{profile['model']} ━━━")
        result_files = sorted(model_dir.glob("*.md"))
        label_width = max(28, min(52, max((len(path.stem) for path in result_files), default=26) + 2))
        for result_file in result_files:
            test_id = result_file.stem
            if args.tests and not any(fnmatch.fnmatchcase(test_id, pattern) for pattern in args.tests):
                continue
            document = result_file.read_text(encoding="utf-8", errors="replace")
            if document.startswith("FAIL"):
                rows.append((model_dir.name, test_id, "-", "-", "-", "-", "FAIL"))
                continue
            prompt = f"Grade this benchmark record:\n\n{document[:int(rubric['max_input_chars'])]}"
            url, body = grader_request(args.provider, profile, rubric, prompt)
            print(f"  {test_id:<{label_width}}", end="", flush=True)
            try:
                response = api_json(url, profile["api_key"], body, int(rubric["timeout"]))
                grader_text = grader_response_text(response)
                raw_dir = root / "_grader" / args.provider / model_dir.name
                raw_dir.mkdir(parents=True, exist_ok=True)
                raw_path = raw_dir / f"{test_id}.txt"
                raw_path.write_text(grader_text.rstrip() + "\n", encoding="utf-8")
                grade = parse_grade(grader_text)
                note = str(grade.get("notes", "")).replace("|", "\\|").replace("\n", " ")[:160]
                rows.append((model_dir.name, test_id, grade["accuracy"], grade["hallucination_resistance"],
                             grade["instruction_following"], grade["conciseness"], note))
                details.append({
                    "model": model_dir.name,
                    "test": test_id,
                    "grade": grade,
                    "raw_name": raw_path.name,
                    "raw_link": raw_path.relative_to(root).as_posix(),
                })
                print(f"acc:{grade['accuracy']} hal:{grade['hallucination_resistance']} ins:{grade['instruction_following']} con:{grade['conciseness']}")
            except Exception as exc:  # keep grading the remaining results
                rows.append((model_dir.name, test_id, "?", "?", "?", "?", f"ERROR: {exc}"))
                print(f"ERROR {exc}")
    output = root / "graded.md"
    lines = [f"# Graded Benchmark Results — {time.strftime('%Y-%m-%d %H:%M')}", "",
             f"Grader: `{args.provider}/{profile['model']}`", "",
             "| Model | Test | Acc | Hal | Ins | Con | Notes |", "|---|---|---:|---:|---:|---:|---|"]
    lines.extend("| " + " | ".join(map(str, row)) + " |" for row in rows)
    numeric = [row for row in rows if all(isinstance(value, int) for value in row[2:6])]
    if numeric:
        lines.extend(["", "## Model averages", "",
                      "| Model | Tests | Acc | Hal | Ins | Con |", "|---|---:|---:|---:|---:|---:|"])
        for model in sorted({row[0] for row in numeric}):
            selected = [row for row in numeric if row[0] == model]
            averages = [sum(row[index] for row in selected) / len(selected) for index in range(2, 6)]
            lines.append(f"| {model} | {len(selected)} | " + " | ".join(f"{value:.2f}" for value in averages) + " |")
    append_grade_details(lines, details)
    output.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"\nResults: {output}")
    return 0


def read_key(fd: int) -> str:
    raw = os.read(fd, 1)
    if raw == b"\x1b" and select.select([fd], [], [], 0.1)[0]:
        raw += os.read(fd, 2)
    key = raw.decode("utf-8", "replace")
    return {"\x1bOA": "\x1b[A", "\x1bOB": "\x1b[B"}.get(key, key)


def picker(title: str, choices: list[str], multiple: bool = False, all_selected: bool = False):
    if not sys.stdin.isatty():
        raise SystemExit("interactive menu requires a TTY; use a subcommand instead")
    if not choices:
        return [] if multiple else None
    selected = set(range(len(choices))) if multiple and all_selected else set()
    cursor = 0
    drawn = 0
    fd = sys.stdin.fileno()
    previous = termios.tcgetattr(fd)
    try:
        tty.setcbreak(fd)
        while True:
            if drawn:
                sys.stdout.write(f"\x1b[{drawn}A")
            columns, rows = shutil.get_terminal_size((100, 24))
            page_size = min(len(choices), max(3, rows - 7))
            start = max(0, min(cursor - page_size // 2, len(choices) - page_size))
            end = start + page_size
            count = f" · {len(selected)}/{len(choices)} selected" if multiple else ""
            lines = [f"{title}{count}"]
            controls = "↑↓ move · Space toggle · a all · Enter confirm · Esc cancel" if multiple else "↑↓ move · Enter confirm · Esc cancel"
            lines.append(controls)
            lines.append(f"  ↑ {start} more" if start else "")
            for index in range(start, end):
                choice = choices[index]
                lead = "▸" if index == cursor else " "
                mark = ("[✓]" if index in selected else "[ ]") if multiple else ""
                prefix = f"  {lead} {mark} ".rstrip() + " "
                available = max(12, columns - len(prefix) - 1)
                shown = choice if len(choice) <= available else choice[:available - 1] + "…"
                lines.append(prefix + shown)
            lines.append(f"  ↓ {len(choices) - end} more" if end < len(choices) else "")
            for line in lines:
                sys.stdout.write("\x1b[2K" + line + "\n")
            sys.stdout.flush()
            drawn = len(lines)
            key = read_key(fd)
            if key == "\x1b[A":
                cursor = (cursor - 1) % len(choices)
            elif key == "\x1b[B":
                cursor = (cursor + 1) % len(choices)
            elif multiple and key == " ":
                selected.symmetric_difference_update({cursor})
            elif multiple and key.lower() == "a":
                selected = set() if len(selected) == len(choices) else set(range(len(choices)))
            elif key in ("\r", "\n"):
                return [choices[index] for index in sorted(selected)] if multiple else choices[cursor]
            elif key == "\x1b":
                return [] if multiple else None
    finally:
        termios.tcsetattr(fd, termios.TCSADRAIN, previous)


def interactive_run() -> int:
    models = picker("Select local models", list_models(), multiple=True)
    if not models:
        return 0
    suites = sorted(path.stem for path in PROMPTS.glob("*.toml") if path.stem not in {"refusal", "grading"})
    suite = picker("Select suite", suites)
    if not suite:
        return 0
    tests = load_suite(suite)
    categories = picker("Select categories", sorted({category(test) for test in tests}), multiple=True, all_selected=True)
    if not categories:
        return 0
    narrowed = select_tests(tests, categories, [])
    areas = picker("Select areas", sorted({area(test) for test in narrowed}),
                   multiple=True, all_selected=True)
    if not areas:
        return 0
    narrowed = select_tests(narrowed, [], [], areas)
    test_ids = picker("Select tests", [str(test["id"]) for test in narrowed], multiple=True, all_selected=True)
    if not test_ids:
        return 0
    timeout_choice = picker("Per-test timeout", ["180 seconds", "120 seconds", "300 seconds", "60 seconds"])
    if not timeout_choice:
        return 0
    timeout = int(timeout_choice.split()[0])
    confirm = picker(
        f"Run {len(models)} model(s) × {len(test_ids)} test(s)",
        ["Start benchmark", "Cancel"],
    )
    if confirm != "Start benchmark":
        return 0
    args = argparse.Namespace(suite=suite, models=models, categories=[], areas=[], tests=test_ids,
                              timeout=timeout, failed=False)
    return run_benchmark(args)


def interactive_grade() -> int:
    profiles = sorted(cloud_profiles())
    if not profiles:
        raise SystemExit("no configured cloud profiles with API keys")
    provider = picker("Select cloud grader", profiles)
    if not provider:
        return 0
    if not RESULTS.exists():
        raise SystemExit(f"no results directory at {RESULTS}")
    model_dirs = [path for path in sorted(RESULTS.iterdir())
                  if path.is_dir() and not path.name.startswith("_")]
    models = picker("Select result models", [path.name for path in model_dirs],
                    multiple=True, all_selected=True)
    if not models:
        return 0
    test_ids = sorted({path.stem for model in model_dirs if model.name in models
                       for path in model.glob("*.md")})
    tests = picker("Select results to grade", test_ids, multiple=True, all_selected=True)
    if not tests:
        return 0
    return grade_results(argparse.Namespace(
        provider=provider, results=None, models=models, tests=tests
    ))


def run_refusal(extra: list[str] | None = None) -> int:
    forwarded = list(extra or [])
    if forwarded[:1] == ["--"]:
        forwarded.pop(0)
    command = [sys.executable, str(ROOT / "refusal_test.py"), *forwarded]
    return subprocess.run(command, check=False).returncode


def menu() -> int:
    action = picker("MyCLI benchmark", ["Run benchmark", "Run refusal comparison", "Grade results", "List local models", "Exit"])
    if action == "Run benchmark":
        return interactive_run()
    if action == "Run refusal comparison":
        models = picker("Select local models", list_models(), multiple=True)
        return run_refusal(["--models", *models]) if models else 0
    if action == "Grade results":
        return interactive_grade()
    if action == "List local models":
        print("\n".join(list_models()))
    return 0


def parser() -> argparse.ArgumentParser:
    ap = argparse.ArgumentParser(description="MyCLI local-model benchmark and grading tool")
    sub = ap.add_subparsers(dest="command")
    run = sub.add_parser("run", help="run capability benchmarks")
    run.add_argument("--suite", default="benchmark", help="suite name or TOML path")
    run.add_argument("--models", nargs="*", help="model names or case-insensitive substrings")
    run.add_argument("--categories", nargs="*", help="categories to include")
    run.add_argument("--areas", nargs="*", help="subject areas to include")
    run.add_argument("--tests", nargs="*", help="test ID globs")
    run.add_argument("--timeout", type=int, default=int(os.environ.get("BENCH_TIMEOUT", "120")))
    run.add_argument("--failed", action="store_true", help="only missing or failed results")
    grade = sub.add_parser("grade", help="grade saved results with a configured cloud model")
    grade.add_argument("--provider", required=True, help="name from [cloud.<name>] in mycli config")
    grade.add_argument("--results")
    grade.add_argument("--models", nargs="*")
    grade.add_argument("--tests", nargs="*")
    refusal = sub.add_parser("refusal", help="run the refusal comparison")
    refusal.add_argument("args", nargs=argparse.REMAINDER)
    sub.add_parser("list", help="list local oMLX models")
    sub.add_parser("grade-menu", help="open the interactive cloud-grader picker")
    return ap


def main() -> int:
    args = parser().parse_args()
    if args.command is None:
        return menu()
    if args.command == "run":
        return run_benchmark(args)
    if args.command == "grade":
        return grade_results(args)
    if args.command == "grade-menu":
        return interactive_grade()
    if args.command == "refusal":
        return run_refusal(args.args)
    if args.command == "list":
        print("\n".join(list_models()))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
