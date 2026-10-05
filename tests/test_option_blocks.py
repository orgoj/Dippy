"""Option-aware rules match literal argv without widening legacy globs."""

import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

from dippy.core.analyzer import analyze
from dippy.core import config as config_module
from dippy.core.config import load_config, parse_config


@pytest.fixture
def isolated(tmp_path, monkeypatch):
    home = tmp_path / "home"
    home.mkdir()
    monkeypatch.setenv("HOME", str(home))
    monkeypatch.setattr(config_module, "USER_CONFIG", home / ".dippy" / "config")
    monkeypatch.delenv("DIPPY_CONFIG", raising=False)
    monkeypatch.delenv("DIPPY_CONFIG_ONLY", raising=False)
    return tmp_path


def action(command, rule, cwd):
    config = parse_config("ask fictcli *\n" + rule)
    return analyze(command, config, cwd).action


HEADER_RULE = (
    'allow [opts: -s, -v, -H="Host: *", --header="Host: *"] fictcli http://10.*'
)


@pytest.mark.parametrize(
    "arguments",
    [
        "http://10.1.2.3/",
        "-s http://10.1.2.3/",
        "-vs http://10.1.2.3/",
        "-H 'Host: demo' http://10.1.2.3/",
        '-H "Host: demo" http://10.1.2.3/',
        r"-H Host:\ demo http://10.1.2.3/",
        '-H"Host: demo" http://10.1.2.3/',
        "-sH'Host: demo' http://10.1.2.3/",
        "--header 'Host: demo' http://10.1.2.3/",
        '--header="Host: demo" http://10.1.2.3/',
        "http://10.1.2.3/ -H 'Host: demo' -s",
        "-- http://10.1.2.3/",
        "http://10.1.2.3/ --",
        "-H 'Host: --upload-file' http://10.1.2.3/",
        "-H 'Host: literal $HOST' http://10.1.2.3/",
    ],
)
def test_permitted_forms(arguments, isolated):
    assert action("fictcli " + arguments, HEADER_RULE, isolated) == "allow"


@pytest.mark.parametrize(
    "arguments",
    [
        "-T /etc/passwd http://10.1.2.3/",
        "--upload-file=/etc/passwd http://10.1.2.3/",
        "-sT/etc/passwd http://10.1.2.3/",
        "-ss http://10.1.2.3/",
        "-s -s http://10.1.2.3/",
        "-H 'Host: one' -H 'Host: two' http://10.1.2.3/",
        "-H=Host:demo http://10.1.2.3/",
        "--header=Wrong:demo http://10.1.2.3/",
        "--header='Host: one' --header='Host: two' http://10.1.2.3/",
        "--head='Host: demo' http://10.1.2.3/",
        "-s=yes http://10.1.2.3/",
        "-H",
        "--header",
        "-H Host: demo http://10.1.2.3/",
        "http://10.1.2.3/ http://outside.example/",
        "http://outside.example/",
        "-- -s http://10.1.2.3/",
        '-H "Host: $HOST" http://10.1.2.3/',
        '-H "Host: $(echo demo)" http://10.1.2.3/',
        "-H 'Host: demo' http://10.*",
        "http://10.{1,2}/",
        "",
    ],
)
def test_unknown_options_and_uncertain_argv_do_not_match(arguments, isolated):
    assert action("fictcli " + arguments, HEADER_RULE, isolated) == "ask"


@pytest.mark.parametrize("value", ["-H=demo", "-H '=demo'", "-H'=demo'"])
def test_short_equals_is_part_of_value(value, isolated):
    rule = 'allow [opts: -H="=demo"] fictcli item'
    assert action("fictcli " + value + " item", rule, isolated) == "allow"


@pytest.mark.parametrize("arguments", ["", "one two", "--bad", "$VALUE", "~/x"])
def test_positional_glob_matches_exactly_one_literal_argument(arguments, isolated):
    assert action("fictcli " + arguments, "allow [opts:] fictcli *", isolated) == "ask"


def test_quoted_positional_glob_is_literal_and_one_argument(isolated):
    assert action("fictcli 'one two'", "allow [opts:] fictcli *", isolated) == "allow"
    assert action("fictcli '*'", "allow [opts:] fictcli *", isolated) == "allow"


def test_option_value_is_not_path_normalized(isolated):
    rule = 'allow [opts: --path="relative/*"] fictcli inspect'
    assert action("fictcli --path relative/file inspect", rule, isolated) == "allow"


def test_quoted_commas_and_glob_brackets_in_declarations(isolated):
    rule = 'allow [opts: --label="a,b[12]", -s] fictcli item'
    assert action("fictcli --label a,b1 -s item", rule, isolated) == "allow"
    assert action("fictcli --label a,b3 item", rule, isolated) == "ask"


def test_unquoted_glob_brackets_in_declarations(isolated):
    rule = "allow [opts: --label=[ab]] fictcli item"
    assert action("fictcli --label a item", rule, isolated) == "allow"


