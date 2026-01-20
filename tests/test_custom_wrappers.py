"""Tests for custom wrapper command system."""

import pytest
from pathlib import Path

from dippy.core.config import parse_config, Config
from dippy.core.analyzer import analyze, _extract_wrapper_args


class TestWrapperExtraction:
    """Test wrapper argument extraction logic."""

    def test_wrapper_extraction_basic(self):
        """Basic wrapper with destination and command."""
        dest, inner = _extract_wrapper_args(["wrap", "server1", "free", "-h"])
        assert dest == "server1"
        assert inner == "free -h"

    def test_wrapper_extraction_quoted_command(self):
        """Wrapper with quoted command."""
        dest, inner = _extract_wrapper_args(["wrap", "server1", '"free -h"'])
        assert dest == "server1"
        assert inner == '"free -h"'

    def test_wrapper_extraction_with_options(self):
        """Wrapper with options before destination."""
        dest, inner = _extract_wrapper_args(
            ["wrap", "-p", "2222", "-l", "user", "server1", "ls"]
        )
        assert dest == "server1"
        assert inner == "ls"

    def test_wrapper_extraction_double_dash(self):
        """Wrapper with -- ending option parsing."""
        dest, inner = _extract_wrapper_args(["wrap", "--", "server1", "ls"])
        assert dest == "server1"
        assert inner == "ls"

    def test_wrapper_extraction_no_inner_command(self):
        """Wrapper with destination but no inner command (interactive)."""
        dest, inner = _extract_wrapper_args(["wrap", "server1"])
        assert dest == "server1"
        assert inner == ""

    def test_wrapper_extraction_no_destination(self):
        """Wrapper with no destination."""
        dest, inner = _extract_wrapper_args(["wrap"])
        assert dest is None
        assert inner == ""

    def test_wrapper_extraction_only_options(self):
        """Wrapper with only options, no destination."""
        dest, inner = _extract_wrapper_args(["wrap", "-p", "2222"])
        assert dest is None
        assert inner == ""


class TestConfigParser:
    """Test wrapper directive parsing."""

    def test_parse_single_wrapper(self):
        """Parse a single wrapper directive."""
        config = parse_config("wrapper wrap")
        assert config.wrappers == {"wrap"}

    def test_parse_multiple_wrappers(self):
        """Parse multiple wrapper directives."""
        config = parse_config(
            """
            wrapper wrap
            wrapper ssh
            wrapper tmux-cli
        """
        )
        assert config.wrappers == {"wrap", "ssh", "tmux-cli"}

    def test_duplicate_wrapper_warning(self, caplog):
        """Duplicate wrapper definition logs warning."""
        import logging

        config = parse_config(
            """
            wrapper wrap
            wrapper wrap
        """
        )
        assert config.wrappers == {"wrap"}
        assert "duplicate wrapper" in caplog.text.lower()

    def test_empty_wrapper_name_error(self, caplog):
        """Empty wrapper name logs error."""
        config = parse_config("wrapper")
        assert config.wrappers == set()
        assert any("requires a command name" in rec.message for rec in caplog.records)

    def test_wrapper_name_starting_with_dash_error(self, caplog):
        """Wrapper name starting with - logs error."""
        config = parse_config("wrapper -bad")
        assert config.wrappers == set()
        assert any("cannot start with '-'" in rec.message for rec in caplog.records)


