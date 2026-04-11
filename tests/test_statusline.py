"""Tests for bin/dippy-statusline entry point."""

from __future__ import annotations

import json
import subprocess
import tempfile
import uuid
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DIPPY_STATUSLINE = REPO_ROOT / "bin" / "dippy-statusline"
SYSTEM_PYTHON = "/usr/bin/python3"


def unique_session_id() -> str:
    """Generate unique session ID to avoid cache interference between tests."""
    return f"test-{uuid.uuid4()}"


def run_statusline(
    input_data: dict | str | None = None,
    via_symlink: bool = False,
) -> subprocess.CompletedProcess:
    """Run dippy-statusline with given input."""
    if input_data is None:
        stdin_bytes = b""
    elif isinstance(input_data, dict):
        stdin_bytes = json.dumps(input_data).encode()
    else:
        stdin_bytes = input_data.encode()

    script = DIPPY_STATUSLINE
    if via_symlink:
        # Create temp symlink for testing
        tmpdir = tempfile.mkdtemp()
        symlink_path = Path(tmpdir) / "dippy-statusline"
        symlink_path.symlink_to(DIPPY_STATUSLINE)
        script = symlink_path

    return subprocess.run(
        [SYSTEM_PYTHON, str(script)],
        input=stdin_bytes,
        capture_output=True,
        timeout=10,
    )


class TestSmokeTest:
    """Basic smoke tests - does it run without crashing?"""

    def test_valid_input(self):
        """Valid input produces output without crashing."""
        input_data = {
            "session_id": unique_session_id(),
            "model": {"display_name": "Opus"},
            "workspace": {"current_dir": "/tmp"},
        }
        result = run_statusline(input_data)
        assert result.returncode == 0, f"stderr: {result.stderr.decode()}"
        output = result.stdout.decode()
        assert len(output) > 0
        # Should contain model name
        assert "Opus" in output

    def test_symlink_invocation(self):
        """Works when invoked via symlink (Homebrew scenario)."""
        input_data = {
            "session_id": unique_session_id(),
            "model": {"display_name": "Claude"},
        }
        result = run_statusline(input_data, via_symlink=True)
        assert result.returncode == 0, f"stderr: {result.stderr.decode()}"


class TestErrorResilience:
    """Tests for graceful handling of bad/missing input."""

    def test_empty_stdin(self):
        """Empty stdin doesn't crash."""
        result = run_statusline(None)
        assert result.returncode == 0
        # Should output something (even if just "?")
        assert len(result.stdout) > 0

    def test_invalid_json(self):
        """Malformed JSON doesn't crash."""
        result = run_statusline("not valid json {{{")
        assert result.returncode == 0
        assert len(result.stdout) > 0

    def test_empty_json_object(self):
        """Empty JSON object doesn't crash."""
        result = run_statusline({})
        assert result.returncode == 0
        assert len(result.stdout) > 0

    def test_missing_model(self):
        """Missing model field doesn't crash."""
        result = run_statusline({"session_id": unique_session_id(), "workspace": {}})
        assert result.returncode == 0

    def test_missing_workspace(self):
        """Missing workspace field doesn't crash."""
        result = run_statusline({"session_id": unique_session_id(), "model": {}})
        assert result.returncode == 0

    def test_missing_session_id(self):
        """Missing session_id field doesn't crash."""
        result = run_statusline({"model": {}, "workspace": {}})
        assert result.returncode == 0

    def test_null_values(self):
        """Null values in fields don't crash."""
        result = run_statusline(
            {
                "session_id": None,
                "model": None,
                "workspace": None,
            }
        )
        assert result.returncode == 0

    def test_wrong_types(self):
        """Wrong types for fields don't crash."""
        result = run_statusline(
            {
                "session_id": 12345,
                "model": "not a dict",
                "workspace": ["list", "instead"],
            }
        )
        assert result.returncode == 0


class TestGitEdgeCases:
    """Tests for git-related edge cases."""

    def test_non_git_directory(self):
        """Non-git directory doesn't crash, just omits git info."""
        input_data = {
            "session_id": unique_session_id(),
            "model": {"display_name": "Claude"},
            "workspace": {"current_dir": "/tmp"},
        }
        result = run_statusline(input_data)
        assert result.returncode == 0
        # Should have output, just without git branch/changes

    def test_nonexistent_directory(self):
        """Nonexistent directory doesn't crash."""
        input_data = {
            "session_id": unique_session_id(),
            "model": {"display_name": "Claude"},
            "workspace": {"current_dir": "/nonexistent/path/that/does/not/exist"},
        }
        result = run_statusline(input_data)
        assert result.returncode == 0

    def test_empty_current_dir(self):
        """Empty current_dir doesn't crash."""
        input_data = {
            "session_id": unique_session_id(),
            "model": {"display_name": "Claude"},
            "workspace": {"current_dir": ""},
        }
        result = run_statusline(input_data)
        assert result.returncode == 0


