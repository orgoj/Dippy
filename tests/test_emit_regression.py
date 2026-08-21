"""Regression tests for the _emit() helper deleted by 62d49af.

Eight call sites survived the deletion. Each one raised NameError, which the
top-level handler swallowed into a generic ask, hiding the real reason from
the user. These tests pin the specific reasons.
"""

from __future__ import annotations

import io
import json


def _run_main_gemini(monkeypatch, stdin_text: str):
    """Run main() in Gemini mode and return the parsed stdout, or None."""
    import dippy.dippy
    from dippy.core.config import Config

    monkeypatch.setattr(dippy.dippy, "MODE", "gemini")
    monkeypatch.setattr(dippy.dippy, "_detect_mode_from_flags", lambda: "gemini")
    monkeypatch.setattr("sys.argv", ["dippy", "--gemini"])
    monkeypatch.setattr("sys.stdin", io.StringIO(stdin_text))
    monkeypatch.setattr(dippy.dippy, "load_config", lambda cwd: Config())

    buf = io.StringIO()
    import sys

    old_stdout = sys.stdout
    sys.stdout = buf
    try:
        dippy.dippy.main()
    finally:
        sys.stdout = old_stdout

    out = buf.getvalue().strip()
    return json.loads(out) if out else None


class TestEmitDefined:
    def test_emit_exists(self):
        """The helper the call sites reference must exist."""
        import dippy.dippy

        assert callable(dippy.dippy._emit)

    def test_emit_prints_json(self, capsys):
        import dippy.dippy

        dippy.dippy._emit({"decision": "allow"})
        assert json.loads(capsys.readouterr().out) == {"decision": "allow"}

    def test_emit_swallows_none(self, capsys):
        """None is the Codex sentinel for 'approved, say nothing'."""
        import dippy.dippy

        dippy.dippy._emit(None)
        assert capsys.readouterr().out == ""


class TestEmitReasonsReachTheUser:
    def test_unsupported_tool_states_the_tool(self, monkeypatch):
        result = _run_main_gemini(
            monkeypatch,
            json.dumps({"tool_name": "totally_unknown_tool", "tool_input": {}}),
        )
        assert result is not None
        assert "unsupported tool" in json.dumps(result)
        assert "totally_unknown_tool" in json.dumps(result)

    def test_missing_file_path_says_so(self, monkeypatch):
        result = _run_main_gemini(
            monkeypatch,
            json.dumps({"tool_name": "read_file", "tool_input": {}}),
        )
        assert result is not None
        assert "no file path provided" in json.dumps(result)


class TestExpandTemplateImported:
    def test_expand_template_is_available(self):
        """The idle-notifier path calls it at module scope."""
        import dippy.dippy

        assert callable(dippy.dippy.expand_template)
