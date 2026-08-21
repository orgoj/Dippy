"""Test multi-CLI mode support (Claude, Gemini, Cursor) and auto-detection."""

from __future__ import annotations

import pytest


@pytest.fixture(autouse=True)
def reset_mode(monkeypatch):
    """Reset MODE to claude after each test."""
    monkeypatch.delenv("DIPPY_GEMINI", raising=False)
    monkeypatch.delenv("DIPPY_CURSOR", raising=False)
    monkeypatch.delenv("DIPPY_CLAUDE", raising=False)
    monkeypatch.delenv("DIPPY_CODEX", raising=False)
    yield
    import dippy.dippy

    dippy.dippy.MODE = "claude"


def _set_mode(monkeypatch, mode: str) -> None:
    """Set MODE on the dippy module for the duration of one test.

    Uses monkeypatch so the module global is restored afterwards; a bare
    assignment leaks the mode into every later test in the same process.
    """
    import dippy.dippy

    monkeypatch.setattr(dippy.dippy, "MODE", mode)


def test_gemini_approve_format(monkeypatch):
    """Test that Gemini mode returns correct JSON format for approval."""
    _set_mode(monkeypatch, "gemini")
    import dippy.dippy

    result = dippy.dippy.approve("git status")

    assert "decision" in result
    assert result["decision"] == "allow"
    assert "reason" in result
    assert "🐤" in result["reason"]
    assert "hookSpecificOutput" not in result


def test_gemini_ask_format(monkeypatch):
    """Test that Gemini mode returns correct JSON format for ask."""
    _set_mode(monkeypatch, "gemini")
    import dippy.dippy

    result = dippy.dippy.ask("rm -rf")

    assert "decision" in result
    assert result["decision"] == "ask"
    assert "reason" in result
    assert "🐤" in result["reason"]
    assert "hookSpecificOutput" not in result


def test_claude_approve_format(monkeypatch):
    """Test that Claude mode returns correct JSON format for approval."""
    _set_mode(monkeypatch, "claude")
    import dippy.dippy

    result = dippy.dippy.approve("git status")

    assert "hookSpecificOutput" in result
    assert result["hookSpecificOutput"]["permissionDecision"] == "allow"
    assert "🐤" in result["hookSpecificOutput"]["permissionDecisionReason"]
    assert "decision" not in result


def test_claude_ask_format(monkeypatch):
    """Test that Claude mode returns correct JSON format for ask."""
    _set_mode(monkeypatch, "claude")
    import dippy.dippy

    result = dippy.dippy.ask("rm -rf")

    assert "hookSpecificOutput" in result
    assert result["hookSpecificOutput"]["permissionDecision"] == "ask"
    assert "🐤" in result["hookSpecificOutput"]["permissionDecisionReason"]
    assert "decision" not in result


def test_gemini_env_var(monkeypatch):
    """Test that DIPPY_GEMINI env var enables Gemini mode."""
    _set_mode(monkeypatch, "gemini")
    import dippy.dippy

    result = dippy.dippy.approve("ls")

    assert "decision" in result
    assert result["decision"] == "allow"


def test_codex_approve_format(monkeypatch):
    """Test that Codex mode uses empty output for approvals."""
    _set_mode(monkeypatch, "codex")
    import dippy.dippy

    result = dippy.dippy.approve("git status")

    assert result is None


def test_codex_permission_request_approve_format(monkeypatch):
    """Test that Codex PermissionRequest approvals use current hook schema."""
    _set_mode(monkeypatch, "codex")
    import dippy.dippy

    result = dippy.dippy.approve("git status", hook_event="PermissionRequest")

    assert result == {
        "hookSpecificOutput": {
            "hookEventName": "PermissionRequest",
            "decision": {"behavior": "allow"},
        }
    }


def test_codex_ask_format(monkeypatch):
    """Test that Codex mode falls back to systemMessage for ask."""
    _set_mode(monkeypatch, "codex")
    import dippy.dippy

    result = dippy.dippy.ask("rm -rf")

    assert result == {"systemMessage": "🐤 rm -rf"}


