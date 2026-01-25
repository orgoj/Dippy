"""
Tests for sudo/doas/pkexec CLI handler.

Sudo commands with command execution are delegated to inner command check.
"""

import pytest

from conftest import is_approved, needs_confirmation


TESTS = [
    # === SAFE: Inner commands are safe ===
    ("sudo ls", True),
    ("sudo pwd", True),
    ("sudo ls -la", True),
    ("sudo cat /etc/passwd", True),
    ("sudo -u root ls", True),
    ("sudo -n ls", True),  # non-interactive
    ("sudo -- ls", True),  # explicit separator
    ("doas ls", True),  # OpenBSD alternative
    ("pkexec ls", True),  # polkit alternative
    #
    # === UNSAFE: Inner commands need confirmation ===
    ("sudo rm file.txt", False),
    ("sudo rm -rf /tmp/test", False),
    ("sudo npm install", False),
    ("sudo pip install requests", False),
    ("sudo -u www-data rm file.txt", False),
    ("doas rm file.txt", False),
    ("pkexec rm file.txt", False),
    #
    # === UNSAFE: Interactive shell modes ===
    ("sudo -i", False),  # login shell
    ("sudo -s", False),  # shell
    ("sudo --shell", False),
    ("sudo --login", False),
    ("sudo", False),  # no command
    ("sudo -u root -i", False),  # user + login shell
    ("doas -s", False),
    #
    # === EDGE CASES ===
    ("sudo -v", False),  # validate (refreshes sudo timeout)
    ("sudo -k", False),  # invalidate credentials
]


@pytest.mark.parametrize("command,expected", TESTS)
def test_command(check, command: str, expected: bool) -> None:
    """Test that sudo command safety is detected correctly."""
    result = check(command)
    if expected:
        assert is_approved(result), f"Expected approved for: {command}"
    else:
        assert needs_confirmation(result), f"Expected confirmation for: {command}"


class TestSudoWrapperContext:
    """Test that sudo sets wrapper_context for context-aware rules."""

    def test_sudo_sets_wrapper_context(self):
        """Sudo handler returns wrapper_context=['sudo'] for delegate actions."""
        from dippy.cli.sudo import classify, HandlerContext

        result = classify(HandlerContext(["sudo", "rm", "/tmp/x"]))
        assert result.action == "delegate"
        assert result.inner_command == "rm /tmp/x"
        assert result.wrapper_context == ["sudo"]

    def test_doas_sets_wrapper_context(self):
        """Doas handler returns wrapper_context=['sudo'] for delegate actions."""
        from dippy.cli.sudo import classify, HandlerContext

        result = classify(HandlerContext(["doas", "rm", "/tmp/x"]))
        assert result.action == "delegate"
        assert result.inner_command == "rm /tmp/x"
        assert result.wrapper_context == ["sudo"]

    def test_pkexec_sets_wrapper_context(self):
        """Pkexec handler returns wrapper_context=['sudo'] for delegate actions."""
        from dippy.cli.sudo import classify, HandlerContext

        result = classify(HandlerContext(["pkexec", "rm", "/tmp/x"]))
        assert result.action == "delegate"
        assert result.inner_command == "rm /tmp/x"
        assert result.wrapper_context == ["sudo"]

    def test_sudo_no_wrapper_context_for_interactive(self):
        """Sudo handler does not set wrapper_context for ask actions."""
        from dippy.cli.sudo import classify, HandlerContext

        result = classify(HandlerContext(["sudo", "-i"]))
        assert result.action == "ask"
        assert result.wrapper_context is None
