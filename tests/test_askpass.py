"""Tests for pluggable askpass (SSH_ASKPASS style) approval programs."""

import json
from pathlib import Path

import pytest

from dippy.core.config import Config, parse_config


# === Fixtures ===


@pytest.fixture
def askpass_approve(tmp_path):
    """Create askpass script that always approves (exit 0)."""
    script = tmp_path / "askpass-approve"
    script.write_text("#!/bin/bash\nexit 0\n")
    script.chmod(0o755)
    return script


@pytest.fixture
def askpass_deny(tmp_path):
    """Create askpass script that always denies (exit 1)."""
    script = tmp_path / "askpass-deny"
    script.write_text("#!/bin/bash\nexit 1\n")
    script.chmod(0o755)
    return script


@pytest.fixture
def askpass_fallback(tmp_path):
    """Create askpass script that returns fallback (exit 2)."""
    script = tmp_path / "askpass-fallback"
    script.write_text("#!/bin/bash\nexit 2\n")
    script.chmod(0o755)
    return script


@pytest.fixture
def askpass_timeout(tmp_path):
    """Create askpass script that sleeps forever (for timeout testing)."""
    script = tmp_path / "askpass-timeout"
    script.write_text("#!/bin/bash\nsleep 100\nexit 0\n")
    script.chmod(0o755)
    return script


@pytest.fixture
def askpass_capture(tmp_path):
    """Create askpass script that captures env vars and stdin to file."""
    output = tmp_path / "askpass-output.json"
    script = tmp_path / "askpass-capture"
    # Python script for reliable JSON output
    script.write_text(f'''#!/usr/bin/env python3
import json
import os
import sys

stdin_data = sys.stdin.read()
env_vars = {{k: v for k, v in os.environ.items() if k.startswith("DIPPY_")}}

with open("{output}", "w") as f:
    json.dump({{"env": env_vars, "stdin": stdin_data}}, f)

sys.exit(0)
''')
    script.chmod(0o755)
    return script, output


# === Config Parsing Tests ===


class TestAskpassConfigParsing:
    """Test parsing of askpass settings in config files."""

    def test_parse_set_askpass(self, tmp_path):
        """set askpass /usr/bin/zenity parses to Path."""
        script = tmp_path / "zenity"
        cfg = parse_config(f"set askpass {script}")
        assert cfg.askpass == script

    def test_parse_set_askpass_tilde_expansion(self):
        """set askpass ~/bin/askpass expands tilde."""
        cfg = parse_config("set askpass ~/bin/askpass")
        assert cfg.askpass == Path.home() / "bin" / "askpass"

    def test_parse_set_askpass_missing_value(self):
        """set askpass without path raises ValueError (logged and skipped)."""
        cfg = parse_config("set askpass")
        # Invalid setting is skipped, askpass remains None
        assert cfg.askpass is None

    def test_parse_set_askpass_timeout(self):
        """set askpass-timeout 30 parses to int."""
        cfg = parse_config("set askpass-timeout 30")
        assert cfg.askpass_timeout == 30

    def test_parse_set_askpass_timeout_invalid(self):
        """set askpass-timeout abc raises ValueError (logged and skipped)."""
        cfg = parse_config("set askpass-timeout abc")
        # Invalid setting is skipped, timeout remains default
        assert cfg.askpass_timeout == 60

    def test_parse_set_askpass_timeout_missing_value(self):
        """set askpass-timeout without value raises ValueError (logged and skipped)."""
        cfg = parse_config("set askpass-timeout")
        # Invalid setting is skipped, timeout remains default
        assert cfg.askpass_timeout == 60

    def test_config_askpass_default_none(self):
        """Config().askpass is None by default."""
        cfg = Config()
        assert cfg.askpass is None

    def test_config_askpass_timeout_default(self):
        """Config().askpass_timeout defaults to 60."""
        cfg = Config()
        assert cfg.askpass_timeout == 60

    def test_parse_combined_askpass_settings(self, tmp_path):
        """Both askpass and askpass-timeout parse together."""
        script = tmp_path / "my-askpass"
        cfg = parse_config(f"""
set askpass {script}
set askpass-timeout 120
""")
        assert cfg.askpass == script
        assert cfg.askpass_timeout == 120


# === Askpass Execution Tests ===


