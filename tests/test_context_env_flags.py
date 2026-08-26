"""Tests for environment-derived context flags (`set context-env`)."""

from pathlib import Path

import pytest

from dippy.core.analyzer import analyze
from dippy.core.config import _merge_configs, parse_config


ENV_VAR = "DIPPY_TEST_AGENT"


@pytest.fixture(autouse=True)
def _clean_env(monkeypatch):
    """Ensure the test variable never leaks in from the real environment."""
    monkeypatch.delenv(ENV_VAR, raising=False)


def test_flag_requires_context_env_directive(monkeypatch):
    """Without 'set context-env' the variable is ignored."""
    monkeypatch.setenv(ENV_VAR, "mail")
    config = parse_config(f"allow [${ENV_VAR}=mail] fictionalcmd *")
    result = analyze("fictionalcmd x", config, Path.cwd())
    assert result.action == "ask"


def test_flag_matches_when_value_matches(monkeypatch):
    monkeypatch.setenv(ENV_VAR, "mail")
    config = parse_config(
        f"""
        set context-env {ENV_VAR}
        allow [${ENV_VAR}=mail] fictionalcmd *
    """
    )
    result = analyze("fictionalcmd x", config, Path.cwd())
    assert result.action == "allow"


def test_flag_does_not_match_other_value(monkeypatch):
    monkeypatch.setenv(ENV_VAR, "traffic")
    config = parse_config(
        f"""
        set context-env {ENV_VAR}
        allow [${ENV_VAR}=mail] fictionalcmd *
    """
    )
    result = analyze("fictionalcmd x", config, Path.cwd())
    assert result.action == "ask"


def test_unset_variable_is_fail_closed():
    """An allow rule guarded by an unset variable must not fire."""
    config = parse_config(
        f"""
        set context-env {ENV_VAR}
        allow [${ENV_VAR}=mail] fictionalcmd *
    """
    )
    result = analyze("fictionalcmd x", config, Path.cwd())
    assert result.action == "ask"


def test_empty_value_produces_no_flag(monkeypatch):
    monkeypatch.setenv(ENV_VAR, "")
    config = parse_config(
        f"""
        set context-env {ENV_VAR}
        allow [${ENV_VAR}=] fictionalcmd *
    """
    )
    result = analyze("fictionalcmd x", config, Path.cwd())
    assert result.action == "ask"


def test_negated_env_flag(monkeypatch):
    """[!$VAR=value] matches everywhere except that agent."""
    config = parse_config(
        f"""
        set context-env {ENV_VAR}
        deny [!${ENV_VAR}=mail] fictionalcmd *
    """
    )

    monkeypatch.setenv(ENV_VAR, "mail")
    assert analyze("fictionalcmd x", config, Path.cwd()).action == "ask"

    monkeypatch.setenv(ENV_VAR, "traffic")
    assert analyze("fictionalcmd x", config, Path.cwd()).action == "deny"


def test_env_flag_combines_with_wrapper_flag(monkeypatch):
    """Env flags reach the inner command of a custom wrapper."""
    monkeypatch.setenv(ENV_VAR, "mail")
    config = parse_config(
        f"""
        set context-env {ENV_VAR}
        wrapper fictionalwrap --cmd run --context -t
        allow [${ENV_VAR}=mail,fictionalwrap] fictionalcmd *
    """
    )

    result = analyze('fictionalwrap -t host1 run "fictionalcmd x"', config, Path.cwd())
    assert result.action == "allow"

    # Same command locally (no wrapper) must not match: wrapper flag is missing.
    assert analyze("fictionalcmd x", config, Path.cwd()).action == "ask"


def test_env_flag_combines_with_wrapper_destination(monkeypatch):
    """Env flag + wrapper destination flag can be combined."""
    monkeypatch.setenv(ENV_VAR, "mail")
    config = parse_config(
        f"""
        set context-env {ENV_VAR}
        wrapper fictionalwrap --cmd run --context -t
        allow [${ENV_VAR}=mail,host1] fictionalcmd *
    """
    )

    assert (
        analyze(
            'fictionalwrap -t host1 run "fictionalcmd x"', config, Path.cwd()
        ).action
        == "allow"
    )
    assert (
        analyze(
            'fictionalwrap -t host2 run "fictionalcmd x"', config, Path.cwd()
        ).action
        == "ask"
    )


def test_multiple_context_env_directives_accumulate(monkeypatch):
    """A second 'set context-env' must not overwrite the first."""
    monkeypatch.setenv(ENV_VAR, "mail")
    monkeypatch.setenv("DIPPY_TEST_TAG", "ops")
    config = parse_config(
        f"""
        set context-env {ENV_VAR}
        set context-env DIPPY_TEST_TAG
        allow [${ENV_VAR}=mail,$DIPPY_TEST_TAG=ops] fictionalcmd *
    """
    )
    assert config.context_env == (ENV_VAR, "DIPPY_TEST_TAG")
    assert analyze("fictionalcmd x", config, Path.cwd()).action == "allow"
    monkeypatch.delenv("DIPPY_TEST_TAG")


def test_context_env_merges_across_configs():
    """User and project configs both contribute watched variables."""
    user = parse_config(f"set context-env {ENV_VAR}")
    project = parse_config("set context-env DIPPY_TEST_TAG")
    merged = _merge_configs(user, project)
    assert merged.context_env == (ENV_VAR, "DIPPY_TEST_TAG")


def test_context_env_requires_a_value():
    """'set context-env' without a variable name is rejected."""
    config = parse_config("set context-env")
    assert config.context_env == ()


def test_read_rule_respects_context_env(monkeypatch):
    monkeypatch.setenv(ENV_VAR, "research")
    config = parse_config(
        f"""
        set context-env {ENV_VAR}
        allow-read [${ENV_VAR}=research] src/**
    """
    )
    from dippy.core.config import env_context_flags, match_read

    flags = env_context_flags(config)
    assert (
        match_read("/work/src/main.py", config, Path("/work"), context_flags=flags)
        is not None
    )
    assert (
        match_read("/work/src/main.py", config, Path("/work"), context_flags=None)
        is None
    )


def test_edit_rule_respects_context_env(monkeypatch):
    monkeypatch.setenv(ENV_VAR, "research")
    config = parse_config(
        f"""
        set context-env {ENV_VAR}
        allow-edit [${ENV_VAR}=research] tmp/**
    """
    )
    from dippy.core.config import env_context_flags, match_edit

    flags = env_context_flags(config)
    assert (
        match_edit("/work/tmp/notes.md", config, Path("/work"), context_flags=flags)
        is not None
    )
    assert (
        match_edit("/work/src/app.py", config, Path("/work"), context_flags=flags)
        is None
    )
    assert (
        match_edit("/work/tmp/notes.md", config, Path("/work"), context_flags=None)
        is None
    )


def test_web_rule_respects_context_env(monkeypatch):
    monkeypatch.setenv(ENV_VAR, "research")
    config = parse_config(
        f"""
        set context-env {ENV_VAR}
        allow-web [${ENV_VAR}=research] *python*
    """
    )
    from dippy.core.config import env_context_flags, match_web

    flags = env_context_flags(config)
    assert match_web("python docs", config, context_flags=flags) is not None
    assert match_web("python docs", config, context_flags=None) is None
