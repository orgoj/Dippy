"""Tests for Dippy's direct command execution pipeline."""

import base64
import io
import json
import re
import subprocess
import threading
from argparse import Namespace
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor

import pytest

from dippy.core.analyzer import Decision
from dippy.core.config import Config
from dippy.core.config import parse_config
from dippy.execution import (
    Approval,
    approve_with_askpass,
    execute,
    parse_marker,
    recover,
    validate_server,
    _run_herdr,
    _run_tmux,
    _ensure_herdr,
    _ensure_tmux,
    _read_state,
    _poll_capture,
    _server_lock,
    _write_state,
    _operation_key,
)
from dippy.dippy import handle_subcommand
from dippy.ssh_transport import build_transport


@pytest.fixture(autouse=True)
def isolated_execution_environment(tmp_path, monkeypatch):
    monkeypatch.setenv("HOME", str(tmp_path))
    monkeypatch.setenv("DIPPY_ASKPASS", "/bin/false")
    monkeypatch.setenv("SSH_ASKPASS", "/bin/false")


@pytest.mark.parametrize(
    "server",
    ["", "-oProxyCommand=x", "user@host", "host name", "host;touch-x", "a/b"],
)
def test_server_validation_rejects_ssh_injection(server):
    with pytest.raises(ValueError):
        validate_server(server)


def test_askpass_gets_versioned_json_and_environment(tmp_path, monkeypatch):
    provider = tmp_path / "askpass"
    provider.write_text("")
    config = Config(askpass=provider)
    seen = {}

    def fake_run(argv, **kwargs):
        seen.update(argv=argv, kwargs=kwargs)
        return subprocess.CompletedProcess(
            argv, 0, '{"decision":"allow","note":"ok"}', ""
        )

    monkeypatch.setattr("dippy.execution.subprocess.run", fake_run)
    result = approve_with_askpass(
        config,
        command="frob --write\nnext",
        decision=Decision("ask", "unknown"),
        server="host-1",
    )

    assert result == Approval(True, "ok")
    payload = json.loads(seen["kwargs"]["input"])
    assert payload["version"] == 1
    assert payload["command"] == "frob --write\nnext"
    assert payload["server"] == "host-1"
    assert seen["kwargs"]["env"]["DIPPY_COMMAND"] == "frob --write\nnext"


def test_askpass_environment_overrides_config(tmp_path, monkeypatch):
    configured_provider = tmp_path / "configured-provider"
    environment_provider = tmp_path / "environment-provider"
    seen = {}
    monkeypatch.setenv("DIPPY_ASKPASS", str(environment_provider))

    def fake_run(argv, **kwargs):
        seen["argv"] = argv
        return subprocess.CompletedProcess(argv, 1, '{"decision":"deny"}', "")

    monkeypatch.setattr("dippy.execution.subprocess.run", fake_run)

    approve_with_askpass(
        Config(askpass=configured_provider),
        "frob",
        Decision("ask", "unknown"),
    )

    assert seen["argv"] == [str(environment_provider), "Approve command?"]


@pytest.mark.parametrize("returncode", [1, 2, 127])
def test_askpass_nonzero_is_deny(tmp_path, monkeypatch, returncode):
    provider = tmp_path / "askpass"
    provider.write_text("")
    monkeypatch.setattr(
        "dippy.execution.subprocess.run",
        lambda *args, **kwargs: subprocess.CompletedProcess(args, returncode, "", ""),
    )
    assert not approve_with_askpass(
        Config(askpass=provider), "frob", Decision("ask", "unknown")
    ).allowed


