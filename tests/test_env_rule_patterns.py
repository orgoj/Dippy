"""Tests for `${NAME}` environment variables in rule patterns."""

from __future__ import annotations

import json
import logging
import os
import subprocess
import sys
from pathlib import Path

import pytest

from dippy.core.analyzer import analyze
from dippy.core.config import (
    match_after,
    match_after_mcp,
    match_after_web,
    match_edit,
    match_mcp,
    match_read,
    match_redirect,
    match_web,
    parse_config,
)

DIR_VAR = "DIPPY_TEST_RULE_DIR"
NAME_VAR = "DIPPY_TEST_RULE_NAME"
REPO_ROOT = Path(__file__).parent.parent


@pytest.fixture(autouse=True)
def _clean_env(monkeypatch):
    """Never let the test variables leak in from the real environment."""
    monkeypatch.delenv(DIR_VAR, raising=False)
    monkeypatch.delenv(NAME_VAR, raising=False)


def _decide(config, command: str, cwd: Path) -> str:
    return analyze(command, config, cwd).action


# === Expansion in each rule kind ===


def test_command_rules_expand(monkeypatch, tmp_path):
    monkeypatch.setenv(NAME_VAR, "fictool")
    for directive in ("allow", "ask", "deny"):
        config = parse_config(f"{directive} ${{{NAME_VAR}}} run *")
        assert _decide(config, "fictool run x", tmp_path) == directive
        assert _decide(config, "othertool run x", tmp_path) == "ask"


def test_delegate_rule_expands(monkeypatch, tmp_path):
    monkeypatch.setenv(NAME_VAR, "fictool")
    config = parse_config(f"delegate ${{{NAME_VAR}}} *")
    assert config.rules[0].pattern == "fictool *"
    assert config.rules[0].inactive is False


def test_option_block_rule_expands(monkeypatch, tmp_path):
    monkeypatch.setenv(NAME_VAR, "a*b")
    config = parse_config(f"allow [opts:--quiet] fictool ${{{NAME_VAR}}}")
    assert _decide(config, "fictool 'a*b'", tmp_path) == "allow"
    assert _decide(config, "fictool --quiet 'a*b'", tmp_path) == "allow"
    assert _decide(config, "fictool axxb", tmp_path) == "ask"


def test_option_block_reference_inside_quotes_is_an_error(monkeypatch, caplog):
    monkeypatch.setenv(NAME_VAR, "x")
    with caplog.at_level(logging.WARNING):
        config = parse_config(f"allow [opts:--quiet] fictool '${{{NAME_VAR}}}'")
    assert config.rules == []
    assert "variable reference inside quotes" in caplog.text


def test_option_items_rule_expands(monkeypatch, tmp_path):
    monkeypatch.setenv(NAME_VAR, "--forcex")
    config = parse_config(f"deny-opt fictool ${{{NAME_VAR}}}")
    assert _decide(config, "fictool push --forcex", tmp_path) == "deny"
    assert _decide(config, "fictool push --force", tmp_path) == "ask"


def test_option_items_prefix_expands(monkeypatch, tmp_path):
    monkeypatch.setenv(NAME_VAR, "fictool")
    config = parse_config(f"deny-opt ${{{NAME_VAR}}} --forcex")
    assert _decide(config, "fictool push --forcex", tmp_path) == "deny"


def test_redirect_rule_expands(monkeypatch, tmp_path):
    monkeypatch.setenv(DIR_VAR, str(tmp_path))
    config = parse_config(f"allow-redirect ${{{DIR_VAR}}}/out/**")
    assert match_redirect(f"{tmp_path}/out/a.txt", config, tmp_path).decision == (
        "allow"
    )
    assert match_redirect(f"{tmp_path}/other/a.txt", config, tmp_path) is None


def test_edit_and_read_rules_expand(monkeypatch, tmp_path):
    monkeypatch.setenv(DIR_VAR, str(tmp_path))
    config = parse_config(
        f"allow-edit ${{{DIR_VAR}}}/src/**\ndeny-read ${{{DIR_VAR}}}/secret/*"
    )
    assert match_edit(f"{tmp_path}/src/a.py", config, tmp_path).decision == "allow"
    assert match_edit(f"{tmp_path}/a.py", config, tmp_path) is None
    assert match_read(f"{tmp_path}/secret/k", config, tmp_path).decision == "deny"
    assert match_read(f"{tmp_path}/k", config, tmp_path) is None