class TestRunAskpass:
    """Test the _run_askpass function."""

    def test_run_askpass_approve(self, askpass_approve):
        """Exit 0 returns 'allow'."""
        from dippy.dippy import _run_askpass

        config = Config(askpass=askpass_approve)
        result = _run_askpass(
            config=config,
            command="git push origin main",
            cwd="/home/user/project",
            rule="ask git push *",
            message="Pushing to remote",
            tool="Bash",
        )
        assert result == "allow"

    def test_run_askpass_deny(self, askpass_deny):
        """Exit 1 returns 'deny'."""
        from dippy.dippy import _run_askpass

        config = Config(askpass=askpass_deny)
        result = _run_askpass(
            config=config,
            command="rm -rf /",
            cwd="/",
            rule="ask rm *",
            message="Dangerous!",
            tool="Bash",
        )
        assert result == "deny"

    def test_run_askpass_fallback_exit2(self, askpass_fallback):
        """Exit 2 returns 'ask' (fallback to Claude dialog)."""
        from dippy.dippy import _run_askpass

        config = Config(askpass=askpass_fallback)
        result = _run_askpass(
            config=config,
            command="some command",
            cwd="/tmp",
            rule=None,
            message=None,
            tool="Bash",
        )
        assert result == "ask"

    def test_run_askpass_timeout(self, askpass_timeout):
        """Timeout returns 'ask' (fallback)."""
        from dippy.dippy import _run_askpass

        config = Config(askpass=askpass_timeout, askpass_timeout=1)  # 1 second
        result = _run_askpass(
            config=config,
            command="some command",
            cwd="/tmp",
            rule=None,
            message=None,
            tool="Bash",
        )
        assert result == "ask"

    def test_run_askpass_not_found(self, tmp_path):
        """Missing program returns 'ask' (fallback)."""
        from dippy.dippy import _run_askpass

        config = Config(askpass=tmp_path / "nonexistent-script")
        result = _run_askpass(
            config=config,
            command="some command",
            cwd="/tmp",
            rule=None,
            message=None,
            tool="Bash",
        )
        assert result == "ask"

    def test_run_askpass_permission_denied(self, tmp_path):
        """Non-executable script returns 'ask' (fallback)."""
        from dippy.dippy import _run_askpass

        script = tmp_path / "not-executable"
        script.write_text("#!/bin/bash\nexit 0\n")
        # Don't set executable bit

        config = Config(askpass=script)
        result = _run_askpass(
            config=config,
            command="some command",
            cwd="/tmp",
            rule=None,
            message=None,
            tool="Bash",
        )
        assert result == "ask"

    def test_run_askpass_env_override(self, askpass_approve, askpass_deny, monkeypatch):
        """DIPPY_ASKPASS env var overrides config.askpass."""
        from dippy.dippy import _run_askpass

        # Config has deny script, but env var points to approve script
        config = Config(askpass=askpass_deny)
        monkeypatch.setenv("DIPPY_ASKPASS", str(askpass_approve))

        result = _run_askpass(
            config=config,
            command="some command",
            cwd="/tmp",
            rule=None,
            message=None,
            tool="Bash",
        )
        # Should use env var (approve) not config (deny)
        assert result == "allow"

    def test_askpass_receives_env_vars(self, askpass_capture):
        """Verify DIPPY_* environment variables are set correctly."""
        from dippy.dippy import _run_askpass

        script, output = askpass_capture
        config = Config(askpass=script)

        _run_askpass(
            config=config,
            command="git push origin main",
            cwd="/home/user/project",
            rule="ask git push *",
            message="Pushing to remote",
            tool="Bash",
        )

        captured = json.loads(output.read_text())
        env = captured["env"]

        assert env["DIPPY_COMMAND"] == "git push origin main"
        assert env["DIPPY_CWD"] == "/home/user/project"
        assert env["DIPPY_RULE"] == "ask git push *"
        assert env["DIPPY_MESSAGE"] == "Pushing to remote"
        assert env["DIPPY_TOOL"] == "Bash"

    def test_askpass_receives_json_stdin(self, askpass_capture):
        """Verify JSON input with all fields is passed via stdin."""
        from dippy.dippy import _run_askpass

        script, output = askpass_capture
        config = Config(askpass=script)

        _run_askpass(
            config=config,
            command="docker run -it ubuntu",
            cwd="/projects/app",
            rule="ask docker *",
            message="Running container",
            tool="Bash",
            source="/home/user/.dippy/config",
        )

        captured = json.loads(output.read_text())
        stdin_data = json.loads(captured["stdin"])

        assert stdin_data["command"] == "docker run -it ubuntu"
        assert stdin_data["cwd"] == "/projects/app"
        assert stdin_data["rule"] == "ask docker *"
        assert stdin_data["message"] == "Running container"
        assert stdin_data["tool"] == "Bash"
        assert stdin_data["source"] == "/home/user/.dippy/config"

    def test_askpass_no_config_returns_ask(self):
        """Without askpass configured, returns 'ask'."""
        from dippy.dippy import _run_askpass

        config = Config()  # No askpass set
        result = _run_askpass(
            config=config,
            command="some command",
            cwd="/tmp",
            rule=None,
            message=None,
            tool="Bash",
        )
        assert result == "ask"

    def test_askpass_optional_fields_omitted(self, askpass_capture):
        """Optional fields (rule, message) can be None."""
        from dippy.dippy import _run_askpass

        script, output = askpass_capture
        config = Config(askpass=script)

        _run_askpass(
            config=config,
            command="ls -la",
            cwd="/tmp",
            rule=None,
            message=None,
            tool="Bash",
        )

        captured = json.loads(output.read_text())
        env = captured["env"]

        assert env["DIPPY_COMMAND"] == "ls -la"
        assert env["DIPPY_CWD"] == "/tmp"
        assert "DIPPY_RULE" not in env
        assert "DIPPY_MESSAGE" not in env
        assert env["DIPPY_TOOL"] == "Bash"


# === Config Merge Tests ===


class TestAskpassConfigMerge:
    """Test that askpass settings merge correctly."""

    def test_merge_configs_askpass(self):
        """Overlay askpass overrides base."""
        from dippy.core.config import _merge_configs

        base = Config(askpass=Path("/base/askpass"))
        overlay = Config(askpass=Path("/overlay/askpass"))
        merged = _merge_configs(base, overlay)
        assert merged.askpass == Path("/overlay/askpass")

    def test_merge_configs_askpass_none_preserves_base(self):
        """Overlay with None askpass preserves base."""
        from dippy.core.config import _merge_configs

        base = Config(askpass=Path("/base/askpass"))
        overlay = Config()  # askpass is None
        merged = _merge_configs(base, overlay)
        assert merged.askpass == Path("/base/askpass")

    def test_merge_configs_askpass_timeout(self):
        """Overlay askpass_timeout overrides base."""
        from dippy.core.config import _merge_configs

        base = Config(askpass_timeout=30)
        overlay = Config(askpass_timeout=120)
        merged = _merge_configs(base, overlay)
        assert merged.askpass_timeout == 120
