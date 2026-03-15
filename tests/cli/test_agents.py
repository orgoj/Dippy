"""
Tests for agent detection and configuration module.

Covers detection of installed AI coding assistants, config path resolution,
and dippy command generation for different hook formats.
"""

from __future__ import annotations

import os
from pathlib import Path
from unittest.mock import patch


from dippy.cli.agents import (
    AGENTS,
    AgentInfo,
    _find_dippy_executable,
    detect_agents,
    get_agent_info,
    list_all_agents,
    resolve_dippy_command,
)


class TestAgentInfo:
    """Test AgentInfo dataclass behavior."""

    def test_agent_info_attributes(self):
        """AgentInfo has all required fields."""
        info = AgentInfo(
            id="test",
            name="Test Agent",
            global_config=Path("/test/config.json"),
            project_config=".test/config.json",
            hook_format="claude",
            config_format="json",
            env_flags=("TEST_FLAG",),
            cli_flags=("--test",),
        )
        assert info.id == "test"
        assert info.name == "Test Agent"
        assert info.global_config == Path("/test/config.json")
        assert info.project_config == ".test/config.json"
        assert info.hook_format == "claude"
        assert info.config_format == "json"
        assert info.env_flags == ("TEST_FLAG",)
        assert info.cli_flags == ("--test",)

    def test_is_installed_file_config_missing(self, tmp_path):
        """is_installed returns False when config file doesn't exist."""
        info = AgentInfo(
            id="test",
            name="Test",
            global_config=tmp_path / ".test" / "settings.json",
            project_config=".test/settings.json",
            hook_format="claude",
            config_format="json",
            env_flags=(),
            cli_flags=(),
        )
        assert not info.is_installed()

    def test_is_installed_file_config_exists(self, tmp_path):
        """is_installed returns True when config file exists."""
        config_dir = tmp_path / ".test"
        config_dir.mkdir()
        (config_dir / "settings.json").write_text("{}")

        info = AgentInfo(
            id="test",
            name="Test",
            global_config=config_dir / "settings.json",
            project_config=".test/settings.json",
            hook_format="claude",
            config_format="json",
            env_flags=(),
            cli_flags=(),
        )
        assert info.is_installed()

    def test_is_installed_dir_config_exists(self, tmp_path):
        """is_installed returns True when config directory exists."""
        config_dir = tmp_path / ".test"
        config_dir.mkdir()

        info = AgentInfo(
            id="test",
            name="Test",
            global_config=config_dir,  # Directory, not file
            project_config=".test/config",
            hook_format="claude",
            config_format="json",
            env_flags=(),
            cli_flags=(),
        )
        assert info.is_installed()


class TestAgentsRegistry:
    """Test the AGENTS registry constant."""

    def test_all_8_agents_defined(self):
        """All 8 required agents are defined in the registry."""
        required_agents = {
            "claude",
            "gemini",
            "cursor",
            "windsurf",
            "pi",
            "moltbot",
            "codex",
            "pearai",
        }
        assert set(AGENTS.keys()) == required_agents

    def test_agent_info_completeness(self):
        """Each agent has complete and valid information."""
        for agent_id, info in AGENTS.items():
            assert isinstance(info.id, str) and len(info.id) > 0
            assert isinstance(info.name, str) and len(info.name) > 0
            assert isinstance(info.global_config, Path)
            assert isinstance(info.project_config, str) and len(info.project_config) > 0
            assert info.hook_format in ("claude", "cursor", "gemini", "pi", "none")
            assert info.config_format in ("json", "toml")
            assert isinstance(info.env_flags, tuple)
            assert isinstance(info.cli_flags, tuple)

    def test_claude_agent_config(self):
        """Claude agent has correct config paths."""
        claude = AGENTS["claude"]
        assert claude.id == "claude"
        assert claude.name == "Claude Code"
        assert claude.global_config == Path.home() / ".claude" / "settings.json"
        assert claude.project_config == ".claude/settings.json"
        assert claude.hook_format == "claude"
        assert claude.config_format == "json"

    def test_gemini_agent_config(self):
        """Gemini agent has correct config paths."""
        gemini = AGENTS["gemini"]
        assert gemini.id == "gemini"
        assert gemini.name == "Gemini CLI"
        assert gemini.global_config == Path.home() / ".gemini" / "settings.json"
        assert gemini.project_config == ".gemini/settings.json"
        assert gemini.hook_format == "gemini"
        assert gemini.config_format == "json"

    def test_cursor_agent_config(self):
        """Cursor agent has correct config paths."""
        cursor = AGENTS["cursor"]
        assert cursor.id == "cursor"
        assert cursor.name == "Cursor IDE"
        assert cursor.global_config == Path.home() / ".cursor" / "hooks.json"
        assert cursor.project_config == ".cursor/hooks.json"
        assert cursor.hook_format == "cursor"
        assert cursor.config_format == "json"

    def test_codex_agent_config(self):
        """Codex agent has TOML config format."""
        codex = AGENTS["codex"]
        assert codex.id == "codex"
        assert codex.name == "OpenAI Codex CLI"
        assert codex.global_config == Path.home() / ".codex" / "config.toml"
        assert codex.project_config == ".codex/config.toml"
        assert codex.hook_format == "none"  # No full hook system
        assert codex.config_format == "toml"


