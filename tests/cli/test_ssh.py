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
        from dippy.cli.ssh import classify, HandlerContext

        result = classify(HandlerContext(["ssh", "host", "rm", "/tmp/x"]))
        assert result.action == "delegate"
        assert result.inner_command == "rm /tmp/x"
        assert result.wrapper_context == ["ssh"]

    def test_ssh_no_wrapper_context_for_interactive(self):
        """SSH handler does not set wrapper_context for ask actions."""
        from dippy.cli.ssh import classify, HandlerContext

        result = classify(HandlerContext(["ssh", "host"]))
        assert result.action == "ask"
        # wrapper_context is None or not present for non-delegate actions
        assert result.wrapper_context is None

    def test_ssh_delegates_with_remote_flag(self):
        """SSH handler sets remote=True on delegate Classification.

        Regression test: SSH commands execute remotely, so inner commands
        must be analyzed with remote=True to avoid expanding paths against
        the local host cwd.
        """
        from dippy.cli.ssh import classify, HandlerContext

        result = classify(HandlerContext(["ssh", "host", "ls", "/tmp"]))
        assert result.action == "delegate"
        assert result.remote is True

    def test_ssh_interactive_no_remote_flag(self):
        """SSH interactive sessions (no command) don't need remote flag."""
        from dippy.cli.ssh import classify, HandlerContext

        result = classify(HandlerContext(["ssh", "host"]))
        assert result.action == "ask"
        assert result.remote is False


class TestSshJoinsWithoutRequoting:
    """ssh must NOT re-quote the remote command.

    Unlike sudo, env or uv run, ssh concatenates its arguments with spaces
    and hands the result to a remote *shell*. The remote metacharacters are
    syntax, so re-quoting the tokens would hide a compound command from
    analysis. This is why ssh keeps a plain join.
    """

    def test_remote_compound_is_analysed_as_two_commands(self, check):
        result = check("ssh host 'ls; pwd'")
        assert is_approved(result), "both remote commands are safe"

    def test_remote_compound_hides_nothing(self, check):
        result = check("ssh host 'ls; rm -rf /'")
        assert needs_confirmation(result), "the remote rm must still be seen"
