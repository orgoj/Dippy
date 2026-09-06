"""Project operation guards must survive profile and transport changes."""

import subprocess
import json
import sys

import pytest

from dippy.core.analyzer import Decision
from dippy.core.config import Config
from dippy.execution import (
    execute,
    recover,
    _operation_key,
    _read_state,
    _write_state,
    _transport_payload,
    _poll_capture,
    parse_marker,
    _ensure_tmux,
    _ensure_herdr,
)
from dippy.ssh_transport import SSHTransport


@pytest.fixture(autouse=True)
def isolated(tmp_path, monkeypatch):
    monkeypatch.setenv("HOME", str(tmp_path))
    monkeypatch.setenv("DIPPY_ASKPASS", "/bin/false")
    monkeypatch.setattr(
        "dippy.execution.analyze", lambda *a, **kw: Decision("allow", "test")
    )


@pytest.mark.parametrize("status", ["running", "indeterminate"])
def test_pending_operation_blocks_changed_backend(tmp_path, monkeypatch, status):
    key = _operation_key(tmp_path, "srv")
    _write_state(key, {"status": status, "backend": "ssh", "profile": "old"})
    monkeypatch.setattr("dippy.execution._run_tmux", lambda *a: pytest.fail("executed"))
    assert (
        execute(
            "frob",
            Config(servers=["srv"], run_on_server_backend="tmux"),
            tmp_path,
            "srv",
        )
        == 1
    )


def test_two_projects_do_not_share_operation_state(tmp_path, monkeypatch):
    a, b = tmp_path / "a", tmp_path / "b"
    a.mkdir()
    b.mkdir()
    _write_state(_operation_key(a, "srv"), {"status": "indeterminate"})
    monkeypatch.setattr("dippy.execution._run_ssh", lambda *a: 0)
    assert execute("frob", Config(servers=["srv"]), b, "srv") == 0
    assert _read_state(_operation_key(a, "srv"))["status"] == "indeterminate"


def test_legacy_pending_state_blocks_new_project(tmp_path, monkeypatch):
    _write_state("srv", {"status": "running"})
    monkeypatch.setattr("dippy.execution._run_ssh", lambda *a: pytest.fail("executed"))
    assert execute("frob", Config(servers=["srv"]), tmp_path, "srv") == 1


def test_corrupt_state_does_not_disappear(tmp_path):
    _write_state("srv", {"status": "running"})
    (tmp_path / ".dippy/run-state/srv.json").write_text("{broken")
    with pytest.raises(ValueError, match="state"):
        _read_state("srv")


def test_recovery_checks_session_before_capture(tmp_path, monkeypatch):
    _write_state(
        _operation_key(tmp_path, "srv"),
        {
            "status": "running",
            "backend": "tmux",
            "session": "old",
            "marker": "abc",
            "profile": "user",
        },
    )
    monkeypatch.setattr(
        "dippy.execution._tmux_capture", lambda *a: pytest.fail("captured")
    )
    assert (
        recover(
            "srv", Config(servers=["srv"], run_on_server_backend="tmux"), cwd=tmp_path
        )
        == 1
    )


def test_ssh_signal_is_indeterminate(tmp_path, monkeypatch):
    monkeypatch.setattr(
        "dippy.execution.subprocess.run",
        lambda *a, **kw: subprocess.CompletedProcess(a, -15),
    )
    assert execute("frob", Config(servers=["srv"]), tmp_path, "srv") == 125