def test_missing_or_broken_askpass_is_deny(tmp_path, monkeypatch):
    monkeypatch.delenv("DIPPY_ASKPASS", raising=False)
    decision = Decision("ask", "unknown")
    missing_config = approve_with_askpass(Config(), "frob", decision)
    assert not missing_config.allowed
    assert missing_config.note.startswith("ERROR: Approval unavailable")
    assert "Dippy" not in missing_config.note
    assert "askpass" not in missing_config.note
    assert "The requested command was not executed" in missing_config.note

    def missing(*args, **kwargs):
        raise FileNotFoundError

    monkeypatch.setattr("dippy.execution.subprocess.run", missing)
    broken_provider = approve_with_askpass(
        Config(askpass=tmp_path / "missing"), "frob", decision
    )
    assert not broken_provider.allowed
    assert broken_provider.note.startswith("ERROR: Approval failed")
    assert "Dippy" not in broken_provider.note
    assert "provider" not in broken_provider.note
    assert "The requested command was not executed" in broken_provider.note


def test_askpass_timeout_is_explicit_and_hides_provider(tmp_path, monkeypatch):
    provider = tmp_path / "secret-provider"

    def timeout(*args, **kwargs):
        raise subprocess.TimeoutExpired(args[0], 1)

    monkeypatch.setattr("dippy.execution.subprocess.run", timeout)
    result = approve_with_askpass(
        Config(askpass=provider, askpass_timeout=59),
        "frob --change /x",
        Decision("ask", "unknown"),
    )

    assert not result.allowed
    assert "ERROR: Approval timed out after 59 seconds" in result.note
    assert "frob --change /x" in result.note
    assert "was not executed" in result.note
    assert "Retry the same request" in result.note
    assert "Dippy" not in result.note
    assert "bypass" not in result.note
    assert str(provider) not in result.note
    assert "Approve command?" not in result.note


def test_askpass_reports_waiting_before_provider_returns(tmp_path, monkeypatch, capsys):
    provider = tmp_path / "askpass"
    monkeypatch.setattr(
        "dippy.execution.subprocess.run",
        lambda *args, **kwargs: subprocess.CompletedProcess(
            args, 0, '{"decision":"allow"}', ""
        ),
    )

    approve_with_askpass(
        Config(askpass=provider, askpass_timeout=59),
        "frob",
        Decision("ask", "unknown"),
    )

    assert capsys.readouterr().err == (
        "STATUS: Approval required; waiting up to 59 seconds. "
        "The requested command has not started. Stop work and wait for the user "
        "unless you can continue safely without this command.\n"
    )


def test_askpass_uses_configured_wait_message(tmp_path, monkeypatch, capsys):
    provider = tmp_path / "askpass"
    monkeypatch.setattr(
        "dippy.execution.subprocess.run",
        lambda *args, **kwargs: subprocess.CompletedProcess(
            args, 1, '{"decision":"deny"}', ""
        ),
    )
    config = Config(
        askpass=provider,
        approval_wait_message="Contact the supervising agent and wait.",
    )

    approve_with_askpass(config, "frob", Decision("ask", "unknown"))

    assert capsys.readouterr().err.startswith(
        "STATUS: Approval required; waiting up to 59 seconds. "
        "The requested command has not started. "
        "Contact the supervising agent and wait.\n"
    )


@pytest.mark.parametrize(
    ("response", "reason"),
    [
        ('{"decision":"deny","note":"Needs manual review"}', "Needs manual review"),
        ('{"decision":"deny"}', "unknown"),
    ],
)
def test_askpass_deny_explains_that_command_was_not_executed(
    tmp_path, monkeypatch, response, reason
):
    provider = tmp_path / "askpass"
    monkeypatch.setattr(
        "dippy.execution.subprocess.run",
        lambda *args, **kwargs: subprocess.CompletedProcess(args, 1, response, ""),
    )

    result = approve_with_askpass(
        Config(askpass=provider),
        "frob --change /x",
        Decision("ask", "unknown"),
    )

    assert not result.allowed
    assert result.note.startswith("ERROR: Approval denied")
    assert "The requested command was not executed" in result.note
    assert "frob --change /x" in result.note
    assert f"Reason: {reason}" in result.note
    assert "Dippy" not in result.note
    assert "bypass" not in result.note


