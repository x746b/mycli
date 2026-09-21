"""Shared mycli config lookup; matches the Rust CLI, including on macOS."""
import os
from pathlib import Path


def config_dir() -> Path:
    xdg = Path(os.environ.get("XDG_CONFIG_HOME", ""))
    return (xdg if xdg.is_absolute() else Path.home() / ".config") / "mycli"


def user_file(name: str) -> Path:
    preferred = config_dir() / name
    return preferred if preferred.exists() else Path.home() / ".mycli" / name
