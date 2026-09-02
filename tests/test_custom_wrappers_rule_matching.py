"""Additional tests for wrapper rule matching acceptance criteria."""

from pathlib import Path

import pytest

from dippy.core.config import parse_config
from dippy.core.analyzer import analyze


def test_wrapper_both_flags_match():
    """allow [wrap,server1] free * matches wrap server1 free -h"""
    config = parse_config(
        """
        wrapper wrap --context-first
        allow [wrap,server1] free *
    """
    )
    result = analyze("wrap server1 free -h", config, Path.cwd())
    assert result.action == "allow"


def test_wrapper_dest_flag_only():
    """allow [server1] free * matches (dest flag only)"""
    config = parse_config(
        """
        wrapper wrap --context-first
        allow [server1] free *
    """
    )
    result = analyze("wrap server1 free -h", config, Path.cwd())
    assert result.action == "allow"


def test_wrapper_name_flag_only():
    """allow [wrap] free * matches (wrapper flag only)"""
    config = parse_config(
        """
        wrapper wrap --context-first
        allow [wrap] free *
    """
    )
    result = analyze("wrap server1 free -h", config, Path.cwd())
    assert result.action == "allow"


def test_wrapper_deny_blocks_command():
    """deny [server1] rm * blocks wrap server1 rm /tmp/x"""
    config = parse_config(
        """
        wrapper wrap --context-first
        deny [server1] rm *
    """
    )
    result = analyze("wrap server1 rm /tmp/x", config, Path.cwd())
    assert result.action == "deny"


def test_wrapper_negation_ssh():
    """Flag matching works with negation: [!ssh]"""
    config = parse_config(
        """
        deny [!ssh] rm *
    """
    )
    # rm without ssh should be denied
    result = analyze("rm /tmp/x", config, Path.cwd())
    assert result.action == "deny"

    # rm with ssh should NOT match the negated rule (no rule = ask)
    result = analyze("ssh host rm /tmp/x", config, Path.cwd())
    assert result.action == "ask"


def test_wrapper_negation_custom():
    """Flag matching works with negation: [!server1]"""
    config = parse_config(
        """
        wrapper wrap --context-first
        deny [!server1] rm *
    """
    )
    # rm without server1 should be denied
    result = analyze("rm /tmp/x", config, Path.cwd())
    assert result.action == "deny"

    # rm with server1 should NOT match the negated rule (no rule = ask)
    result = analyze("wrap server1 rm /tmp/x", config, Path.cwd())
    assert result.action == "ask"


def test_wrapper_last_match_wins():
    """Last matching rule wins for wrapper commands"""
    config = parse_config(
        """
        wrapper wrap --context-first
        deny [server1] free *
        allow [server1] free *
    """
    )
    result = analyze("wrap server1 free -h", config, Path.cwd())
    assert result.action == "allow"


def test_wrapper_last_match_wins_with_flags():
    """Last matching rule wins, even with different flag specificity"""
    config = parse_config(
        """
        wrapper wrap --context-first
        deny [wrap,server1] rm *
        allow [wrap] rm *
    """
    )
    # Both rules match, last one wins (allow)
    result = analyze("wrap server1 rm /tmp/x", config, Path.cwd())
    assert result.action == "allow"

    # Generic rule also allows
    result = analyze("wrap server2 rm /tmp/x", config, Path.cwd())
    assert result.action == "allow"


def test_delegate_rule_opts_in_to_ssh_inner_analysis():
    """A scoped delegate rule bypasses only the outer SSH ask rule."""
    config = parse_config(
        """
        ask ssh *
        delegate [mp1_all] ssh *
        allow [ssh] tail *
    """
    )

    delegated = analyze(
        "ssh ferda7 'tail -n 20 /log/php/error_log'",
        config,
        Path.cwd(),
        frozenset({"mp1_all"}),
    )
    assert delegated.action == "allow"

    other_agent = analyze(
        "ssh ferda7 'tail -n 20 /log/php/error_log'", config, Path.cwd()
    )
    assert other_agent.action == "ask"


def test_delegate_rule_still_checks_compound_ssh_command():
    config = parse_config(
        """
        ask ssh *
        delegate [mp1_all] ssh *
        allow [ssh] tail *
    """
    )

    result = analyze(
        "ssh ferda7 'tail /log/php/error_log; rm -rf /tmp/x'",
        config,
        Path.cwd(),
        frozenset({"mp1_all"}),
    )
    assert result.action == "ask"


@pytest.mark.parametrize("redirect", [">", ">|", "3>", "3>>", "{fd}>", "<>", ">&"])
def test_delegate_rule_keeps_ssh_output_redirects_on_ask(redirect):
    config = parse_config(
        """
        ask ssh *
        delegate [mp1_all] ssh *
        allow [ssh] tail *
    """
    )

    result = analyze(
        f"ssh ferda7 'tail /log/php/error_log {redirect} /tmp/copied'",
        config,
        Path.cwd(),
        frozenset({"mp1_all"}),
    )
    assert result.action == "ask"


def test_delegate_rule_keeps_ssh_fd_duplication_safe():
    config = parse_config(
        """
        ask ssh *
        delegate [mp1_all] ssh *
        allow [ssh] tail *
    """
    )

    result = analyze(
        "ssh ferda7 'tail /log/php/error_log 2>&1'",
        config,
        Path.cwd(),
        frozenset({"mp1_all"}),
    )
    assert result.action == "allow"


def test_delegate_rule_rejects_ampersand_prefixed_redirect_filename():
    config = parse_config(
        """
        delegate [mp1_all] ssh *
        allow [ssh] tail *
    """
    )

    result = analyze(
        "ssh ferda7 \"tail /log/php/error_log > '&file'\"",
        config,
        Path.cwd(),
        frozenset({"mp1_all"}),
    )
    assert result.action == "ask"


def test_deny_after_delegate_rule_still_wins():
    config = parse_config(
        """
        ask ssh *
        delegate [mp1_all] ssh *
        deny [mp1_all] ssh *
        allow [ssh] tail *
    """
    )

    result = analyze(
        "ssh ferda7 'tail /log/php/error_log'",
        config,
        Path.cwd(),
        frozenset({"mp1_all"}),
    )
    assert result.action == "deny"


def test_ssh_handler_redirect_cannot_inherit_local_redirect_allow():
    config = parse_config(
        """
        delegate [mp1_all] ssh *
        ask [mp1_all,ssh] *
        delegate [mp1_all,ssh,ferda7] sed *
        allow-redirect /tmp/**
    """
    )

    result = analyze(
        "ssh ferda7 \"sed -n -e 'w /tmp/copied' /log/error\"",
        config,
        Path.cwd(),
        frozenset({"mp1_all"}),
    )
    assert result.action == "ask"