def test_mcp_and_web_rules_expand(monkeypatch):
    monkeypatch.setenv(NAME_VAR, "fictserver")
    config = parse_config(
        f"allow-mcp mcp__${{{NAME_VAR}}}__*\n"
        f'deny-mcp mcp__x__${{{NAME_VAR}}} "no"\n'
        f"allow-web docs ${{{NAME_VAR}}} *\n"
        f'after-mcp mcp__${{{NAME_VAR}}}__get "seen"\n'
        f'after-web ${{{NAME_VAR}}} * "web seen"'
    )
    assert match_mcp("mcp__fictserver__get", config).decision == "allow"
    assert match_mcp("mcp__other__get", config) is None
    assert match_mcp("mcp__x__fictserver", config).decision == "deny"
    assert match_web("docs fictserver api", config).decision == "allow"
    assert match_web("docs other api", config) is None
    assert match_after_mcp("mcp__fictserver__get", config) == "seen"
    assert match_after_web("fictserver q", config) == "web seen"


def test_after_rule_expands(monkeypatch, tmp_path):
    monkeypatch.setenv(NAME_VAR, "fictool")
    config = parse_config(f'after ${{{NAME_VAR}}} push * "pushed"')
    assert match_after(["fictool", "push", "x"], config, tmp_path) == "pushed"
    assert match_after(["other", "push", "x"], config, tmp_path) is None


def test_context_flags_combine_with_expansion(monkeypatch, tmp_path):
    monkeypatch.setenv(NAME_VAR, "fictool")
    config = parse_config(f"allow [@subshell] ${{{NAME_VAR}}} *")
    assert _decide(config, "(fictool x)", tmp_path) == "allow"
    assert _decide(config, "fictool x", tmp_path) == "ask"


# === Unset and empty variables ===


@pytest.mark.parametrize("value", [None, ""])
def test_unset_or_empty_variable_makes_rule_inactive(
    monkeypatch, tmp_path, caplog, value
):
    if value is not None:
        monkeypatch.setenv(NAME_VAR, value)
    text = f"deny fictool *\nallow ${{{NAME_VAR}}}fictool *\nask other *"
    with caplog.at_level(logging.WARNING):
        config = parse_config(text, source="/cfg/rules")
    assert [r.inactive for r in config.rules] == [False, True, False]
    assert _decide(config, "fictool x", tmp_path) == "deny"
    assert _decide(config, "other x", tmp_path) == "ask"
    warnings = [r.getMessage() for r in caplog.records]
    assert warnings == [
        f"/cfg/rules: line 2: ${{{NAME_VAR}}}: unset or empty (rule inactive)"
    ]


def test_unset_variable_never_matches_empty_string(tmp_path):
    """`allow ${UNSET}*` must not turn into `allow *`."""
    config = parse_config(f"allow ${{{NAME_VAR}}}*")
    assert _decide(config, "fictool x", tmp_path) == "ask"


def test_inactive_rule_does_not_change_default(tmp_path):
    config = parse_config(f"set default allow\ndeny ${{{NAME_VAR}}} *")
    assert config.default == "allow"
    assert config.rules[0].inactive is True


def test_unset_path_rule_inactive(tmp_path):
    config = parse_config(f"allow-edit ${{{DIR_VAR}}}/**")
    assert match_edit(f"{tmp_path}/a", config, tmp_path) is None
    assert match_edit("/a", config, tmp_path) is None


# === Literal values ===


@pytest.mark.parametrize("value", ["*", "?", "[a-z]", "**"])
def test_glob_metacharacters_in_value_are_literal(monkeypatch, tmp_path, value):
    monkeypatch.setenv(NAME_VAR, value)
    config = parse_config(f"allow fictool ${{{NAME_VAR}}}")
    assert _decide(config, "fictool x", tmp_path) == "ask"
    assert _decide(config, "fictool rm -rf /", tmp_path) == "ask"


