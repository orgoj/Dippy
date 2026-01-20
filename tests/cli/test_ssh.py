"""
Tests for SSH CLI handler.

SSH commands with remote command execution are delegated to inner command check.
"""

import pytest

from conftest import is_approved, needs_confirmation


TESTS = [
    # === SAFE: Inner commands are safe ===
    ("ssh host ls", True),
    ("ssh host pwd", True),
    ("ssh host 'ls -la'", True),
    ("ssh host cat /etc/passwd", True),
    ("ssh user@host ls", True),
    ("ssh user@host 'git status'", True),
    ("ssh -p 22 host ls", True),
    ("ssh -i ~/.ssh/key host ls", True),
    ("ssh -t host ls", True),  # with tty allocation
    ("ssh -v host ls", True),  # verbose
    ("ssh host -- ls", True),  # explicit command separator
    #
    # === UNSAFE: Inner commands need confirmation ===
    ("ssh host rm file.txt", False),
    ("ssh host 'rm -rf /tmp/test'", False),
    ("ssh host npm install", False),
    ("ssh host 'pip install requests'", False),
    ("ssh user@host rm file.txt", False),
    ("ssh -p 2222 host rm file.txt", False),
    ("ssh -t host rm file.txt", False),
    #
    # === UNSAFE: Interactive SSH sessions ===
    ("ssh host", False),
    ("ssh user@host", False),
    ("ssh -t host", False),
    ("ssh -p 22 host", False),
    ("ssh -i ~/.ssh/key host", False),
    #
    # === EDGE CASES ===
    ("ssh", False),  # No target
]


@pytest.mark.parametrize("command,expected", TESTS)
def test_command(check, command: str, expected: bool) -> None:
    """Test that SSH command safety is detected correctly."""
    result = check(command)
    if expected:
        assert is_approved(result), f"Expected approved for: {command}"
    else:
        assert needs_confirmation(result), f"Expected confirmation for: {command}"


class TestSshWrapperContext:
    """Test that SSH sets wrapper_context for context-aware rules."""

    def test_ssh_sets_wrapper_context(self):
        """SSH handler returns wrapper_context=['ssh'] for delegate actions."""
        from dippy.cli.ssh import classify

        result = classify(["ssh", "host", "rm", "/tmp/x"])
        assert result.action == "delegate"
        assert result.inner_command == "rm /tmp/x"
        assert result.wrapper_context == ["ssh"]

    def test_ssh_no_wrapper_context_for_interactive(self):
        """SSH handler does not set wrapper_context for ask actions."""
        from dippy.cli.ssh import classify

        result = classify(["ssh", "host"])
        assert result.action == "ask"
        # wrapper_context is None or not present for non-delegate actions
        assert result.wrapper_context is None