def test_codex_mode_detection(monkeypatch):
    """Test that --codex flag explicitly sets Codex mode."""
    monkeypatch.setattr("sys.argv", ["dippy", "--codex"])
    from dippy.dippy import _detect_mode_from_flags

    assert _detect_mode_from_flags() == "codex"


def test_shell_tool_names():
    """Test that shell tool names include Claude and Gemini variants."""
    from dippy.dippy import SHELL_TOOL_NAMES

    assert "Bash" in SHELL_TOOL_NAMES  # Claude
    assert "shell" in SHELL_TOOL_NAMES  # Gemini
    assert "run_shell_command" in SHELL_TOOL_NAMES  # Gemini official


# === Cursor Tests ===


def test_cursor_approve_format(monkeypatch):
    """Test that Cursor mode returns correct JSON format for approval."""
    _set_mode(monkeypatch, "cursor")
    import dippy.dippy

    result = dippy.dippy.approve("git status")

    assert "permission" in result
    assert result["permission"] == "allow"
    # snake_case (v2.0+)
    assert "user_message" in result
    assert "agent_message" in result
    assert "🐤" in result["user_message"]
    # camelCase (v1.7.x)
    assert "userMessage" in result
    assert "agentMessage" in result
    assert "🐤" in result["userMessage"]
    # Not other formats
    assert "hookSpecificOutput" not in result
    assert "decision" not in result


def test_cursor_ask_format(monkeypatch):
    """Test that Cursor mode returns correct JSON format for ask."""
    _set_mode(monkeypatch, "cursor")
    import dippy.dippy

    result = dippy.dippy.ask("rm -rf")

    assert "permission" in result
    assert result["permission"] == "ask"
    # snake_case (v2.0+)
    assert "user_message" in result
    assert "agent_message" in result
    assert "🐤" in result["user_message"]
    # camelCase (v1.7.x)
    assert "userMessage" in result
    assert "agentMessage" in result
    assert "🐤" in result["userMessage"]
    # Not other formats
    assert "hookSpecificOutput" not in result
    assert "decision" not in result


def test_cursor_env_var(monkeypatch):
    """Test that DIPPY_CURSOR env var enables Cursor mode."""
    _set_mode(monkeypatch, "cursor")
    import dippy.dippy

    result = dippy.dippy.approve("ls")

    assert "permission" in result
    assert result["permission"] == "allow"


def test_cursor_mode_detection(monkeypatch):
    """Test that Cursor mode is correctly detected."""
    monkeypatch.setattr("sys.argv", ["dippy", "--cursor"])
    from dippy.dippy import _detect_mode_from_flags

    assert _detect_mode_from_flags() == "cursor"


# === Claude Flag Tests ===


def test_claude_flag(monkeypatch):
    """Test that --claude flag explicitly sets Claude mode."""
    monkeypatch.setattr("sys.argv", ["dippy", "--claude"])
    from dippy.dippy import _detect_mode_from_flags

    assert _detect_mode_from_flags() == "claude"


def test_claude_env_var(monkeypatch):
    """Test that DIPPY_CLAUDE env var enables Claude mode."""
    monkeypatch.setattr("sys.argv", ["dippy"])
    monkeypatch.setenv("DIPPY_CLAUDE", "true")
    from dippy.dippy import _detect_mode_from_flags

    assert _detect_mode_from_flags() == "claude"


def test_file_tool_names():
    """Test that file tool names include Claude and Gemini variants."""
    from dippy.dippy import FILE_TOOL_NAMES

    assert "Read" in FILE_TOOL_NAMES  # Claude
    assert "Write" in FILE_TOOL_NAMES  # Claude
    assert "read_file" in FILE_TOOL_NAMES  # Gemini
    assert "write_file" in FILE_TOOL_NAMES  # Gemini
    assert "replace" in FILE_TOOL_NAMES  # Gemini


def test_no_flag_defaults_to_claude(monkeypatch):
    """Test that no flag defaults to Claude mode."""
    monkeypatch.setattr("sys.argv", ["dippy"])
    monkeypatch.delenv("DIPPY_GEMINI", raising=False)
    monkeypatch.delenv("DIPPY_CURSOR", raising=False)
    monkeypatch.delenv("DIPPY_CLAUDE", raising=False)
    from dippy.dippy import _detect_mode_from_flags

    assert _detect_mode_from_flags() is None
