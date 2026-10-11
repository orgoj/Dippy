"""Tests for logging enhancements - context flags in audit log."""

import io
import json
import os
import tempfile
import threading
from argparse import Namespace
from datetime import datetime, timedelta
from pathlib import Path

import pytest

from dippy import dippy as dippy_main
from dippy import pi_wrapper
from dippy.core.analyzer import Decision
from dippy.core.config import Config, _rotate_logs, configure_logging, log_decision


def test_log_decision_with_context_flags():
    """Test that log_decision includes context_flags in audit log."""
    # Create temp file for logging
    with tempfile.NamedTemporaryFile(mode="w", delete=False, suffix=".log") as f:
        log_path = Path(f.name)

    try:
        # Enable logging for tests
        old_val = os.environ.get("DIPPY_TEST_NO_LOG")
        os.environ.pop("DIPPY_TEST_NO_LOG", None)

        try:
            # Configure logging to temp file
            from dippy.core.config import Config

            test_config = Config(log=log_path, log_full=True)
            configure_logging(test_config)

            # Log decision with context flags
            log_decision(
                "allow",
                "free",
                rule="free *",
                command="wrap server1 free -h",
                context_flags=frozenset(["wrap", "server1"]),
            )

            # Read and verify log file
            log_content = log_path.read_text()
            entries = [
                json.loads(line)
                for line in log_content.strip().split("\n")
                if line.strip()
            ]

            assert len(entries) == 1
            entry = entries[0]

            assert entry["decision"] == "allow"
            assert entry["cmd"] == "free"
            assert entry["rule"] == "free *"
            assert entry["command"] == "wrap server1 free -h"
            assert "context_flags" in entry
            assert sorted(entry["context_flags"]) == ["server1", "wrap"]

        finally:
            # Restore old value
            if old_val is not None:
                os.environ["DIPPY_TEST_NO_LOG"] = old_val

    finally:
        # Clean up temp file
        log_path.unlink(missing_ok=True)


def test_log_decision_without_context_flags():
    """Test that log_decision works without context_flags."""
    with tempfile.NamedTemporaryFile(mode="w", delete=False, suffix=".log") as f:
        log_path = Path(f.name)

    try:
        old_val = os.environ.get("DIPPY_TEST_NO_LOG")
        os.environ.pop("DIPPY_TEST_NO_LOG", None)

        try:
            from dippy.core.config import Config

            test_config = Config(log=log_path)
            configure_logging(test_config)

            # Log decision without context flags
            log_decision(
                "allow",
                "ls",
                rule="ls *",
            )

            # Read and verify log file
            log_content = log_path.read_text()
            entries = [
                json.loads(line)
                for line in log_content.strip().split("\n")
                if line.strip()
            ]

            assert len(entries) == 1
            entry = entries[0]

            assert entry["decision"] == "allow"
            assert entry["cmd"] == "ls"
            assert entry["rule"] == "ls *"
            # context_flags should not be present when None
            assert "context_flags" not in entry

        finally:
            if old_val is not None:
                os.environ["DIPPY_TEST_NO_LOG"] = old_val

    finally:
        log_path.unlink(missing_ok=True)


def test_log_decision_with_empty_context_flags():
    """Test that empty context_flags are not logged."""
    with tempfile.NamedTemporaryFile(mode="w", delete=False, suffix=".log") as f:
        log_path = Path(f.name)

    try:
        old_val = os.environ.get("DIPPY_TEST_NO_LOG")
        os.environ.pop("DIPPY_TEST_NO_LOG", None)

        try:
            from dippy.core.config import Config

            test_config = Config(log=log_path)
            configure_logging(test_config)

            # Log decision with empty context flags
            log_decision(
                "allow",
                "ls",
                rule="ls *",
                context_flags=frozenset(),  # Empty set
            )

            # Read and verify log file
            log_content = log_path.read_text()
            entries = [
                json.loads(line)
                for line in log_content.strip().split("\n")
                if line.strip()
            ]

            assert len(entries) == 1
            entry = entries[0]

            # Empty context_flags should not be in log
            assert "context_flags" not in entry

        finally:
            if old_val is not None:
                os.environ["DIPPY_TEST_NO_LOG"] = old_val

    finally:
        log_path.unlink(missing_ok=True)


def test_context_flags_propagated_in_pipeline():
    """Test that @pipeline and @compound flags are propagated to final Decision."""
    from dippy.core.analyzer import analyze
    from dippy.core.config import Config

    config = Config()
    cwd = Path.cwd()

    # Pipeline should have @pipeline and @compound flags
    result = analyze("ls | head", config, cwd)
    assert result.action == "allow"
    assert result.context_flags is not None
    assert "@pipeline" in result.context_flags
    assert "@compound" in result.context_flags


def test_context_flags_propagated_in_list():
    """Test that @compound flag is propagated in list (&&) constructs."""
    from dippy.core.analyzer import analyze
    from dippy.core.config import Config

    config = Config()
    cwd = Path.cwd()

    # List (&&) should have @compound flag
    result = analyze("echo test && ls", config, cwd)
    assert result.action == "allow"
    assert result.context_flags is not None
    assert "@compound" in result.context_flags


def test_context_flags_propagated_in_subshell():
    """Test that @subshell and @compound flags are propagated from subshell."""
    from dippy.core.analyzer import analyze
    from dippy.core.config import parse_config

    # Need to allow cd in subshell for this test
    config = parse_config("allow [@subshell] cd *")
    cwd = Path.cwd()

    # Subshell should have @subshell and @compound flags
    result = analyze("(cd /tmp && pwd)", config, cwd)
    assert result.action == "allow"
    assert result.context_flags is not None
    assert "@subshell" in result.context_flags
    assert "@compound" in result.context_flags