def test_star_value_matches_only_itself(monkeypatch, tmp_path):
    monkeypatch.setenv(NAME_VAR, "*")
    config = parse_config(f"allow fictool ${{{NAME_VAR}}} *")
    assert _decide(config, "fictool '*' x", tmp_path) == "allow"
    assert _decide(config, "fictool y x", tmp_path) == "ask"


def test_star_value_in_path_rule_is_literal(monkeypatch, tmp_path):
    monkeypatch.setenv(NAME_VAR, "*")
    config = parse_config(f"allow-edit {tmp_path}/${{{NAME_VAR}}}/**")
    assert match_edit(f"{tmp_path}/*/a", config, tmp_path).decision == "allow"
    assert match_edit(f"{tmp_path}/b/a", config, tmp_path) is None


def test_star_value_in_mcp_and_web_rules_is_literal(monkeypatch):
    monkeypatch.setenv(NAME_VAR, "*")
    config = parse_config(f"allow-mcp ${{{NAME_VAR}}}\nallow-web ${{{NAME_VAR}}}")
    assert match_mcp("mcp__a__b", config) is None
    assert match_mcp("*", config).decision == "allow"
    assert match_web("anything", config) is None


def test_value_is_never_tilde_expanded(monkeypatch, tmp_path, caplog):
    monkeypatch.setenv(DIR_VAR, "~")
    with caplog.at_level(logging.WARNING):
        config = parse_config(
            f"allow-edit ${{{DIR_VAR}}}/**\nallow ${{{DIR_VAR}}}/bin/x *"
        )
    assert [r.inactive for r in config.edit_rules + config.rules] == [True, True]
    assert match_edit(str(Path.home() / "a"), config, tmp_path) is None
    assert "value starts with '~'" in caplog.text


def test_tilde_inside_value_is_literal(monkeypatch, tmp_path):
    monkeypatch.setenv(NAME_VAR, "a~b")
    config = parse_config(f"allow-edit {tmp_path}/${{{NAME_VAR}}}/*")
    assert match_edit(f"{tmp_path}/a~b/x", config, tmp_path).decision == "allow"


def test_written_tilde_keeps_its_meaning(monkeypatch, tmp_path):
    monkeypatch.setenv(NAME_VAR, "proj")
    config = parse_config(f"allow-edit ~/${{{NAME_VAR}}}/**")
    home = Path.home()
    assert match_edit(f"{home}/proj/a", config, tmp_path).decision == "allow"


# === Lookalike paths ===


def test_lookalike_agent_directories(monkeypatch, tmp_path):
    hcom = tmp_path / "hcom"
    monkeypatch.setenv(DIR_VAR, str(hcom))
    monkeypatch.setenv(NAME_VAR, "bot1")
    config = parse_config(
        f"allow-edit ${{{DIR_VAR}}}/agents/${{{NAME_VAR}}}/**\n"
        f"allow ${{{DIR_VAR}}}/agents/${{{NAME_VAR}}}/bin/* *"
    )
    assert match_edit(f"{hcom}/agents/bot1/x", config, tmp_path).decision == "allow"
    for path in ("agents/bot1_evil/x", "agents/bot10/x", "agents/x", "agents/bot1"):
        assert match_edit(f"{hcom}/{path}", config, tmp_path) is None, path
    assert _decide(config, f"{hcom}/agents/bot1/bin/tool x", tmp_path) == "allow"
    assert _decide(config, f"{hcom}/agents/bot10/bin/tool x", tmp_path) == "ask"
    assert _decide(config, f"{hcom}/agents/bot1_evil/bin/tool x", tmp_path) == "ask"


# === Unsafe values ===


@pytest.mark.parametrize(
    ("value", "reason"),
    [
        ("../x", "value contains a '.' or '..' path component"),
        (".", "value contains a '.' or '..' path component"),
        ("a/./b", "value contains a '.' or '..' path component"),
        ("a\nb", "value contains a NUL or newline"),
    ],
)
def test_unsafe_path_values(monkeypatch, tmp_path, caplog, value, reason):
    monkeypatch.setenv(NAME_VAR, value)
    with caplog.at_level(logging.WARNING):
        config = parse_config(f"allow-edit {tmp_path}/agents/${{{NAME_VAR}}}/**")
    assert config.edit_rules[0].inactive is True
    assert match_edit(f"{tmp_path}/x", config, tmp_path) is None
    assert match_edit(f"{tmp_path}/agents/x/y", config, tmp_path) is None
    assert caplog.records[0].getMessage() == (
        f"line 1: ${{{NAME_VAR}}}: {reason} (rule inactive)"
    )


