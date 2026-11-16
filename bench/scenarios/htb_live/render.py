#!/usr/bin/env python3
"""Render a platform-specific live HTB briefing with a current IP and optional name."""
import argparse
from ipaddress import ip_address
from pathlib import Path
import re

HERE = Path(__file__).resolve().parent
PLATFORMS = {"linux": "linux.md", "windows": "windows.md"}
NAME_RE = re.compile(r"[A-Za-z0-9][A-Za-z0-9 ._-]{0,79}\Z")


def render_prompt(platform: str, target_ip: str, name: str | None = None) -> str:
    if platform not in PLATFORMS:
        raise ValueError(f"unknown live platform: {platform}")
    address = ip_address(target_ip)
    if address.version != 4:
        raise ValueError("target must have an IPv4 address")
    if name is not None:
        name = name.strip()
        if not NAME_RE.fullmatch(name):
            raise ValueError("box name must be 1–80 letters, digits, spaces, dots, underscores, or hyphens")
    prompt = (HERE / PLATFORMS[platform]).read_text(encoding="utf-8")
    if prompt.count("{{TARGET_IP}}") < 2 or prompt.count("{{BOX_CONTEXT}}") != 1:
        raise ValueError(f"unexpected placeholders in {PLATFORMS[platform]}")
    context = f"The operator identifies this box as **{name}**." if name else "No box name was supplied."
    return prompt.replace("{{TARGET_IP}}", str(address)).replace("{{BOX_CONTEXT}}", context).strip()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("platform", choices=PLATFORMS)
    parser.add_argument("ip", help="current operator-spawned HTB target IPv4 address")
    parser.add_argument("--name", help="optional box name for context only")
    args = parser.parse_args()
    try:
        print(render_prompt(args.platform, args.ip, args.name))
    except ValueError as exc:
        parser.error(str(exc))


if __name__ == "__main__":
    main()