def test_execute_allow_runs_original_string_once(tmp_path, monkeypatch):
    config = Config()
    command = "printf '%s\\n' 'a b' | frob > out"
    seen = []
    monkeypatch.setattr(
        "dippy.execution.analyze",
        lambda *args, **kwargs: Decision("allow", "rule"),
    )

    def fake_run(argv, **kwargs):
        seen.append((argv, kwargs))
        return subprocess.CompletedProcess(argv, 17)

    monkeypatch.setattr("dippy.execution.subprocess.run", fake_run)
    assert execute(command, config, tmp_path) == 17
    assert seen == [(["/bin/bash", "-c", command], {"cwd": tmp_path})]


@pytest.mark.parametrize(
    ("subcommand", "server"), [("run", None), ("run-on-server", "srv")]
)
def test_execution_subcommand_reads_missing_command_from_stdin(
    subcommand, server, tmp_path, monkeypatch
):
    command = "fictionalread /one\nfictionalread /two\n"
    args = Namespace(
        subcommand=subcommand,
        server=server,
        command=None,
        cwd=str(tmp_path),
        config=None,
    )
    seen = {}
    monkeypatch.setattr("sys.stdin", io.StringIO(command))
    monkeypatch.setattr(
        "dippy.dippy.configure_and_execute",
        lambda value, cwd, config, target=None: seen.update(
            command=value, cwd=cwd, server=target
        )
        or 17,
    )

    assert handle_subcommand(args) == 17
    assert seen == {"command": command, "cwd": tmp_path, "server": server}


def test_execution_subcommand_rejects_empty_stdin(tmp_path, monkeypatch, capsys):
    args = Namespace(subcommand="run", command=None, cwd=str(tmp_path), config=None)
    monkeypatch.setattr("sys.stdin", io.StringIO("\n"))
    monkeypatch.setattr(
        "dippy.dippy.configure_and_execute",
        lambda *args, **kwargs: pytest.fail("empty command was executed"),
    )

    assert handle_subcommand(args) == 1
    assert "command is required" in capsys.readouterr().err


def test_execute_rule_deny_explains_that_command_was_not_executed(
    tmp_path, monkeypatch, capsys
):
    command = "frob --erase /x"
    monkeypatch.setattr(
        "dippy.execution.analyze",
        lambda *args, **kwargs: Decision("deny", "blocked by policy"),
    )
    monkeypatch.setattr(
        "dippy.execution.subprocess.run",
        lambda *args, **kwargs: pytest.fail("command was executed"),
    )

    assert execute(command, Config(), tmp_path) == 1

    error = capsys.readouterr().err
    assert error.startswith("ERROR: Execution denied")
    assert "The requested command was not executed" in error
    assert command in error
    assert "Reason: blocked by policy" in error
    assert "Dippy" not in error
    assert "bypass" not in error


def test_remote_deny_never_calls_backend(tmp_path, monkeypatch):
    monkeypatch.setattr(
        "dippy.execution.analyze",
        lambda *args, **kwargs: Decision("deny", "blocked"),
    )
    monkeypatch.setattr(
        "dippy.execution.subprocess.run",
        lambda *args, **kwargs: pytest.fail("backend was called"),
    )
    assert execute("frob --erase", Config(servers=["srv"]), tmp_path, "srv") == 1


def test_real_remote_deny_rule_blocks_destructive_bypass(tmp_path, monkeypatch):
    config = parse_config("server srv\ndeny [run-on-server] frob --erase|")
    monkeypatch.setattr(
        "dippy.execution.subprocess.run",
        lambda *args, **kwargs: pytest.fail("backend was called"),
    )
    assert execute("frob --erase", config, tmp_path, "srv") == 1


def test_ssh_backend_uses_stdin_and_preserves_exit(tmp_path, monkeypatch):
    config = Config(servers=["srv"], run_on_server_backend="ssh")
    monkeypatch.setattr(
        "dippy.execution.analyze",
        lambda *args, **kwargs: Decision("allow", "rule"),
    )
    monkeypatch.setattr("dippy.execution._state_dir", lambda: tmp_path / "state")
    seen = {}

    def fake_run(argv, **kwargs):
        seen.update(argv=argv, kwargs=kwargs)
        return subprocess.CompletedProcess(argv, 23)

    monkeypatch.setattr("dippy.execution.subprocess.run", fake_run)
    assert execute("frob | next", config, tmp_path, "srv") == 23
    assert seen["argv"] == ["ssh", "--", "srv", "bash", "-s"]
    assert seen["kwargs"]["input"] == b"frob | next"


