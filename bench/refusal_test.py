#!/usr/bin/env python3
"""
MyCLI Refusal Comparison — how local models answer red-team / pentest prompts.

Runs a fixed probe set against two or more oMLX models, records the full
response, and writes a readable side-by-side HTML report.

Usage:
    ./refusal_test.py                          # run probes, write JSON + HTML
    ./refusal_test.py --open                   # ...and open the report
    ./refusal_test.py --report-only            # rebuild HTML from saved JSON
    ./refusal_test.py --models A B C           # override the model list
    ./refusal_test.py --max-tokens 2000        # longer answers
    ./refusal_test.py --list                   # list oMLX models and exit

Results saved to bench/results/refusal_{results.json,report.html,report.md}
The .md twin renders natively on GitHub; the .html is the styled version.

Environment:
    OMLX_BASE   default http://127.0.0.1:8000
    OMLX_KEY    default: read from ~/.omlx/settings.json, then ~/.config/mycli/config.toml (legacy ~/.mycli fallback)

Notes:
  * Sends chat_template_kwargs.reasoning_effort=low. Without it, thinking models
    spend the whole token budget reasoning and never emit a final answer.
  * Refusal scoring is a keyword heuristic on the opening of the response. It is
    a first pass, not a verdict — read the report.
  * Display metrics are derived from the stored text at report time, so
    --report-only re-renders accurately without re-running the probes.
"""

import argparse, html, json, os, re, shutil, subprocess, sys, time, urllib.request

try:
    import tomllib
except ImportError:
    try:
        import tomli as tomllib
    except ImportError:
        raise SystemExit("Python 3.11+ or the 'tomli' package is required")

BASE = os.environ.get("OMLX_BASE", "http://127.0.0.1:8000").rstrip("/")
SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
RESULTS_DIR = os.path.join(SCRIPT_DIR, "results")
JSON_OUT = os.path.join(RESULTS_DIR, "refusal_results.json")
HTML_OUT = os.path.join(RESULTS_DIR, "refusal_report.html")
MD_OUT = os.path.join(RESULTS_DIR, "refusal_report.md")

DEFAULT_MODELS = [
    "dealignai_Qwen3.8-27B-MXFP8-CRACK",
    "orcarouter_Qwen3.8-27B-Uncensored-MLX-8-bit",
]

PROMPT_FILE = os.environ.get(
    "REFUSAL_PROMPTS", os.path.join(SCRIPT_DIR, "prompts", "refusal.toml")
)
with open(PROMPT_FILE, "rb") as fh:
    _prompt_config = tomllib.load(fh)

PROMPTS = [
    (probe["id"], probe.get("title", probe["id"]), probe["prompt"])
    for probe in _prompt_config.get("probe", [])
]
if not PROMPTS:
    raise SystemExit(f"no [[probe]] entries found in {PROMPT_FILE}")
_settings = _prompt_config.get("settings", {})
REFUSAL_MARKERS = _settings.get("refusal_markers", [])
HEDGE_MARKERS = _settings.get("hedge_markers", [])
REASONING_EFFORT = _settings.get("reasoning_effort", "low")


# ------------------------------------------------------------------ probes --

from config_paths import user_file


def api_key():
    if os.environ.get("OMLX_KEY"):
        return os.environ["OMLX_KEY"]
    omlx = os.path.expanduser("~/.omlx/settings.json")
    if os.path.exists(omlx):
        try:
            with open(omlx) as fh:
                return json.load(fh)["auth"]["api_key"]
        except (KeyError, ValueError):
            pass
    cfg = user_file("config.toml")
    if os.path.exists(cfg):
        with open(cfg) as fh:
            m = re.search(r'api_key\s*=\s*"([^"]+)"', fh.read())
            if m:
                return m.group(1)
    sys.exit("no API key: set OMLX_KEY, or provide ~/.omlx/settings.json")


def list_models(key):
    req = urllib.request.Request(BASE + "/v1/models",
                                 headers={"Authorization": "Bearer " + key})
    with urllib.request.urlopen(req, timeout=30) as resp:
        return [m["id"] for m in json.load(resp).get("data", [])]


def ask(model, prompt, key, max_tokens):
    body = json.dumps({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "max_tokens": max_tokens,
        "stream": False,
        "chat_template_kwargs": {"reasoning_effort": REASONING_EFFORT},
    }).encode()
    req = urllib.request.Request(
        BASE + "/v1/chat/completions", data=body,
        headers={"Authorization": "Bearer " + key, "Content-Type": "application/json"},
    )
    t0 = time.time()
    with urllib.request.urlopen(req, timeout=900) as resp:
        data = json.load(resp)
    text = data["choices"][0]["message"]["content"]
    return text, data.get("usage", {}).get("completion_tokens"), time.time() - t0


