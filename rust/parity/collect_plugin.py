"""Pytest plugin that harvests parity corpus candidates from the Python suite.

Load with ``-p collect_plugin`` (``rust/parity`` on ``PYTHONPATH``) and set
``DIPPY_PARITY_OUT`` to a directory. Each worker appends JSON lines there:

* ``{"src": "param", "cmd": ...}`` for every string parametrised test value;
* ``{"src": "analyze", "cmd": ..., "config": ...}`` for every top-level
  ``analyze()`` call whose config is the default ``Config()`` or an unmodified
  result of ``parse_config(text)``.

Only command strings and config text are recorded; expected decisions come
from the oracle (``build_corpus.py``), never from the test assertions.
"""

from __future__ import annotations

import copy
import json
import os
from pathlib import Path

from dippy.core import analyzer, config as config_mod

_OUT = os.environ.get("DIPPY_PARITY_OUT")
_SOURCES: dict[int, tuple[str, object]] = {}
_DEPTH = [0]
_SEEN: set[str] = set()


def _emit(record: dict) -> None:
    if not _OUT:
        return
    line = json.dumps(record, sort_keys=True)
    if line in _SEEN:
        return
    _SEEN.add(line)
    path = Path(_OUT) / f"harvest-{os.getpid()}.jsonl"
    with path.open("a", encoding="utf-8") as fh:
        fh.write(line + "\n")


def _strings(value, out: list[str]) -> None:
    if isinstance(value, str):
        out.append(value)
    elif isinstance(value, (list, tuple)):
        for item in value:
            _strings(item, out)


_original_parse_config = config_mod.parse_config
_original_analyze = analyzer.analyze


def _recording_parse_config(text, source=None):
    result = _original_parse_config(text, source)
    if source is None:
        _SOURCES[id(result)] = (text, copy.deepcopy(result))
    return result


def _config_text(config) -> str | None:
    if config == config_mod.Config():
        return ""
    entry = _SOURCES.get(id(config))
    if entry is None:
        return None
    text, snapshot = entry
    return text if snapshot == config else None


def _recording_analyze(command, config, cwd, context_flags=None, *, remote=False):
    if _DEPTH[0] == 0 and not context_flags and not remote:
        text = _config_text(config)
        if text is not None and isinstance(command, str):
            _emit({"src": "analyze", "cmd": command, "config": text})
    _DEPTH[0] += 1
    try:
        return _original_analyze(command, config, cwd, context_flags, remote=remote)
    finally:
        _DEPTH[0] -= 1


# Patch before any test module or dippy.dippy binds the names locally.
config_mod.parse_config = _recording_parse_config
analyzer.analyze = _recording_analyze


def pytest_collection_modifyitems(session, config, items):
    for item in items:
        callspec = getattr(item, "callspec", None)
        if callspec is None:
            continue
        found: list[str] = []
        for value in callspec.params.values():
            _strings(value, found)
        for value in found:
            if value.strip() and len(value) < 4000:
                _emit({"src": "param", "cmd": value})