def test_space_in_path_value_is_allowed(monkeypatch, tmp_path):
    monkeypatch.setenv(NAME_VAR, "a b")
    config = parse_config(f"allow-edit {tmp_path}/${{{NAME_VAR}}}/*")
    assert match_edit(f"{tmp_path}/a b/x", config, tmp_path).decision == "allow"


@pytest.mark.parametrize("value", ["a b", "a\tb", "fictool *"])
def test_whitespace_in_command_value_is_unsafe(monkeypatch, tmp_path, caplog, value):
    monkeypatch.setenv(NAME_VAR, value)
    with caplog.at_level(logging.WARNING):
        config = parse_config(f"allow ${{{NAME_VAR}}}")
    assert config.rules[0].inactive is True
    assert _decide(config, "fictool x", tmp_path) == "ask"
    assert "value contains whitespace" in caplog.text


def test_whitespace_in_option_item_value_is_unsafe(monkeypatch, tmp_path):
    monkeypatch.setenv(NAME_VAR, "--a --b")
    config = parse_config(f"allow-opt fictool ${{{NAME_VAR}}}")
    assert config.rules[0].inactive is True
    assert _decide(config, "fictool --a", tmp_path) == "ask"


def test_dot_component_in_command_value_is_unsafe(monkeypatch, tmp_path):
    monkeypatch.setenv(NAME_VAR, "..")
    config = parse_config(f"allow {tmp_path}/a/${{{NAME_VAR}}}/bin/* *")
    assert config.rules[0].inactive is True
    assert _decide(config, f"{tmp_path}/bin/x y", tmp_path) == "ask"


def test_slash_in_value_is_allowed(monkeypatch, tmp_path):
    monkeypatch.setenv(NAME_VAR, "a/b")
    config = parse_config(f"allow-read {tmp_path}/${{{NAME_VAR}}}/*")
    assert match_read(f"{tmp_path}/a/b/x", config, tmp_path).decision == "allow"


# === Syntax ===


def test_double_dollar_writes_literal_reference(monkeypatch, tmp_path):
    monkeypatch.setenv(NAME_VAR, "fictool")
    config = parse_config(f"allow fictecho $${{{NAME_VAR}}}")
    rule = config.rules[0]
    assert rule.pattern == f"fictecho ${{{NAME_VAR}}}"
    assert rule.inactive is False
    assert _decide(config, f"fictecho '${{{NAME_VAR}}}'", tmp_path) == "allow"
    assert _decide(config, "fictecho fictool", tmp_path) == "ask"


def test_dollar_name_without_braces_is_literal(monkeypatch, tmp_path):
    monkeypatch.setenv(NAME_VAR, "fictool")
    config = parse_config(f"allow-edit {tmp_path}/${NAME_VAR}/*")
    rule = config.edit_rules[0]
    assert rule.pattern == f"{tmp_path}/${NAME_VAR}/*"
    assert rule.written is None
    assert match_edit(f"{tmp_path}/fictool/x", config, tmp_path) is None


@pytest.mark.parametrize(
    ("pattern", "error"),
    [
        ("fictool ${", "unterminated variable reference '${'"),
        ("fictool ${NAME", "unterminated variable reference '${NAME'"),
        ("fictool ${}", "invalid variable reference '${}'"),
        ("fictool ${1A}", "invalid variable reference '${1A}'"),
        ("fictool ${A-B}", "invalid variable reference '${A-B}'"),
        ("fictool ${A B}", "invalid variable reference '${A B}'"),
    ],
)
def test_invalid_reference_is_a_config_error(caplog, pattern, error):
    with caplog.at_level(logging.WARNING):
        config = parse_config(f"allow {pattern}", source="/cfg/rules")
    assert config.rules == []
    assert [r.getMessage() for r in caplog.records] == [
        f"/cfg/rules: line 1: {error} (skipped)"
    ]