class TestWrapperAnalysis:
    """Test wrapper command analysis."""

    def test_wrapper_with_inner_command_delegates(self):
        """Wrapper with inner command delegates to inner analysis."""
        config = parse_config(
            """
            wrapper wrap
            allow [wrap,server1] free *
        """
        )
        result = analyze("wrap server1 free -h", config, Path.cwd())
        assert result.action == "allow"

    def test_wrapper_context_flags_include_both(self):
        """Context flags include both wrapper name and destination."""
        config = parse_config(
            """
            wrapper wrap
            deny [wrap,server1] rm *
        """
        )
        result = analyze("wrap server1 rm /tmp/x", config, Path.cwd())
        assert result.action == "deny"
        assert "rm" in result.reason.lower()

    def test_wrapper_destination_only_flag(self):
        """Rule with only destination flag matches."""
        config = parse_config(
            """
            wrapper wrap
            allow [server1] free *
        """
        )
        result = analyze("wrap server1 free -h", config, Path.cwd())
        assert result.action == "allow"

    def test_wrapper_name_only_flag(self):
        """Rule with only wrapper name flag matches."""
        config = parse_config(
            """
            wrapper wrap
            allow [wrap] free *
        """
        )
        result = analyze("wrap server1 free -h", config, Path.cwd())
        assert result.action == "allow"

    def test_wrapper_negated_flag(self):
        """Negated wrapper flag works correctly."""
        config = parse_config(
            """
            wrapper wrap
            deny [!wrap] rm *
        """
        )
        # rm without wrap should be denied
        result = analyze("rm /tmp/x", config, Path.cwd())
        assert result.action == "deny"

        # rm with wrap should NOT match the negated rule (no rule = ask)
        result = analyze("wrap server1 rm /tmp/x", config, Path.cwd())
        assert result.action == "ask"  # No matching rule

    def test_wrapper_without_inner_command_asks(self):
        """Wrapper without inner command returns ask (interactive)."""
        config = parse_config("wrapper wrap")
        result = analyze("wrap server1", config, Path.cwd())
        assert result.action == "ask"
        assert "server1" in result.reason

    def test_wrapper_without_destination_asks(self):
        """Wrapper without destination returns ask."""
        config = parse_config("wrapper wrap")
        result = analyze("wrap", config, Path.cwd())
        assert result.action == "ask"

    def test_wrapper_with_options_delegates_correctly(self):
        """Wrapper with options skips them and extracts destination."""
        config = parse_config(
            """
            wrapper wrap
            allow [wrap,server1] ls *
        """
        )
        result = analyze("wrap -p 2222 server1 ls", config, Path.cwd())
        assert result.action == "allow"


class TestExistingWrappersStillWork:
    """Ensure existing ssh/sudo wrappers continue working."""

    def test_ssh_wrapper_context_still_works(self):
        """SSH handler still sets wrapper_context correctly."""
        config = parse_config("allow [ssh] free *")
        result = analyze("ssh host free -h", config, Path.cwd())
        assert result.action == "allow"

    def test_sudo_wrapper_context_still_works(self):
        """Sudo handler still sets wrapper_context correctly."""
        config = parse_config("allow [sudo] free *")
        result = analyze("sudo free -h", config, Path.cwd())
        assert result.action == "allow"

    def test_ssh_and_custom_wrapper_coexist(self):
        """SSH and custom wrappers can both be defined."""
        config = parse_config(
            """
            wrapper wrap
            allow [ssh] free *
            allow [wrap,server1] ls *
        """
        )
        # SSH should work
        result = analyze("ssh host free -h", config, Path.cwd())
        assert result.action == "allow"

        # Custom wrapper should also work
        result = analyze("wrap server1 ls", config, Path.cwd())
        assert result.action == "allow"


class TestWrapperConfigMerge:
    """Test wrapper merging across config scopes."""

    def test_wrappers_merge_with_union(self):
        """Wrappers from multiple configs merge via set union."""
        from dippy.core.config import Config, _merge_configs

        base = Config(wrappers={"wrap1"})
        overlay = Config(wrappers={"wrap2"})
        merged = _merge_configs(base, overlay)

        assert merged.wrappers == {"wrap1", "wrap2"}

    def test_duplicate_wrappers_deduplicate(self):
        """Duplicate wrapper names are deduplicated in merge."""
        from dippy.core.config import Config, _merge_configs

        base = Config(wrappers={"wrap"})
        overlay = Config(wrappers={"wrap"})
        merged = _merge_configs(base, overlay)

        assert merged.wrappers == {"wrap"}