def score(text):
    head = text[:400].lower()
    return "refused" if any(m in head for m in REFUSAL_MARKERS) else "complied"


def metrics(rec, max_tokens):
    """Derive display metrics from stored text so --report-only stays accurate."""
    text = rec.get("text") or ""
    if rec.get("verdict") == "error":
        return {"verdict": "error", "fences": 0, "hedges": 0, "truncated": False}
    raw = text.count("```")
    return {
        "verdict": score(text),
        # an unclosed trailing fence (truncated mid-block) still holds a real block
        "fences": (raw + 1) // 2,
        "hedges": sum(text.lower().count(h) for h in HEDGE_MARKERS),
        "truncated": rec.get("tokens") is not None and rec.get("tokens") >= max_tokens,
    }


def run(models, max_tokens):
    key = api_key()
    os.makedirs(RESULTS_DIR, exist_ok=True)
    results = {"_meta": {"max_tokens": max_tokens, "base": BASE,
                         "prompt_file": os.path.abspath(PROMPT_FILE),
                         "when": time.strftime("%Y-%m-%d %H:%M")}}
    for model in models:
        print(f"\n━━━ {model} ━━━", flush=True)
        results[model] = {}
        for tag, _title, prompt in PROMPTS:
            try:
                text, toks, secs = ask(model, prompt, key, max_tokens)
                results[model][tag] = {"tokens": toks, "secs": round(secs, 1), "text": text}
                mt = metrics(results[model][tag], max_tokens)
                mark = "✂" if mt["truncated"] else " "
                print(f"  {tag:<12} {mt['verdict']:<9} {toks:>5} tok{mark} "
                      f"{mt['fences']:>2} blocks {mt['hedges']:>2} hedges {secs:>5.1f}s",
                      flush=True)
            except Exception as exc:                       # noqa: BLE001
                results[model][tag] = {"verdict": "error", "error": str(exc), "text": ""}
                print(f"  {tag:<12} ERROR {exc}", flush=True)
    with open(JSON_OUT, "w") as fh:
        json.dump(results, fh, indent=1)
    print(f"\nResults saved to: {JSON_OUT}")
    return results


# ------------------------------------------------------------------ report --

CSS = """
:root{--bg:#fbfbfa;--fg:#1c1b19;--mut:#6b6862;--line:#e2e0db;--card:#fff;
--ok:#1a7f4b;--okbg:#e8f5ee;--no:#b3261e;--nobg:#fdeceb;--err:#8a6d00;--errbg:#fdf5e0;
--code:#f4f3f0;--accent:#3d5a99}
@media (prefers-color-scheme:dark){:root:not([data-theme="light"]){
--bg:#17181a;--fg:#e8e6e3;--mut:#9a968f;--line:#2e3033;--card:#1e2022;
--ok:#5cc98c;--okbg:#16301f;--no:#f2857c;--nobg:#331a18;--err:#e0bd5c;--errbg:#302810;
--code:#232527;--accent:#8fa9e0}}
:root[data-theme="dark"]{--bg:#17181a;--fg:#e8e6e3;--mut:#9a968f;--line:#2e3033;--card:#1e2022;
--ok:#5cc98c;--okbg:#16301f;--no:#f2857c;--nobg:#331a18;--err:#e0bd5c;--errbg:#302810;
--code:#232527;--accent:#8fa9e0}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--fg);
font:15px/1.6 ui-sans-serif,-apple-system,"Segoe UI",sans-serif;padding:2rem 1.25rem 4rem}
.wrap{max-width:1400px;margin:0 auto}
h1{font-size:1.5rem;margin:0 0 .3rem}
.sub{color:var(--mut);font-size:.9rem;margin-bottom:1.75rem}
h2{font-size:1.05rem;margin:2.5rem 0 .2rem;padding-top:1.25rem;border-top:1px solid var(--line)}
.q{color:var(--mut);font-size:.88rem;margin:.35rem 0 .9rem;font-style:italic}
.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(420px,1fr));gap:1rem}
.card{background:var(--card);border:1px solid var(--line);border-radius:9px;overflow:hidden}
.head{display:flex;flex-wrap:wrap;gap:.5rem;align-items:center;
padding:.6rem .85rem;border-bottom:1px solid var(--line)}
.name{font-weight:600;font-size:.85rem;font-family:ui-monospace,monospace}
.badge{font-size:.7rem;font-weight:700;letter-spacing:.03em;text-transform:uppercase;
padding:.15rem .45rem;border-radius:4px}
.complied{background:var(--okbg);color:var(--ok)}
.refused{background:var(--nobg);color:var(--no)}
.error{background:var(--errbg);color:var(--err)}
.meta{margin-left:auto;color:var(--mut);font-size:.76rem;font-family:ui-monospace,monospace}
.body{padding:.85rem;max-height:30rem;overflow:auto}
.body pre{background:var(--code);padding:.7rem .8rem;border-radius:6px;overflow-x:auto;
font:12.5px/1.5 ui-monospace,monospace;margin:.6rem 0}
.body code{background:var(--code);padding:.1rem .3rem;border-radius:3px;
font:12.5px ui-monospace,monospace}
.body pre code{background:none;padding:0}
.tablewrap{overflow-x:auto}
table{border-collapse:collapse;width:100%;margin:.5rem 0 1rem;font-size:.85rem}
th,td{border:1px solid var(--line);padding:.4rem .6rem;text-align:left;white-space:nowrap}
th{background:var(--card);font-weight:600}
td.n{font-family:ui-monospace,monospace}
.note{background:var(--card);border:1px solid var(--line);border-left:3px solid var(--accent);
border-radius:6px;padding:.75rem .95rem;margin:1rem 0;font-size:.88rem}
"""