@pytest.mark.parametrize(
    "block",
    [
        "[opts: -s, -s]",
        "[opts: word]",
        "[opts: -long]",
        "[opts: --]",
        "[opts: -s extra]",
        "[opts: -s,]",
        "[opts: --x=$VALUE]",
        '[opts: --x="broken]',
        "[opts: -s",
        "[opts: -s] [opts: -v]",
    ],
)
def test_invalid_declarations_are_reported(block, isolated, caplog):
    config = parse_config("allow " + block + " fictcli *")
    assert not config.rules
    assert "skipped" in caplog.text


def test_context_flags_and_rule_order(isolated, monkeypatch):
    monkeypatch.setenv("HCOM_INSTANCE_NAME", "fictagent")
    config = parse_config(
        "set context-env HCOM_INSTANCE_NAME\n"
        "wrapper fictwrap --cmd run --context -t --script-stdin --script\n"
        "ask [fictwrap] *\n"
        "allow [$HCOM_INSTANCE_NAME=fictagent,fictwrap,box] [opts: -s] fictcli *\n"
        "deny [fictwrap] fictcli forbidden\n"
    )
    result = analyze("fictwrap -t box run 'fictcli -s item'", config, isolated)
    assert result.action == "allow"
    assert {"fictwrap", "box", "$HCOM_INSTANCE_NAME=fictagent"} <= result.context_flags
    assert (
        analyze("fictwrap -t other run 'fictcli -s item'", config, isolated).action
        == "ask"
    )
    assert (
        analyze("fictwrap -t box run 'fictcli forbidden'", config, isolated).action
        == "deny"
    )
    monkeypatch.setenv("HCOM_INSTANCE_NAME", "other")
    assert (
        analyze("fictwrap -t box run 'fictcli -s item'", config, isolated).action
        == "ask"
    )


def test_remote_heredoc_pipeline(isolated):
    config = parse_config(
        "wrapper fictwrap --cmd run --context -t --script-stdin --script\n"
        "ask [fictwrap] *\n"
        'allow [fictwrap] [opts: -s, -H="Host: *"] fictcli http://10.*\n'
        "allow [fictwrap] fictfilter *\n"
    )
    command = (
        "fictwrap -t box run --script <<'REMOTE'\n"
        "fictcli -s -H'Host: demo' http://10.1.2.3/ | fictfilter one\nREMOTE"
    )
    assert analyze(command, config, isolated).action == "allow"
    bypass = command.replace(" | fictfilter", " --upload-file /etc/passwd | fictfilter")
    assert analyze(bypass, config, isolated).action == "ask"


def test_environment_prefix_respects_raw_deny(isolated):
    rule = "allow [opts: -s] fictcli item"
    assert action("MODE=demo fictcli -s item", rule, isolated) == "allow"
    config = parse_config("deny MODE=bad fictcli *\n" + rule)
    assert analyze("MODE=bad fictcli -s item", config, isolated).action == "deny"


def test_local_and_remote_positional_paths(isolated):
    rule = "allow [opts: -s] fictcli ./data/*"
    assert action("fictcli -s ./data/file", rule, isolated) == "allow"
    config = parse_config(rule)
    assert (
        analyze("fictcli -s ./data/file", config, isolated, remote=True).action
        == "allow"
    )
    assert (
        analyze("fictcli -s /data/file", config, isolated, remote=True).action == "ask"
    )


def test_complete_config_load_order(isolated, monkeypatch):
    project = isolated / "project"
    project.mkdir()
    user_dir = Path.home() / ".dippy"
    user_dir.mkdir()
    final = user_dir / "final"
    final.write_text("deny fictcli forbidden\n")
    (user_dir / "config").write_text("ask fictcli *\nset final " + str(final))
    (project / ".dippy").write_text("allow [opts: -s] fictcli *\n")
    config = load_config(project)
    assert analyze("fictcli -s item", config, project).action == "allow"
    assert analyze("fictcli forbidden", config, project).action == "deny"


def test_legacy_patterns_keep_prefix_and_cross_argument_matching(isolated):
    assert action("fictcli one two", "allow fictcli *", isolated) == "allow"
    assert action("fictcli item --bad", "allow fictcli item", isolated) == "allow"


@pytest.mark.parametrize("directive", ["ask", "deny"])
def test_option_rules_keep_messages_and_precedence(directive, isolated):
    config = parse_config(
        "allow fictcli *\n"
        + directive
        + ' [opts: -s] fictcli item "review this invocation"\n'
    )
    result = analyze("fictcli -s item", config, isolated)
    assert result.action == directive
    assert "review this invocation" in result.reason
    assert analyze("fictcli --other item", config, isolated).action == "allow"


def test_delegate_option_block_runs_native_handler(isolated):
    config = parse_config(
        'ask curl *\ndelegate [opts: -s, -H="Host: *"] curl http://10.*\n'
    )
    assert (
        analyze("curl -s -H'Host: demo' http://10.1.2.3/", config, isolated).action
        == "allow"
    )
    assert (
        analyze("curl -T/etc/passwd http://10.1.2.3/", config, isolated).action == "ask"
    )