def test_remote_analysis_has_server_context(tmp_path, monkeypatch):
    config = Config(servers=["srv"])
    seen = {}

    def fake_analyze(command, config, cwd, context_flags, remote):
        seen.update(flags=context_flags, remote=remote)
        return Decision("deny", "stop")

    monkeypatch.setattr("dippy.execution.analyze", fake_analyze)
    assert execute("frob", config, tmp_path, "srv") == 1
    assert seen == {"flags": frozenset({"run-on-server", "srv"}), "remote": True}


def test_marker_parser_requires_matching_marker():
    assert parse_marker("noise\nDIPPY_END_abcd:7\n", "abcd") == 7
    with pytest.raises(ValueError, match="missing completion marker"):
        parse_marker("DIPPY_END_other:0", "abcd")


def test_poll_capture_returns_available_output_when_start_scrolled_out(capsys):
    output = "oldest available line\nnewest line\nDIPPY_END_abcdef:7\n"

    assert _poll_capture(lambda: output, "abcdef", 0.1, 0.001) == 7

    assert capsys.readouterr().out == "oldest available line\nnewest line\n"


def test_timeout_blocks_target_until_explicit_recovery(tmp_path, monkeypatch):
    config = Config(servers=["srv"], run_on_server_backend="ssh")
    monkeypatch.setattr("dippy.execution._state_dir", lambda: tmp_path / "state")
    monkeypatch.setattr(
        "dippy.execution.analyze",
        lambda *args, **kwargs: Decision("allow", "rule"),
    )
    calls = []

    def timeout(*args):
        calls.append(args)
        raise TimeoutError

    monkeypatch.setattr("dippy.execution._run_ssh", timeout)
    assert execute("frob", config, tmp_path, "srv") == 125
    assert execute("frob", config, tmp_path, "srv") == 1
    assert len(calls) == 1
    assert recover("srv", config, clear=True, cwd=tmp_path) == 0


def test_tmux_backend_uses_readiness_and_base64(monkeypatch):
    sent = []
    monkeypatch.setattr("dippy.execution._ensure_tmux", lambda config, server: "target")
    monkeypatch.setattr(
        "dippy.execution._tmux_send", lambda target, text: sent.append(text)
    )

    def capture(target):
        markers = []
        for command in sent:
            markers.extend(re.findall(r"DIPPY_(?:END|TRANSPORT)_([a-f0-9]+)", command))
        return "\n".join(f"DIPPY_END_{marker}:0" for marker in markers) + "\n"

    monkeypatch.setattr("dippy.execution._tmux_capture", capture)
    config = Config(run_on_server_timeout=1, run_on_server_poll_interval=0.001)
    assert (
        _run_tmux(
            config,
            "srv",
            "frob | next > out",
            "abcdef",
            build_transport(config, "srv", Path.cwd()),
        )
        == 0
    )
    assert len(sent) == 2
    encoded = re.search(r"printf %s (\S+) \| base64", sent[1]).group(1)
    payload = base64.b64decode(encoded).decode()
    assert base64.b64encode(b"frob | next > out").decode() in payload
    assert "'ssh'" in sent[1].replace("'\"'\"'", "'")


def test_herdr_backend_uses_readiness_and_persistent_pane(monkeypatch):
    sent = []
    monkeypatch.setattr("dippy.execution._ensure_herdr", lambda config, server: "w1:p1")
    monkeypatch.setattr(
        "dippy.execution._herdr_send", lambda config, pane, text: sent.append(text)
    )

    def capture(config, pane):
        markers = []
        for command in sent:
            markers.extend(re.findall(r"DIPPY_(?:END|TRANSPORT)_([a-f0-9]+)", command))
        return "\n".join(f"DIPPY_END_{marker}:0" for marker in markers) + "\n"

    monkeypatch.setattr("dippy.execution._herdr_capture", capture)
    config = Config(run_on_server_timeout=1, run_on_server_poll_interval=0.001)
    assert (
        _run_herdr(
            config,
            "srv",
            "frob\nnext",
            "abcdef",
            build_transport(config, "srv", Path.cwd()),
        )
        == 0
    )
    assert len(sent) == 2


