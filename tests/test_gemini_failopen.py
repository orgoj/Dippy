"""
Tests for Gemini fail-open security fix (bead bd-gvg).

All error/unknown paths in Gemini mode must return ask, not allow.
"""

from __future__ import annotations

import io
import json


def _run_main_gemini(monkeypatch, stdin_text: str, *, mock_load_config=True):
    """
    Run dippy.dippy.main() in Gemini mode with given stdin.
    Returns parsed JSON output dict (or None if stdout was empty).
    Avoids importlib.reload to prevent cross-test module state pollution.
    """
    import dippy.dippy

    # Patch mode detection directly — no importlib.reload needed
    monkeypatch.setattr(dippy.dippy, "MODE", "gemini")
    monkeypatch.setattr(dippy.dippy, "_detect_mode_from_flags", lambda: "gemini")
    monkeypatch.setattr("sys.argv", ["dippy", "--gemini"])
    monkeypatch.setattr("sys.stdin", io.StringIO(stdin_text))

    if mock_load_config:
        from dippy.core.config import Config

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


class TestGeminiFailOpenJsonError:
    """JSONDecodeError path must return ask, not allow."""

    def test_invalid_json_returns_ask(self, monkeypatch):
        output = _run_main_gemini(monkeypatch, "not valid json", mock_load_config=False)
        assert output is not None
        assert output.get("decision") == "ask", (
            f"Expected 'ask' but got {output.get('decision')!r}. "
            "Gemini JSONDecodeError must not fail-open with 'allow'."
        )

    def test_invalid_json_not_allow(self, monkeypatch):
        output = _run_main_gemini(monkeypatch, "{{{bad json}}}", mock_load_config=False)
        assert output is not None
        assert output.get("decision") != "allow"


class TestGeminiFailOpenUnsupportedTool:
    """Non-shell, non-file tools in Gemini mode must return ask, not allow."""

    def test_unsupported_tool_returns_ask(self, monkeypatch, tmp_path):
        input_data = {
            "hook_event_name": "BeforeTool",
            "tool_name": "TodoWrite",
            "tool_input": {"todos": []},
            "cwd": str(tmp_path),
        }
        output = _run_main_gemini(monkeypatch, json.dumps(input_data))
        assert output is not None
        assert output.get("decision") == "ask", (
            f"Expected 'ask' but got {output.get('decision')!r}. "
            "Gemini unsupported-tool must not fail-open with 'allow'."
        )

    def test_unsupported_tool_not_allow(self, monkeypatch, tmp_path):
        input_data = {
            "hook_event_name": "BeforeTool",
            "tool_name": "AskUser",
            "tool_input": {},
            "cwd": str(tmp_path),
        }
        output = _run_main_gemini(monkeypatch, json.dumps(input_data))
        assert output is not None
        assert output.get("decision") != "allow"


class TestGeminiFailOpenNoFilePath:
    """File tool with no file_path in Gemini mode must return ask, not allow."""

    def test_file_tool_no_path_returns_ask(self, monkeypatch, tmp_path):
        input_data = {
            "hook_event_name": "BeforeTool",
            "tool_name": "Read",
            "tool_input": {},  # no file_path, no paths
            "cwd": str(tmp_path),
        }
        output = _run_main_gemini(monkeypatch, json.dumps(input_data))
        assert output is not None
        assert output.get("decision") == "ask", (
            f"Expected 'ask' but got {output.get('decision')!r}. "
            "Gemini file-tool with no path must not fail-open with 'allow'."
        )

    def test_write_tool_no_path_not_allow(self, monkeypatch, tmp_path):
        input_data = {
            "hook_event_name": "BeforeTool",
            "tool_name": "Write",
            "tool_input": {},
            "cwd": str(tmp_path),
        }
        output = _run_main_gemini(monkeypatch, json.dumps(input_data))
        assert output is not None
        assert output.get("decision") != "allow"


class TestGeminiFailOpenFileNoMatch:
    """File tool with path but no matching rule in Gemini mode must return ask, not allow."""

    def test_file_no_match_returns_ask(self, monkeypatch, tmp_path):
        input_data = {
            "hook_event_name": "BeforeTool",
            "tool_name": "Read",
            "tool_input": {"file_path": str(tmp_path / "somefile.txt")},
            "cwd": str(tmp_path),
        }
        # Empty Config has no rules, so no rule will match
        output = _run_main_gemini(monkeypatch, json.dumps(input_data))
        assert output is not None
        assert output.get("decision") == "ask", (
            f"Expected 'ask' but got {output.get('decision')!r}. "
            "Gemini file-no-match must not fail-open with 'allow'."
        )

    def test_multi_file_no_match_returns_ask(self, monkeypatch, tmp_path):
        input_data = {
            "hook_event_name": "BeforeTool",
            "tool_name": "read_many_files",
            "tool_input": {
                "paths": [
                    str(tmp_path / "a.txt"),
                    str(tmp_path / "b.txt"),
                ]
            },
            "cwd": str(tmp_path),
        }
        output = _run_main_gemini(monkeypatch, json.dumps(input_data))
        assert output is not None
        assert output.get("decision") == "ask", (
            f"Expected 'ask' but got {output.get('decision')!r}. "
            "Gemini multi-file no-match must not fail-open with 'allow'."
        )


class TestGeminiFailOpenTopLevelException:
    """Top-level exceptions in Gemini mode must return ask, not allow."""

    def test_top_level_exception_returns_ask(self, monkeypatch, tmp_path):
        import dippy.dippy

        def raise_runtime_error(cwd):
            raise RuntimeError("simulated internal error")

        input_data = {
            "hook_event_name": "BeforeTool",
            "tool_name": "Bash",
            "tool_input": {"command": "echo hello"},
            "cwd": str(tmp_path),
        }

        monkeypatch.setattr(dippy.dippy, "MODE", "gemini")
        monkeypatch.setattr(dippy.dippy, "_detect_mode_from_flags", lambda: "gemini")
        monkeypatch.setattr("sys.argv", ["dippy", "--gemini"])
        monkeypatch.setattr(dippy.dippy, "load_config", raise_runtime_error)
        monkeypatch.setattr("sys.stdin", io.StringIO(json.dumps(input_data)))

        buf = io.StringIO()
        import sys

        old_stdout = sys.stdout
        sys.stdout = buf
        try:
            dippy.dippy.main()
        finally:
            sys.stdout = old_stdout

        out = buf.getvalue().strip()
        assert out, "Expected output for top-level exception"
        output = json.loads(out)
        assert output.get("decision") == "ask", (
            f"Expected 'ask' but got {output.get('decision')!r}. "
            "Gemini top-level exception must not fail-open with 'allow'."
        )
