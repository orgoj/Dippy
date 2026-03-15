"""Tests for deny-format configuration and formatting."""

import json
from pathlib import Path

from dippy.core.config import parse_config
from dippy.pi_wrapper import (
    format_deny_reason,
    DEFAULT_DENY_FORMAT,
    DEFAULT_DENY_FORMATS,
)


class TestDenyFormatConfig:
    """Test deny-format configuration parsing."""

    def test_default_deny_format_is_set(self):
        """Default deny format should be defined."""
        assert DEFAULT_DENY_FORMAT is not None
        assert "{command}" in DEFAULT_DENY_FORMAT
        assert "{reason}" in DEFAULT_DENY_FORMAT

    def test_default_deny_formats_for_agents(self):
        """Default deny formats for pi and claude should be defined."""
        assert "pi" in DEFAULT_DENY_FORMATS
        assert "claude" in DEFAULT_DENY_FORMATS
        assert "{command}" in DEFAULT_DENY_FORMATS["pi"]
        assert "{reason}" in DEFAULT_DENY_FORMATS["pi"]

    def test_parse_deny_format(self, tmp_path: Path):
        """Test parsing deny-format setting."""
        config_text = """
set deny-format "Custom format: {command} -> {reason}"
"""
        config = parse_config(config_text)
        assert config.deny_format == "Custom format: {command} -> {reason}"

    def test_parse_deny_format_agent_specific(self, tmp_path: Path):
        """Test parsing deny-format-pi and deny-format-claude settings."""
        config_text = """
set deny-format-pi "PI: {reason}"
set deny-format-claude "Claude: {reason}"
"""
        config = parse_config(config_text)
        assert config.deny_format_agents.get("pi") == "PI: {reason}"
        assert config.deny_format_agents.get("claude") == "Claude: {reason}"

    def test_merge_configs_deny_format(self):
        """Test that deny_format merges correctly (overlay wins)."""
        from dippy.core.config import _merge_configs, Config

        base = Config(deny_format="Base format")
        overlay = Config(deny_format="Overlay format")
        merged = _merge_configs(base, overlay)
        assert merged.deny_format == "Overlay format"

    def test_merge_configs_deny_format_agents(self):
        """Test that deny_format_agents merges correctly."""
        from dippy.core.config import _merge_configs

        base = parse_config('set deny-format-pi "Base pi format"')
        overlay = parse_config('set deny-format-claude "Claude format"')
        merged = _merge_configs(base, overlay)
        assert merged.deny_format_agents.get("pi") == "Base pi format"
        assert merged.deny_format_agents.get("claude") == "Claude format"


class TestFormatDenyReason:
    """Test deny reason formatting."""

    def test_format_with_default_template(self):
        """Test formatting with default template."""
        from dippy.core.config import Config

        config = Config()

        result = format_deny_reason(
            reason="find: Use rg instead",
            command="find . -name test",
            pattern="find",
            config=config,
            agent="unknown",
        )

        assert "find . -name test" in result
        assert "find: Use rg instead" in result

    def test_format_with_agent_specific_template(self):
        """Test formatting with agent-specific template."""
        from dippy.core.config import Config

        config = Config()

        result = format_deny_reason(
            reason="find: Use rg instead",
            command="find . -name test",
            pattern="find",
            config=config,
            agent="pi",
        )

        assert "find . -name test" in result
        assert "find: Use rg instead" in result
        assert "INSTRUCTION:" in result

    def test_format_with_custom_template(self):
        """Test formatting with custom template from config."""
        from dippy.core.config import Config

        config = Config(deny_format="CUSTOM: {command} says {reason}")

        result = format_deny_reason(
            reason="find: Use rg instead",
            command="find . -name test",
            pattern="find",
            config=config,
            agent="pi",
        )

        assert result == "CUSTOM: find . -name test says find: Use rg instead"

    def test_format_with_custom_agent_template(self):
        """Test that agent-specific template takes precedence."""
        from dippy.core.config import Config

        config = Config(
            deny_format="General: {reason}",
            deny_format_agents={"pi": "PI-specific: {reason}"},
        )

        result = format_deny_reason(
            reason="blocked", command="cmd", pattern="cmd", config=config, agent="pi"
        )

        assert result == "PI-specific: blocked"

    def test_format_with_none_command(self):
        """Test formatting when command is None."""
        from dippy.core.config import Config

        config = Config()

        result = format_deny_reason(
            reason="edit blocked", command=None, pattern=None, config=config, agent="pi"
        )

        # Should not crash, empty string for command
        assert "edit blocked" in result

    def test_format_extracts_pattern_from_reason(self):
        """Test pattern extraction from reason when not provided."""
        from dippy.core.config import Config

        config = Config(deny_format="Pattern: {pattern}, Reason: {reason}")

        result = format_deny_reason(
            reason="find: Use rg instead",
            command="find .",
            pattern=None,
            config=config,
            agent="pi",
        )

        # Should extract "find" from "find: Use rg instead"
        assert "Pattern: find" in result


class TestDenyFormatIntegration:
    """Integration tests with pi_wrapper."""

    def test_wrapper_uses_deny_format(self, tmp_path):
        """Test that pi_wrapper uses deny-format configuration."""
        import subprocess

        # Create test config
        config_file = tmp_path / "test.cfg"
        config_file.write_text("""
set default allow
allow python3 *
deny find * "Use rg instead"
set deny-format-pi "TEST_DENY: {command}|{reason}"
""")

        # Run pi_wrapper
        input_data = {
            "type": "bash",
            "command": "find .",
            "cwd": str(tmp_path),
            "agent": "pi",
        }
        result = subprocess.run(
            ["python3", "src/dippy/pi_wrapper.py"],
            input=json.dumps(input_data),
            capture_output=True,
            text=True,
            cwd="/home/michael/work/ai/CLAUDE/TOOLS/dippy-dev",
            env={**{"DIPPY_CONFIG": str(config_file), "PYTHONPATH": "src"}},
        )

        output = json.loads(result.stdout)
        assert output["action"] == "deny"
        assert "TEST_DENY:" in output["reason"]
        assert "find ." in output["reason"]
        assert "Use rg instead" in output["reason"]
