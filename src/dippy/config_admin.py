"""Comment-preserving administration of Dippy configuration files."""

from __future__ import annotations

import os
import tempfile
from pathlib import Path


def _setting_key(line: str) -> str | None:
    stripped = line.strip()
    if not stripped or stripped.startswith("#"):
        return None
    parts = stripped.split(None, 2)
    if len(parts) >= 2 and parts[0].lower() == "set":
        return parts[1].lower().replace("_", "-")
    return None


def _write_atomic(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    mode = path.stat().st_mode if path.exists() else None
    fd, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    temporary_path = Path(temporary)
    try:
        with os.fdopen(fd, "w") as stream:
            stream.write(text)
            stream.flush()
            os.fsync(stream.fileno())
        if mode is not None:
            os.chmod(temporary_path, mode)
        os.replace(temporary_path, path)
    finally:
        if temporary_path.exists():
            temporary_path.unlink()


def edit_config(path: Path, operation: str, key: str, value: str | None = None) -> None:
    """Edit one setting or server directive without rewriting unrelated lines."""
    lines = path.read_text().splitlines() if path.exists() else []
    if operation in ("set", "unset"):
        normalized = key.lower().replace("_", "-")
        replacement = f"set {normalized} {value}" if operation == "set" else None
        output = []
        replaced = False
        for line in lines:
            if _setting_key(line) == normalized:
                if replacement is not None and not replaced:
                    output.append(replacement)
                    replaced = True
                continue
            output.append(line)
        if replacement is not None and not replaced:
            output.append(replacement)
    elif operation in ("server-add", "server-remove"):
        directive = f"server {key}"
        output = [line for line in lines if line.strip() != directive]
        if operation == "server-add":
            output.append(directive)
    else:
        raise ValueError(f"unknown config operation: {operation}")
    text = "\n".join(output)
    if output:
        text += "\n"
    _write_atomic(path, text)