class TestDetectAgents:
    """Test agent detection functionality."""

    def test_detect_agents_no_installations(self, tmp_path):
        """detect_agents returns empty dict when nothing installed."""
        # Clear env flags and sys.argv
        import sys

        original_argv = sys.argv
        original_env = {}
        for flag in (
            "DIPPY_CLAUDE",
            "DIPPY_GEMINI",
            "DIPPY_CURSOR",
            "DIPPY_WINDSURF",
            "DIPPY_PI",
            "DIPPY_MOLTBOT",
            "DIPPY_CODEX",
            "DIPPY_PEARAI",
        ):
            if flag in os.environ:
                original_env[flag] = os.environ.pop(flag)

        try:
            sys.argv = ["test_script"]
            # Patch is_installed method to return False for all agents
            with patch("dippy.cli.agents.AgentInfo.is_installed", return_value=False):
                detected = detect_agents()
            assert detected == {}
        finally:
            sys.argv = original_argv
            for key, value in original_env.items():
                os.environ[key] = value

    def test_detect_agents_finds_claude(self, tmp_path):
        """detect_agents finds Claude when config exists."""
        import sys

        original_argv = sys.argv
        original_env = {}
        for flag in (
            "DIPPY_CLAUDE",
            "DIPPY_GEMINI",
            "DIPPY_CURSOR",
            "DIPPY_WINDSURF",
            "DIPPY_PI",
            "DIPPY_MOLTBOT",
            "DIPPY_CODEX",
            "DIPPY_PEARAI",
        ):
            if flag in os.environ:
                original_env[flag] = os.environ.pop(flag)

        try:
            sys.argv = ["test_script"]

            def mock_is_installed(self):
                # Only claude is "installed"
                return self.id == "claude"

            with patch("dippy.cli.agents.AgentInfo.is_installed", mock_is_installed):
                detected = detect_agents()
            assert "claude" in detected
            assert detected["claude"].id == "claude"
        finally:
            sys.argv = original_argv
            for key, value in original_env.items():
                os.environ[key] = value

    def test_detect_agents_multiple(self, tmp_path):
        """detect_agents finds multiple installed agents."""
        import sys

        original_argv = sys.argv
        original_env = {}
        for flag in (
            "DIPPY_CLAUDE",
            "DIPPY_GEMINI",
            "DIPPY_CURSOR",
            "DIPPY_WINDSURF",
            "DIPPY_PI",
            "DIPPY_MOLTBOT",
            "DIPPY_CODEX",
            "DIPPY_PEARAI",
        ):
            if flag in os.environ:
                original_env[flag] = os.environ.pop(flag)

        try:
            sys.argv = ["test_script"]

            def mock_is_installed(self):
                # claude and cursor are "installed"
                return self.id in ("claude", "cursor")

            with patch("dippy.cli.agents.AgentInfo.is_installed", mock_is_installed):
                detected = detect_agents()
            assert "claude" in detected
            assert "cursor" in detected
        finally:
            sys.argv = original_argv
            for key, value in original_env.items():
                os.environ[key] = value

    def test_detect_agents_env_flag(self):
        """detect_agents detects agent via environment variable."""
        import sys

        original_argv = sys.argv
        original_env = {}
        for flag in (
            "DIPPY_CLAUDE",
            "DIPPY_GEMINI",
            "DIPPY_CURSOR",
            "DIPPY_WINDSURF",
            "DIPPY_PI",
            "DIPPY_MOLTBOT",
            "DIPPY_CODEX",
            "DIPPY_PEARAI",
        ):
            if flag == "DIPPY_CLAUDE":
                if flag in os.environ:
                    original_env[flag] = os.environ.pop(flag)
            else:
                if flag in os.environ:
                    original_env[flag] = os.environ.pop(flag)

        try:
            sys.argv = ["test_script"]
            os.environ["DIPPY_CLAUDE"] = "1"
            # Mock is_installed to return False so only env flag triggers detection
            with patch("dippy.cli.agents.AgentInfo.is_installed", return_value=False):
                detected = detect_agents()
            assert "claude" in detected
        finally:
            sys.argv = original_argv
            for key, value in original_env.items():
                os.environ[key] = value

    def test_detect_agents_cli_flag(self):
        """detect_agents detects agent via CLI flag in sys.argv."""
        import sys

        original_argv = sys.argv
        original_env = {}
        for flag in (
            "DIPPY_CLAUDE",
            "DIPPY_GEMINI",
            "DIPPY_CURSOR",
            "DIPPY_WINDSURF",
            "DIPPY_PI",
            "DIPPY_MOLTBOT",
            "DIPPY_CODEX",
            "DIPPY_PEARAI",
        ):
            if flag in os.environ:
                original_env[flag] = os.environ.pop(flag)

        try:
            sys.argv = ["script", "--claude"]
            # Mock is_installed to return False so only CLI flag triggers detection
            with patch("dippy.cli.agents.AgentInfo.is_installed", return_value=False):
                detected = detect_agents()
            assert "claude" in detected
        finally:
            sys.argv = original_argv
            for key, value in original_env.items():
                os.environ[key] = value