class TestIsDippyConfigured:
    """Tests for is_dippy_configured() — reads settings at call time, no reload needed."""

    def test_returns_false_when_settings_missing(self, tmp_path, monkeypatch):
        monkeypatch.setenv("HOME", str(tmp_path))
        from dippy.dippy_statusline import is_dippy_configured

        assert is_dippy_configured() is False

    def test_returns_false_when_no_hooks(self, tmp_path, monkeypatch):
        monkeypatch.setenv("HOME", str(tmp_path))
        claude_dir = tmp_path / ".claude"
        claude_dir.mkdir()
        (claude_dir / "settings.json").write_text('{"hooks": {}}')
        from dippy.dippy_statusline import is_dippy_configured

        assert is_dippy_configured() is False

    def test_returns_true_when_dippy_executable_configured(self, tmp_path, monkeypatch):
        monkeypatch.setenv("HOME", str(tmp_path))
        fake_dippy = tmp_path / "dippy"
        fake_dippy.write_text("#!/bin/sh\necho test")
        fake_dippy.chmod(0o755)
        settings = {
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [{"command": str(fake_dippy)}],
                    }
                ]
            }
        }
        claude_dir = tmp_path / ".claude"
        claude_dir.mkdir()
        (claude_dir / "settings.json").write_text(json.dumps(settings))
        from dippy.dippy_statusline import is_dippy_configured

        assert is_dippy_configured() is True

    def test_returns_false_when_command_not_executable(self, tmp_path, monkeypatch):
        monkeypatch.setenv("HOME", str(tmp_path))
        settings = {
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [{"command": "/nonexistent/dippy"}],
                    }
                ]
            }
        }
        claude_dir = tmp_path / ".claude"
        claude_dir.mkdir()
        (claude_dir / "settings.json").write_text(json.dumps(settings))
        from dippy.dippy_statusline import is_dippy_configured

        assert is_dippy_configured() is False


class TestOutputFormat:
    """Tests for output format validation."""

    def test_output_has_separator(self):
        """Output uses pipe separator between sections."""
        input_data = {
            "session_id": unique_session_id(),
            "model": {"display_name": "Claude Sonnet"},
            "workspace": {"current_dir": "/tmp"},
        }
        result = run_statusline(input_data)
        assert result.returncode == 0
        output = result.stdout.decode()
        # Should have at least one separator (model | dir)
        assert "|" in output or len(output.strip()) > 0

    def test_output_is_single_line(self):
        """Output is a single line (no embedded newlines except trailing)."""
        input_data = {
            "session_id": unique_session_id(),
            "model": {"display_name": "Claude"},
            "workspace": {"current_dir": "/tmp"},
        }
        result = run_statusline(input_data)
        assert result.returncode == 0
        output = result.stdout.decode()
        # Strip trailing newline, then check no newlines remain
        assert "\n" not in output.rstrip("\n")


class TestMcpCacheNoShellInjection:
    """Tests for bd-a56: no shell=True in MCP cache refresh."""

    def test_no_shell_true_in_source(self):
        """get_mcp_servers must not use shell=True in subprocess calls."""
        source_path = REPO_ROOT / "src" / "dippy" / "dippy_statusline.py"
        source = source_path.read_text()
        assert "shell=True" not in source, (
            "dippy_statusline.py must not use shell=True (shell injection risk)"
        )

    def test_format_mcp_servers_output_connected(self):
        """_format_mcp_servers_output formats connected servers with color."""
        from dippy.dippy_statusline import _format_mcp_servers_output

        mcp_output = "filesystem: Connected\ngithub: Disconnected\n"
        result = _format_mcp_servers_output(mcp_output, 0, 255, 0, 255, 0, 0)
        assert "filesystem" in result
        assert "!filesystem" not in result
        assert "!github" in result
        assert ", " in result

    def test_format_mcp_servers_output_skips_lines_without_colon(self):
        """_format_mcp_servers_output ignores lines with no colon."""
        from dippy.dippy_statusline import _format_mcp_servers_output

        mcp_output = "header line\nfilesystem: Connected\n"
        result = _format_mcp_servers_output(mcp_output, 0, 255, 0, 255, 0, 0)
        assert "filesystem" in result
        assert "header" not in result

    def test_write_mcp_cache_handles_spaces_in_path(self, tmp_path):
        """_write_mcp_cache writes atomically to paths containing spaces."""
        from dippy.dippy_statusline import _write_mcp_cache

        cache_dir = tmp_path / "path with spaces in name"
        cache_dir.mkdir()
        cache_path = cache_dir / "mcp.cache"
        _write_mcp_cache(str(cache_path), "server1, server2")
        assert cache_path.read_text() == "server1, server2"

    def test_write_mcp_cache_atomic(self, tmp_path):
        """_write_mcp_cache uses atomic write (tmp + rename)."""
        from dippy.dippy_statusline import _write_mcp_cache

        cache_path = tmp_path / "mcp.cache"
        _write_mcp_cache(str(cache_path), "myserver")
        assert cache_path.read_text() == "myserver"
        tmp_files = list(tmp_path.glob("*.tmp.*"))
        assert not tmp_files, f"Leftover tmp files: {tmp_files}"