@pytest.mark.parametrize(
    "line",
    [
        "allow-edit /x/${}",
        "deny-read /x/${",
        "ask-redirect /x/${1}",
        "allow-mcp mcp__${A.B}",
        "allow-web ${-}",
        "after fictool ${",
        "allow-opt fictool ${",
    ],
)
def test_invalid_reference_in_every_rule_kind(caplog, line):
    with caplog.at_level(logging.WARNING):
        config = parse_config(line)
    rules = (
        config.rules
        + config.edit_rules
        + config.read_rules
        + config.redirect_rules
        + config.mcp_rules
        + config.web_rules
        + config.after_rules
    )
    assert rules == []
    assert "variable reference" in caplog.text


def test_messages_settings_and_flags_are_not_expanded(monkeypatch):
    monkeypatch.setenv(NAME_VAR, "fictool")
    config = parse_config(
        f'deny other * "use ${{{NAME_VAR}}}"\n'
        f'set approval-wait-message "wait ${{{NAME_VAR}}}"\n'
        f"allow [${NAME_VAR}=x] other2 *"
    )
    assert config.rules[0].message == f"use ${{{NAME_VAR}}}"
    assert config.approval_wait_message == f"wait ${{{NAME_VAR}}}"
    assert config.rules[1].required_flags == frozenset({f"${NAME_VAR}=x"})


# === Last-match-wins ===


def test_last_match_wins_with_env_rules(monkeypatch, tmp_path):
    monkeypatch.setenv(NAME_VAR, "bot1")
    config = parse_config(
        f"deny-edit {tmp_path}/agents/**\n"
        f"allow-edit {tmp_path}/agents/${{{NAME_VAR}}}/**\n"
        f"deny-edit {tmp_path}/agents/*/secret/**\n"
    )
    assert match_edit(f"{tmp_path}/agents/bot1/a", config, tmp_path).decision == (
        "allow"
    )
    assert match_edit(f"{tmp_path}/agents/bot2/a", config, tmp_path).decision == (
        "deny"
    )
    assert (
        match_edit(f"{tmp_path}/agents/bot1/secret/k", config, tmp_path).decision
        == "deny"
    )


def test_inactive_rule_keeps_earlier_decision(tmp_path):
    config = parse_config(
        f"allow-edit {tmp_path}/**\ndeny-edit {tmp_path}/${{{NAME_VAR}}}/**\n"
    )
    assert match_edit(f"{tmp_path}/a/b", config, tmp_path).decision == "allow"


# === Display ===


def test_match_shows_pattern_as_written(monkeypatch, tmp_path):
    monkeypatch.setenv(NAME_VAR, "fictool")
    monkeypatch.setenv(DIR_VAR, str(tmp_path))
    config = parse_config(
        f'ask ${{{NAME_VAR}}} * "check"\n'
        f"ask ${{{NAME_VAR}}}2 *\n"
        f"deny-edit ${{{DIR_VAR}}}/*\n"
        f"allow-mcp mcp__${{{NAME_VAR}}}__*\n"
        f"deny-opt ${{{NAME_VAR}}} --x"
    )
    assert config.rules[0].pattern == "fictool *"
    assert config.rules[0].written == f"${{{NAME_VAR}}} *"
    assert analyze("fictool x", config, tmp_path).reason == "fictool: check"
    assert match_edit(f"{tmp_path}/a", config, tmp_path).pattern == (
        f"${{{DIR_VAR}}}/*"
    )
    assert match_mcp("mcp__fictool__a", config).pattern == f"mcp__${{{NAME_VAR}}}__*"
    assert f"${{{NAME_VAR}}}2 *" in analyze("fictool2 x", config, tmp_path).reason
    deny = analyze("fictool push --x", config, tmp_path)
    assert deny.action == "deny"
    assert f"${{{NAME_VAR}}}" in deny.reason


# === Hook mode end to end ===


def _run_hook(tmp_path: Path, payload: dict, env_extra: dict, mode: str) -> dict:
    home = tmp_path / "home"
    home.mkdir(exist_ok=True)
    askpass_log = tmp_path / "askpass.json"
    askpass = tmp_path / "fake-askpass"
    askpass.write_text(f"#!/bin/sh\ncat > {askpass_log}\nexit 2\n")
    askpass.chmod(0o755)
    env = {
        "HOME": str(home),
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "PYTHONPATH": str(REPO_ROOT / "src"),
        "DIPPY_ASKPASS": str(askpass),
        **env_extra,
    }
    result = subprocess.run(
        [sys.executable, "-m", "dippy", mode],
        input=json.dumps(payload),
        capture_output=True,
        text=True,
        cwd=tmp_path,
        env=env,
        timeout=60,
    )
    assert result.returncode == 0, result.stderr
    return json.loads(result.stdout)


