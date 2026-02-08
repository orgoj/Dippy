"""Tests for the Universal Notifier (Sidekick) feature."""

from dippy.core.config import parse_config, Config
from dippy.core.notifier import should_run_notifier


class TestNotifierConfig:
    """Test parsing of notifier-related settings."""

    def test_parse_notifier_command(self):
        text = 'set notifier-command "check-mail --fast"'
        conf = parse_config(text)
        assert conf.notifier_command == "check-mail --fast"

    def test_parse_notifier_command_unquoted(self):
        text = "set notifier-command /usr/bin/check-mail"
        conf = parse_config(text)
        assert conf.notifier_command == "/usr/bin/check-mail"

    def test_parse_notifier_include(self):
        text = 'set notifier-include "git commit, Read, Edit, npm run lint"'
        conf = parse_config(text)
        assert conf.notifier_include == frozenset(
            {"git commit", "Read", "Edit", "npm run lint"}
        )

    def test_parse_notifier_include_unquoted(self):
        text = "set notifier-include Read, Edit"
        conf = parse_config(text)
        assert conf.notifier_include == frozenset({"Read", "Edit"})

    def test_parse_notifier_include_strip_whitespace(self):
        text = 'set notifier-include " git commit , Read "'
        conf = parse_config(text)
        assert conf.notifier_include == frozenset({"git commit", "Read"})

    def test_parse_notifier_include_empty(self):
        text = 'set notifier-include ""'
        conf = parse_config(text)
        # Empty string results in empty frozenset or None depending on implementation
        # Our implementation: [i.strip() for i in value.split(",") if i.strip()]
        assert conf.notifier_include == frozenset()


class TestNotifierFiltering:
    """Test should_run_notifier filtering logic."""

    def test_runs_always_if_no_include_set(self):
        conf = Config(notifier_command="foo")
        assert should_run_notifier(conf, tool_name="Read") is True
        assert should_run_notifier(conf, command="ls -la") is True

    def test_filters_by_tool_name(self):
        conf = Config(
            notifier_command="foo", notifier_include=frozenset({"Read", "Edit"})
        )
        assert should_run_notifier(conf, tool_name="Read") is True
        assert should_run_notifier(conf, tool_name="Edit") is True
        assert should_run_notifier(conf, tool_name="Bash") is False

    def test_filters_by_command_prefix(self):
        conf = Config(
            notifier_command="foo",
            notifier_include=frozenset({"git commit", "npm test"}),
        )
        assert should_run_notifier(conf, command="git commit -m 'feat'") is True
        assert should_run_notifier(conf, command="npm test -- --watch") is True
        assert should_run_notifier(conf, command="ls -la") is False

    def test_filters_combined(self):
        conf = Config(
            notifier_command="foo", notifier_include=frozenset({"Read", "git push"})
        )
        assert should_run_notifier(conf, tool_name="Read") is True
        assert should_run_notifier(conf, command="git push origin main") is True
        assert should_run_notifier(conf, tool_name="Edit") is False
        assert should_run_notifier(conf, command="git status") is False

    def test_no_command_set(self):
        conf = Config(notifier_command=None)
        assert should_run_notifier(conf, tool_name="Read") is False

    def test_empty_include_behaves_as_none(self):
        # If user explicitly sets to empty string, it might result in empty set
        conf = Config(notifier_command="foo", notifier_include=frozenset())
        # should_run_notifier checks: if not config.notifier_include: return True
        assert should_run_notifier(conf, tool_name="Read") is True