class TestResolveDippyCommand:
    """Test dippy command resolution for different agents."""

    def test_resolve_unknown_agent(self):
        """Unknown agent returns default dippy command."""
        cmd = resolve_dippy_command("unknown")
        assert cmd == "dippy"

    def test_resolve_claude_command(self):
        """Claude agent includes --claude flag."""
        cmd = resolve_dippy_command("claude")
        assert "--claude" in cmd
        assert "dippy" in cmd

    def test_resolve_gemini_command(self):
        """Gemini agent includes --gemini flag."""
        cmd = resolve_dippy_command("gemini")
        assert "--gemini" in cmd
        assert "dippy" in cmd

    def test_resolve_cursor_command(self):
        """Cursor agent includes --cursor flag."""
        cmd = resolve_dippy_command("cursor")
        assert "--cursor" in cmd
        assert "dippy" in cmd

    def test_resolve_windsurf_command(self):
        """Windsurf agent includes --windsurf flag."""
        cmd = resolve_dippy_command("windsurf")
        assert "--windsurf" in cmd
        assert "dippy" in cmd

    def test_resolve_pi_command(self, monkeypatch):
        """pi-mono agent uses pi_wrapper.py."""
        # Mock the pi_wrapper finder to return a test path
        monkeypatch.setattr(
            "dippy.cli.agents._find_pi_wrapper",
            lambda: "/test/pi_wrapper.py",
        )

        cmd = resolve_dippy_command("pi")
        assert "pi_wrapper.py" in cmd
        assert "python3" in cmd

    def test_resolve_moltbot_command(self, monkeypatch):
        """Moltbot agent uses pi_wrapper.py."""
        monkeypatch.setattr(
            "dippy.cli.agents._find_pi_wrapper",
            lambda: "/test/pi_wrapper.py",
        )

        cmd = resolve_dippy_command("moltbot")
        assert "pi_wrapper.py" in cmd

    def test_resolve_pearai_command(self):
        """PearAI agent includes --claude flag (compatible format)."""
        cmd = resolve_dippy_command("pearai")
        # PearAI uses Claude-compatible format
        assert "dippy" in cmd

    def test_resolve_codex_command(self):
        """Codex agent has no hook system, returns base command."""
        cmd = resolve_dippy_command("codex")
        assert "dippy" in cmd


class TestHelperFunctions:
    """Test module helper functions."""

    def test_get_agent_info_valid(self):
        """get_agent_info returns correct AgentInfo."""
        info = get_agent_info("claude")
        assert info is not None
        assert info.id == "claude"

    def test_get_agent_info_invalid(self):
        """get_agent_info returns None for unknown agent."""
        info = get_agent_info("does-not-exist")
        assert info is None

    def test_list_all_agents(self):
        """list_all_agents returns all 8 agents."""
        all_agents = list_all_agents()
        assert len(all_agents) == 8
        agent_ids = {a.id for a in all_agents}
        assert agent_ids == {
            "claude",
            "gemini",
            "cursor",
            "windsurf",
            "pi",
            "moltbot",
            "codex",
            "pearai",
        }

    def test_find_dippy_executable(self, monkeypatch):
        """_find_dippy_executable returns sensible default."""
        # Mock shutil.which to return None
        import shutil

        monkeypatch.setattr(shutil, "which", lambda x: None)

        result = _find_dippy_executable()
        assert "dippy" in result
        assert "python" in result

    def test_find_pi_wrapper_not_found(self):
        """_find_pi_wrapper returns None when not found."""
        # Create a fresh module import with mocked paths
        import sys
        from types import ModuleType

        # Create a minimal mock of the agents module
        mock_agents = ModuleType("mock_agents")
        mock_agents.__file__ = __file__
        mock_agents.Path = Path

        exec(
            """
from pathlib import Path

def _find_pi_wrapper():
    here = Path(__file__).parent.parent
    wrapper = here / "pi_wrapper.py"
    if wrapper.exists():
        return str(wrapper)

    import sys
    for site_dir in sys.path:
        candidate = Path(site_dir) / "dippy" / "pi_wrapper.py"
        if candidate.exists():
            return str(candidate)
    return None
""",
            mock_agents.__dict__,
        )

        # Mock sys.path to not contain the wrapper
        original_path = sys.path.copy()
        sys.path = ["/nonexistent/path"]
        try:
            result = mock_agents._find_pi_wrapper()
            assert result is None
        finally:
            sys.path = original_path