def test_terminal_transport_keeps_command_on_ssh_stdin(tmp_path):
    fake = tmp_path / "fake ssh '$%.py"
    result_file = tmp_path / "received"
    fake.write_text(
        "import json, os, sys\n"
        "from pathlib import Path\n"
        "Path(sys.argv[1]).write_text(json.dumps([sys.argv[2:], "
        'os.environ.get("SSH_AUTH_SOCK"), sys.stdin.read()]))\n'
    )
    injected = tmp_path / "must-not-exist"
    command = f"frob\nprintf unsafe > '{injected}'\n"
    socket = "/project/agent ' $ socket"
    transport = SSHTransport(
        [sys.executable, str(fake), str(result_file), "-F", "config '$%\nspace"],
        {"SSH_AUTH_SOCK": socket},
        tmp_path,
        "test",
    )
    result = subprocess.run(
        ["/bin/bash", "-c", _transport_payload(transport, command, "abc")],
        capture_output=True,
        text=True,
        timeout=5,
    )
    assert result.returncode == 0
    argv, actual_socket, stdin = json.loads(result_file.read_text())
    assert argv == ["-F", "config '$%\nspace"]
    assert actual_socket == socket
    assert "DIPPY_END_abc" in stdin
    assert not injected.exists()
    assert "DIPPY_END_abc" not in result.stdout
    assert "DIPPY_TRANSPORT_abc:0" in result.stdout


@pytest.mark.parametrize("status", [0, 255, 143])
def test_transport_exit_without_remote_marker_stays_uncertain(status):
    with pytest.raises(TimeoutError, match="remote completion"):
        _poll_capture(lambda: f"DIPPY_TRANSPORT_abc:{status}\n", "abc", 1, 0.001)


def test_actual_remote_exit_255_can_be_recovered():
    assert parse_marker("DIPPY_END_abc:255\nDIPPY_TRANSPORT_abc:255\n", "abc") == 255


def test_remote_wrapper_handles_output_without_newline_and_exit_255(tmp_path):
    # A local Bash stands in for SSH's remote Bash; no connection is made.
    transport = SSHTransport(["/bin/bash", "-s"], {}, tmp_path, "test")
    result = subprocess.run(
        [
            "/bin/bash",
            "-c",
            _transport_payload(transport, "printf hello; exit 255", "abc"),
        ],
        capture_output=True,
        text=True,
        timeout=5,
    )
    assert "hello\nDIPPY_END_abc:255\n" in result.stdout
    assert parse_marker(result.stdout, "abc") == 255


def test_new_tmux_operation_never_adopts_selected_window(tmp_path, monkeypatch):
    calls = []

    def run(argv, **kwargs):
        calls.append(argv)
        return subprocess.CompletedProcess(argv, 0, "%42\n", "")

    monkeypatch.setattr("dippy.execution.subprocess.run", run)
    _write_state("srv", {"status": "running", "marker": "new", "tmux_pane": "%7"})
    assert _ensure_tmux(Config(), "srv") == "%42"
    assert calls[1][1] == "new-window"
    assert _read_state("srv")["marker"] == "new"
    assert _read_state("srv")["status"] == "running"


def test_new_herdr_operation_never_adopts_old_pane(tmp_path, monkeypatch):
    calls = []

    def run(argv, **kwargs):
        calls.append(argv)
        return subprocess.CompletedProcess(
            argv, 0, '{"result":{"root_pane":{"pane_id":"w2:p8"}}}', ""
        )

    monkeypatch.setattr("dippy.execution.subprocess.run", run)
    _write_state("srv", {"status": "running", "marker": "new", "herdr_pane": "w1:p1"})
    assert _ensure_herdr(Config(), "srv") == "w2:p8"
    assert all("get" not in call for call in calls)
    assert _read_state("srv")["marker"] == "new"
    assert _read_state("srv")["status"] == "running"


@pytest.mark.parametrize(
    "setting",
    [
        {"run_on_server_session": "changed"},
        {"run_on_server_ssh_auth_sock": "/different/socket"},
        {"run_on_server_ssh_auth_sock": "none"},
    ],
)
def test_pending_operation_blocks_profile_changes_and_reset(
    tmp_path, monkeypatch, setting
):
    _write_state(
        _operation_key(tmp_path, "srv"), {"status": "indeterminate", "profile": "old"}
    )
    monkeypatch.setattr(
        "dippy.execution.build_transport",
        lambda *a: pytest.fail("must check state first"),
    )
    assert execute("frob", Config(servers=["srv"], **setting), tmp_path, "srv") == 1


def test_subdirectories_share_project_guard(tmp_path):
    (tmp_path / ".dippy").write_text("server srv\n")
    child = tmp_path / "child"
    child.mkdir()
    assert _operation_key(tmp_path, "srv") == _operation_key(child, "srv")