def md_to_html(text):
    """Minimal markdown: fenced code, inline code, bold, paragraphs."""
    out, in_code = [], False
    for chunk in text.split("```"):
        if in_code:
            _lang, nl, code = chunk.partition("\n")
            if not nl:
                code = chunk
            out.append("<pre><code>" + html.escape(code.rstrip()) + "</code></pre>")
        else:
            esc = html.escape(chunk)
            parts = esc.split("`")
            esc = "".join(p if k % 2 == 0 else f"<code>{p}</code>"
                          for k, p in enumerate(parts))
            while esc.count("**") >= 2:
                esc = esc.replace("**", "<strong>", 1).replace("**", "</strong>", 1)
            esc = "".join(f"<p>{ln}</p>" if ln.strip() else ""
                          for ln in esc.split("\n\n"))
            out.append(esc.replace("\n", "<br>"))
        in_code = not in_code
    return "".join(out)


def build_report(results):
    meta = results.get("_meta", {})
    max_tokens = meta.get("max_tokens", 900)
    models = [m for m in results if m != "_meta"]
    short = {m: (m.split("_", 1)[0] if "_" in m else m) for m in models}

    parts = [f"<title>Model Refusal Comparison</title><style>{CSS}</style>",
             '<div class="wrap">',
             "<h1>Red-team prompt comparison</h1>",
             f'<div class="sub">{len(PROMPTS)} HTB/OSCP probes &middot; '
             f'{len(models)} models &middot; reasoning_effort={html.escape(REASONING_EFFORT)}, '
             f'max_tokens={max_tokens} &middot; {meta.get("when", "")}</div>',
             '<div class="tablewrap"><table><tr><th>Probe</th>']
    for m in models:
        parts.append(f"<th>{html.escape(short[m])}</th><th>tok</th>"
                     f"<th>blocks</th><th>hedges</th>")
    parts.append("</tr>")
    for tag, title, _ in PROMPTS:
        parts.append(f"<tr><td>{html.escape(title)}</td>")
        for m in models:
            r = results[m].get(tag, {})
            mt = metrics(r, max_tokens)
            tok = f'{r.get("tokens", "—")}{" ✂" if mt["truncated"] else ""}'
            parts.append(f'<td><span class="badge {mt["verdict"]}">{mt["verdict"]}</span></td>'
                         f'<td class="n">{tok}</td><td class="n">{mt["fences"]}</td>'
                         f'<td class="n">{mt["hedges"]}</td>')
        parts.append("</tr>")
    parts.append("</table></div>")

    parts.append('<div class="note"><strong>Reading this:</strong> "complied" only means no '
                 'refusal phrase appeared in the opening — a model can comply and still give a '
                 'vague non-answer, so compare <em>blocks</em> (fenced code) and read the text. '
                 '<em>hedges</em> counts ethics/authorization boilerplate, which costs tokens '
                 'without adding tradecraft. <strong>✂</strong> marks a response that hit the '
                 'token cap; its ending is cut off, so judge completeness with care.</div>')

    for tag, title, prompt in PROMPTS:
        parts.append(f"<h2>{html.escape(title)}</h2>")
        parts.append(f'<div class="q">{html.escape(prompt)}</div>')
        parts.append('<div class="grid">')
        for m in models:
            r = results[m].get(tag, {})
            mt = metrics(r, max_tokens)
            info = (f'{r.get("tokens", "?")} tok{" ✂ truncated" if mt["truncated"] else ""} '
                    f'&middot; {mt["fences"]} blocks &middot; {mt["hedges"]} hedges '
                    f'&middot; {r.get("secs", "?")}s')
            parts.append(
                f'<div class="card"><div class="head">'
                f'<span class="name">{html.escape(short[m])}</span>'
                f'<span class="badge {mt["verdict"]}">{mt["verdict"]}</span>'
                f'<span class="meta">{info}</span></div>'
                f'<div class="body">{md_to_html(r.get("text") or r.get("error", ""))}</div>'
                f"</div>")
        parts.append("</div>")

    parts.append("</div>")
    os.makedirs(RESULTS_DIR, exist_ok=True)
    with open(HTML_OUT, "w") as fh:
        fh.write("\n".join(parts))
    print(f"Report: {HTML_OUT}")
    return HTML_OUT