def test_bypass_mode_audit_entry_names_the_command_like_the_normal_path(
    tmp_path, monkeypatch, capsys
):
    entries = []
    monkeypatch.setattr(dippy_main, "MODE", "claude")
    monkeypatch.setattr(dippy_main, "_detect_mode_from_flags", lambda: "claude")
    monkeypatch.setattr("sys.argv", ["dippy", "--claude"])
    monkeypatch.setattr(dippy_main, "load_config", lambda cwd: Config())
    monkeypatch.setattr(
        dippy_main,
        "log_decision",
        lambda decision, cmd=None, **kwargs: entries.append((decision, cmd)),
    )
    for mode in ["default", "bypassPermissions", "dontAsk"]:
        request = {
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": "frobnicate --hard /x"},
            "cwd": str(tmp_path),
            "permission_mode": mode,
        }
        monkeypatch.setattr("sys.stdin", io.StringIO(json.dumps(request)))
        dippy_main.main()
    capsys.readouterr()

    normal_cmd = entries[0][1]
    assert normal_cmd
    assert entries[1:] == [("allow", normal_cmd), ("allow", normal_cmd)]


def test_cli_mode_audit_entry_names_the_command(tmp_path, monkeypatch, capsys):
    entries = []
    monkeypatch.setattr(dippy_main, "load_config", lambda cwd, **kwargs: Config())
    monkeypatch.setattr(
        dippy_main,
        "log_decision",
        lambda decision, cmd=None, **kwargs: entries.append((decision, cmd)),
    )
    monkeypatch.setattr(
        dippy_main, "analyze", lambda *args, **kwargs: Decision("ask", "frob --x")
    )
    args = Namespace(
        cmd="frob --x /y",
        stdin=False,
        cwd=str(tmp_path),
        config=None,
        config_only=None,
        remote=False,
        json_output=True,
    )

    dippy_main.cli_mode(args)
    capsys.readouterr()

    assert entries == [("ask", "frob --x")]


def test_rotation_names_the_log_after_its_last_write_day(tmp_path):
    log = tmp_path / "audit.log"
    config = Config(log=log)
    log.write_text("today\n")
    _rotate_logs(config)
    assert sorted(p.name for p in tmp_path.iterdir()) == ["audit.log"]

    old = datetime.now() - timedelta(days=3)
    os.utime(log, (old.timestamp(), old.timestamp()))
    _rotate_logs(config)
    rotated = tmp_path / f"audit-{old:%Y-%m-%d}.log"
    assert sorted(p.name for p in tmp_path.glob("audit*")) == [rotated.name]
    assert rotated.read_text() == "today\n"


def test_concurrent_rotation_keeps_every_line(tmp_path):
    log = tmp_path / "audit.log"
    config = Config(log=log)
    old = datetime.now() - timedelta(days=3)
    rotated = tmp_path / f"audit-{old:%Y-%m-%d}.log"
    for _ in range(20):
        for path in tmp_path.iterdir():
            path.unlink()
        log.write_text("old\n")
        os.utime(log, (old.timestamp(), old.timestamp()))
        barrier = threading.Barrier(8)

        def worker():
            barrier.wait()
            _rotate_logs(config)
            with open(log, "a") as f:
                f.write("new\n")

        threads = [threading.Thread(target=worker) for _ in range(8)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        assert rotated.read_text() == "old\n"
        assert log.read_text() == "new\n" * 8


def test_rotation_ignores_filesystem_errors(tmp_path):
    blocker = tmp_path / "file"
    blocker.write_text("")
    _rotate_logs(Config(log=blocker / "audit.log"))


@pytest.mark.parametrize(
    ("extra", "agent"), [([], "cli"), (["--agent", "wrapper"], "wrapper")]
)
def test_cli_mode_writes_the_audit_log(extra, agent, tmp_path, monkeypatch, capsys):
    config = Config()
    configured = []
    entries = []
    monkeypatch.setattr(dippy_main, "load_config", lambda cwd, **kwargs: config)
    monkeypatch.setattr(dippy_main, "configure_logging", configured.append)
    monkeypatch.setattr(
        dippy_main,
        "log_decision",
        lambda decision, agent=None, **kwargs: entries.append((decision, agent)),
    )
    monkeypatch.setattr(dippy_main, "MODE", "claude")
    argv = ["dippy", "--cmd", "frobnicate --x", "--cwd", str(tmp_path), *extra]
    monkeypatch.setattr("sys.argv", argv)

    with pytest.raises(SystemExit):
        dippy_main.main()
    capsys.readouterr()

    assert configured == [config]
    assert [a for _, a in entries] == [agent]


def test_pi_bash_audit_entry_names_the_request(tmp_path, monkeypatch, capsys):
    entries = []
    monkeypatch.setattr(pi_wrapper, "load_config", lambda cwd: Config())
    monkeypatch.setattr(pi_wrapper, "configure_logging", lambda config: None)
    monkeypatch.setattr(
        pi_wrapper,
        "log_decision",
        lambda decision, **kwargs: entries.append({"decision": decision, **kwargs}),
    )
    monkeypatch.setattr(
        pi_wrapper, "analyze", lambda *args, **kwargs: Decision("ask", "frob --x")
    )
    request = {"type": "bash", "command": "frob --x /y", "cwd": str(tmp_path)}
    monkeypatch.setattr("sys.stdin", io.StringIO(json.dumps(request)))

    with pytest.raises(SystemExit):
        pi_wrapper.main()

    assert json.loads(capsys.readouterr().out)["action"] == "ask"
    assert [(e["decision"], e["cmd"]) for e in entries] == [("ask", "frob --x")]