def _claude_decision(output: dict) -> str:
    return output.get("hookSpecificOutput", {}).get("permissionDecision")


def test_claude_hook_end_to_end(tmp_path):
    agents = tmp_path / "agents"
    log = tmp_path / "audit.log"
    config = tmp_path / "rules"
    config.write_text(
        f"set log {log}\n"
        f"allow-edit ${{{DIR_VAR}}}/agents/${{{NAME_VAR}}}/**\n"
        f'ask ${{{NAME_VAR}}}-tool * "check it"\n'
    )
    edit = {
        "hook_event_name": "PreToolUse",
        "tool_name": "Write",
        "tool_input": {"file_path": f"{agents}/bot1/x.txt", "content": "x"},
        "cwd": str(tmp_path),
    }
    env = {"DIPPY_CONFIG": str(config), DIR_VAR: str(tmp_path), NAME_VAR: "bot1"}
    assert _claude_decision(_run_hook(tmp_path, edit, env, "--claude")) == "allow"

    other = dict(edit, tool_input={"file_path": f"{agents}/bot10/x.txt"})
    assert _claude_decision(_run_hook(tmp_path, other, env, "--claude")) != "allow"

    unset = {"DIPPY_CONFIG": str(config)}
    assert _claude_decision(_run_hook(tmp_path, edit, unset, "--claude")) != "allow"

    bash = {
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": "bot1-tool run"},
        "cwd": str(tmp_path),
    }
    assert _claude_decision(_run_hook(tmp_path, bash, env, "--claude")) == "ask"

    entries = [json.loads(line) for line in log.read_text().splitlines()]
    rules = [entry.get("rule") for entry in entries]
    assert f"${{{DIR_VAR}}}/agents/${{{NAME_VAR}}}/**" in rules
    assert all("bot1" not in (rule or "") for rule in rules)


def test_agy_askpass_receives_rule_as_written(tmp_path):
    config = tmp_path / "rules"
    config.write_text(f"ask-edit ${{{DIR_VAR}}}/agents/*\n")
    payload = {
        "toolCall": {
            "name": "write_to_file",
            "args": {"TargetFile": f"{tmp_path}/agents/a", "Cwd": str(tmp_path)},
        },
        "workspacePaths": [str(tmp_path)],
    }
    env = {"DIPPY_CONFIG": str(config), DIR_VAR: str(tmp_path)}
    assert _run_hook(tmp_path, payload, env, "--agy")["decision"] == "deny"
    sent = json.loads((tmp_path / "askpass.json").read_text())
    assert f"[${{{DIR_VAR}}}/agents/* @ {config}]" in sent["message"]


@pytest.mark.parametrize(
    "line",
    [
        "allow fictool [${N}]",
        "allow fictool [!${N}]x",
        "allow-edit /w/[${N}]/*",
        "allow-mcp mcp__[a${N}]",
        "allow-web [${N}",
    ],
)
def test_reference_inside_bracket_expression_is_an_error(monkeypatch, caplog, line):
    """A value inside `[...]` would be a set of characters, not literal text."""
    monkeypatch.setenv("N", "a-z")
    with caplog.at_level(logging.WARNING):
        config = parse_config(line)
    assert config.rules + config.edit_rules + config.mcp_rules + config.web_rules == []
    assert "variable reference inside a bracket expression '${N}'" in caplog.text


def test_reference_after_closed_bracket_is_expanded(monkeypatch, tmp_path):
    monkeypatch.setenv(NAME_VAR, "a*")
    config = parse_config(
        f"allow fictool [xy]${{{NAME_VAR}}} *\nallow-opt fict [${{{NAME_VAR}}}]"
    )
    assert _decide(config, "fictool 'xa*' z", tmp_path) == "allow"
    assert _decide(config, "fictool xab z", tmp_path) == "ask"
    assert config.rules[1].items == ["[a*]"]