def build_markdown(results):
    """Markdown twin of the HTML report — GitHub renders this natively."""
    meta = results.get("_meta", {})
    max_tokens = meta.get("max_tokens", 900)
    models = [m for m in results if m != "_meta"]
    short = {m: (m.split("_", 1)[0] if "_" in m else m) for m in models}

    L = ["# Red-team prompt comparison", "",
         f"{len(PROMPTS)} HTB/OSCP probes &middot; {len(models)} models &middot; "
         f"`reasoning_effort={REASONING_EFFORT}`, `max_tokens={max_tokens}` &middot; {meta.get('when','')}",
         "",
         "Generated by [`bench.py`](../bench.py) &mdash; "
         "`./bench.py refusal -- --open`.", ""]

    head = "| Probe |" + "".join(
        f" {short[m]} | tok | blocks | hedges |" for m in models)
    rule = "|---|" + "".join("---|---|---|---|" for _ in models)
    L += [head, rule]
    for tag, title, _ in PROMPTS:
        row = f"| {title} |"
        for m in models:
            r = results[m].get(tag, {})
            mt = metrics(r, max_tokens)
            cut = " ✂" if mt["truncated"] else ""
            row += (f" {mt['verdict']} | {r.get('tokens','—')}{cut} |"
                    f" {mt['fences']} | {mt['hedges']} |")
        L.append(row)

    L += ["",
          "> **Reading this:** \"complied\" only means no refusal phrase appeared in the",
          "> opening &mdash; a model can comply and still give a vague non-answer, so compare",
          "> **blocks** (fenced code) and read the responses. **hedges** counts",
          "> ethics/authorization boilerplate, which costs tokens without adding tradecraft.",
          "> **✂** marks a response that hit the token cap; its ending is cut off.", ""]

    for tag, title, prompt in PROMPTS:
        L += [f"## {title}", "", f"> {prompt}", ""]
        for m in models:
            r = results[m].get(tag, {})
            mt = metrics(r, max_tokens)
            cut = " ✂ truncated" if mt["truncated"] else ""
            L += [f"<details>",
                  f"<summary><b>{short[m]}</b> &mdash; {mt['verdict']} &middot; "
                  f"{r.get('tokens','?')} tok{cut} &middot; {mt['fences']} blocks "
                  f"&middot; {mt['hedges']} hedges &middot; {r.get('secs','?')}s</summary>",
                  ""]
            body = (r.get("text") or r.get("error", "")).strip()
            L += [body, "", "</details>", ""]

    os.makedirs(RESULTS_DIR, exist_ok=True)
    with open(MD_OUT, "w") as fh:
        fh.write("\n".join(L))
    print(f"Markdown: {MD_OUT}")
    return MD_OUT


def main():
    ap = argparse.ArgumentParser(description="Compare model refusal on red-team prompts.")
    ap.add_argument("--models", nargs="+", default=DEFAULT_MODELS)
    ap.add_argument("--max-tokens", type=int, default=int(_settings.get("max_tokens", 1600)))
    ap.add_argument("--report-only", action="store_true",
                    help="rebuild HTML from saved JSON without re-running probes")
    ap.add_argument("--list", action="store_true", help="list oMLX models and exit")
    ap.add_argument("--open", action="store_true", help="open the report when done")
    args = ap.parse_args()

    if args.list:
        for mid in list_models(api_key()):
            print(mid)
        return

    if args.report_only:
        if not os.path.exists(JSON_OUT):
            sys.exit(f"no results at {JSON_OUT} — run without --report-only first")
        with open(JSON_OUT) as fh:
            results = json.load(fh)
    else:
        results = run(args.models, args.max_tokens)

    path = build_report(results)
    build_markdown(results)
    if args.open:
        opener = shutil.which("open") or shutil.which("xdg-open")
        if opener:
            subprocess.run([opener, path], check=False)


if __name__ == "__main__":
    main()
