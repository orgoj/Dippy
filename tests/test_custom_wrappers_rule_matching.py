"""Additional tests for wrapper rule matching acceptance criteria."""

from pathlib import Path

from dippy.core.config import parse_config
from dippy.core.analyzer import analyze


def test_wrapper_both_flags_match():
    """allow [wrap,server1] free * matches wrap server1 free -h"""
    config = parse_config(
        """
        wrapper wrap
        allow [wrap,server1] free *
    """
    )
    result = analyze("wrap server1 free -h", config, Path.cwd())
    assert result.action == "allow"


def test_wrapper_dest_flag_only():
    """allow [server1] free * matches (dest flag only)"""
    config = parse_config(
        """
        wrapper wrap
        allow [server1] free *
    """
    )
    result = analyze("wrap server1 free -h", config, Path.cwd())
    assert result.action == "allow"


def test_wrapper_name_flag_only():
    """allow [wrap] free * matches (wrapper flag only)"""
    config = parse_config(
        """
        wrapper wrap
        allow [wrap] free *
    """
    )
    result = analyze("wrap server1 free -h", config, Path.cwd())
    assert result.action == "allow"


def test_wrapper_deny_blocks_command():
    """deny [server1] rm * blocks wrap server1 rm /tmp/x"""
    config = parse_config(
        """
        wrapper wrap
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
        wrapper wrap
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
        wrapper wrap
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
        wrapper wrap
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