def test_remote_tilde_is_literal(isolated):
    config = parse_config("allow [opts:] fictcli ~/data/*")
    assert (
        analyze("fictcli '~/data/file'", config, isolated, remote=True).action
        == "allow"
    )
    assert analyze("fictcli /data/file", config, isolated, remote=True).action == "ask"


def test_quoted_command_and_subcommand_patterns(isolated):
    rule = 'allow [opts: -s] fictcli inspect "one *"'
    assert action("'fictcli' -s inspect 'one item'", rule, isolated) == "allow"
    assert action("fictcli inspect one item -s", rule, isolated) == "ask"


def test_option_rules_preserve_redirect_checks(isolated):
    config = parse_config("allow [opts: -s] fictcli item\ndeny-redirect **\n")
    assert analyze("fictcli -s item > output", config, isolated).action == "deny"


@pytest.mark.parametrize(
    "pattern",
    ["", "fictcli *; fictother", "fictcli * | fictother", "fictcli * > output"],
)
def test_option_pattern_rejects_compound_syntax(pattern, isolated, caplog):
    config = parse_config("allow [opts: -s] " + pattern)
    assert not config.rules
    assert "skipped" in caplog.text


def test_explicit_binary_alias_still_matches(isolated):
    config = parse_config("alias ./bin/fictcli fictcli\nallow [opts: -s] fictcli item")
    assert analyze("./bin/fictcli -s item", config, isolated).action == "allow"


@pytest.mark.parametrize(
    "mode", ["cli", "claude", "codex", "gemini", "agy", "agy-integrated"]
)
@pytest.mark.parametrize("upload", [False, True])
def test_original_curl_pipeline_through_cli_and_hooks(mode, upload, isolated):
    project = isolated / "project"
    project.mkdir()
    fake_askpass = isolated / "askpass"
    fake_askpass.write_text("#!/bin/sh\nexit 1\n")
    fake_askpass.chmod(0o700)
    (project / ".dippy").write_text(
        "wrapper cca-tmux-cli --cmd run --context -t --script-stdin --script\n"
        "set context-env HCOM_INSTANCE_NAME\n"
        "ask [cca-tmux-cli] *\n"
        'allow [$HCOM_INSTANCE_NAME=wdt_main,cca-tmux-cli] [opts: -s, -H="Host: *"] curl http://10.*\n'
        "allow [cca-tmux-cli] grep *\n"
        "allow [cca-tmux-cli] sort *\n"
        "allow [cca-tmux-cli] head *\n"
    )
    command = (
        "cca-tmux-cli -t artur2 run --script <<'REMOTE'\n"
        "curl -s -H 'Host: test.kili.cz' http://10.208.0.132/"
        + (" --upload-file /etc/passwd" if upload else "")
        + ' | grep -oE \'href="(https?://www\\.kili\\.cz)?/[^"#?]+/"\' | sort -u | head -n 5\n'
        "REMOTE"
    )
    env = os.environ.copy()
    env["DIPPY_ASKPASS"] = str(fake_askpass)
    env["HCOM_INSTANCE_NAME"] = "wdt_main"
    env.pop("DIPPY_POLICY_CWD", None)
    if mode == "agy-integrated":
        env["DIPPY_POLICY_CWD"] = str(project)
    args = [sys.executable, "-m", "dippy"]
    if mode == "cli":
        args += ["--cmd", command, "--json", "--cwd", str(project)]
        payload = None
    elif mode.startswith("agy"):
        args += ["--agy"]
        payload = {
            "hook_event_name": "PreToolUse",
            "workspacePaths": [str(project)],
            "toolCall": {
                "name": "run_command",
                "args": {"CommandLine": command, "Cwd": str(project)},
            },
        }
    else:
        args += ["--" + mode]
        payload = {
            "hook_event_name": "PermissionRequest" if mode == "codex" else "PreToolUse",
            "cwd": str(project),
            "tool_name": "run_shell_command" if mode == "gemini" else "Bash",
            "tool_input": {"command": command},
        }
    completed = subprocess.run(
        args,
        input=json.dumps(payload) if payload is not None else None,
        cwd=project,
        env=env,
        capture_output=True,
        text=True,
        timeout=15,
    )
    assert completed.stderr == ""
    response = json.loads(completed.stdout) if completed.stdout.strip() else {}
    if mode == "cli":
        assert response["decision"] == ("ask" if upload else "allow")
        assert completed.returncode == (2 if upload else 0)
    elif mode in ("gemini", "agy", "agy-integrated"):
        assert (response.get("decision") == "allow") is (not upload)
    elif mode == "codex":
        assert (
            response.get("hookSpecificOutput", {}).get("decision", {}).get("behavior")
            == "allow"
        ) is (not upload)
    else:
        assert (
            response.get("hookSpecificOutput", {}).get("permissionDecision") == "allow"
        ) is (not upload)