def test_tmux_new_session_argv_is_fixed(tmp_path, monkeypatch):
    monkeypatch.setattr("dippy.execution._state_dir", lambda: tmp_path / "state")
    calls = []

    def fake_run(argv, **kwargs):
        calls.append(argv)
        code = 1 if argv[1] == "has-session" else 0
        return subprocess.CompletedProcess(argv, code, "%12\n", "")

    monkeypatch.setattr("dippy.execution.subprocess.run", fake_run)
    config = Config(run_on_server_session="managed")
    assert _ensure_tmux(config, "srv") == "%12"
    assert calls == [
        ["tmux", "has-session", "-t", "=managed"],
        [
            "tmux",
            "new-session",
            "-d",
            "-s",
            "managed",
            "-n",
            "srv",
            "-P",
            "-F",
            "#{pane_id}",
            "/bin/bash --noprofile --norc",
        ],
    ]


def test_herdr_create_uses_json_pane_id_and_fixed_argv(tmp_path, monkeypatch):
    calls = []
    monkeypatch.setattr("dippy.execution._state_dir", lambda: tmp_path / "state")

    def fake_run(argv, **kwargs):
        calls.append(argv)
        if "workspace" in argv:
            stdout = json.dumps({"result": {"root_pane": {"pane_id": "w1:p9"}}})
        else:
            stdout = json.dumps({"result": {}})
        return subprocess.CompletedProcess(argv, 0, stdout, "")

    monkeypatch.setattr("dippy.execution.subprocess.run", fake_run)
    config = Config(run_on_server_session="managed")
    assert _ensure_herdr(config, "srv") == "w1:p9"
    assert calls == [
        [
            "herdr",
            "--session",
            "managed",
            "workspace",
            "create",
            "--label",
            "dippy-srv",
            "--no-focus",
        ],
        [
            "herdr",
            "--session",
            "managed",
            "pane",
            "run",
            "w1:p9",
            "exec /bin/bash --noprofile --norc",
        ],
    ]


def test_recovery_parses_saved_marker(tmp_path, monkeypatch):
    monkeypatch.setattr("dippy.execution._state_dir", lambda: tmp_path / "state")
    _write_state(
        _operation_key(tmp_path, "srv"),
        {
            "status": "indeterminate",
            "marker": "abcdef",
            "backend": "tmux",
            "session": "dippy",
            "profile": "user",
            "tmux_pane": "%1",
        },
    )
    monkeypatch.setattr(
        "dippy.execution._tmux_capture", lambda target: "DIPPY_END_abcdef:9\n"
    )
    config = Config(servers=["srv"], run_on_server_backend="tmux")
    assert recover("srv", config, cwd=tmp_path) == 9
    assert _read_state(_operation_key(tmp_path, "srv"))["status"] == "complete"


def test_server_lock_serializes_callers(tmp_path, monkeypatch):
    monkeypatch.setattr("dippy.execution._state_dir", lambda: tmp_path / "state")
    first_entered = threading.Event()
    release_first = threading.Event()
    second_entered = threading.Event()

    def first():
        with _server_lock("srv"):
            first_entered.set()
            release_first.wait(1)

    def second():
        first_entered.wait(1)
        with _server_lock("srv"):
            second_entered.set()

    with ThreadPoolExecutor(max_workers=2) as pool:
        one = pool.submit(first)
        two = pool.submit(second)
        assert first_entered.wait(1)
        assert not second_entered.wait(0.05)
        release_first.set()
        one.result(timeout=1)
        two.result(timeout=1)
    assert second_entered.is_set()
