"""Shared, deterministic environment for the parity oracle and runner.

Both implementations see the same fixed paths so that decisions depending on
the working directory, home directory or config file location are comparable.
"""

from __future__ import annotations

import hashlib
import os
import shutil
from pathlib import Path

ROOT = Path(os.environ.get("DIPPY_PARITY_ROOT", "/tmp/dippy-parity"))
HOME = ROOT / "home"
WORK = ROOT / "work"
CONFIGS = ROOT / "configs"
CWD_PLACEHOLDER = "$PARITY_WORK"


def config_path_for(text: str) -> Path:
    digest = hashlib.sha256(text.encode("utf-8")).hexdigest()[:16]
    return CONFIGS / f"{digest}.dippy"


def resolve_cwd(value: str) -> str:
    return value.replace(CWD_PLACEHOLDER, str(WORK))


def prepare_root(cases) -> None:
    """Create an empty HOME and work dir and write every config file."""
    for path in (HOME, WORK, CONFIGS):
        if path.exists():
            shutil.rmtree(path)
        path.mkdir(parents=True)
    for case in cases:
        text = case.get("config", "")
        if text:
            config_path_for(text).write_text(text, encoding="utf-8")


def child_env() -> dict[str, str]:
    """Minimal environment: no DIPPY_* variables, empty HOME."""
    return {
        "HOME": str(HOME),
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "LANG": "C.UTF-8",
        "DIPPY_TEST_NO_LOG": "1",
        "PARITY_ENV": "1",
    }


def cli_args(case: dict) -> list[str]:
    args = ["--cmd", case["cmd"], "--json", "--cwd", resolve_cwd(case.get("cwd", CWD_PLACEHOLDER))]
    if case.get("config"):
        args += ["--config", str(config_path_for(case["config"]))]
    return args
