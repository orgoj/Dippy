"""Approved local and remote command execution."""

from __future__ import annotations

import base64
import fcntl
import json
import os
import re
import subprocess
import sys
import time
import uuid
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path
from typing import Iterator

from dippy.core.analyzer import Decision, analyze
from dippy.core.config import Config, configure_logging, log_decision

_SERVER_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.-]*$")
_SESSION_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.-]*$")


@dataclass(frozen=True)
class Approval:
    allowed: bool
    note: str | None = None


def _blocked_message(
    command: str, heading: str, reason: str | None, guidance: str
) -> str:
    message = f"ERROR: {heading}.\nThe requested command was not executed:\n{command}"
    if reason:
        message += f"\n\nReason: {reason}"
    return f"{message}\n\n{guidance}"


def _approval_timeout_message(command: str, timeout: int) -> str:
    return _blocked_message(
        command,
        f"Approval timed out after {timeout} seconds",
        None,
        "Retry the same request when approval can be reviewed, or use "
        "another safe solution.",
    )


def _approval_denied_message(command: str, reason: str | None) -> str:
    return _blocked_message(
        command,
        "Approval denied",
        reason or "unknown",
        "Revise the request or use another safe solution.",
    )


def validate_server(server: str) -> str:
    """Return a safe SSH-config alias or raise ``ValueError``."""
    if not _SERVER_RE.fullmatch(server) or "@" in server or server.startswith("-"):
        raise ValueError(f"invalid server alias: {server!r}")
    return server


def _approval_payload(
    command: str, decision: Decision, cwd: Path, server: str | None
) -> dict[str, object]:
    return {
        "version": 1,
        "command": command,
        "cwd": str(cwd),
        "server": server,
        "decision": decision.action,
        "reason": decision.reason,
        "context_flags": sorted(decision.context_flags or ()),
    }


def approve_with_askpass(
    config: Config,
    command: str,
    decision: Decision,
    server: str | None = None,
    cwd: Path | None = None,
) -> Approval:
    """Ask an external provider; every provider failure denies execution."""
    askpass_override = os.environ.get("DIPPY_ASKPASS")
    askpass = (
        Path(askpass_override).expanduser() if askpass_override else config.askpass
    )
    if askpass is None:
        return Approval(
            False,
            _blocked_message(
                command,
                "Approval unavailable",
                "Approval is not configured.",
                "Retry later or use another safe solution.",
            ),
        )
    actual_cwd = cwd or Path.cwd()
    payload = _approval_payload(command, decision, actual_cwd, server)
    prompt = f"Approve command on {server}?" if server else "Approve command?"
    env = os.environ.copy()
    env.update(
        {
            "DIPPY_COMMAND": command,
            "DIPPY_CWD": str(actual_cwd),
            "DIPPY_RULE": decision.reason,
            "DIPPY_MESSAGE": decision.reason,
            "DIPPY_TOOL": "run-on-server" if server else "run",
            "DIPPY_SERVER": server or "",
            "DIPPY_ASKPASS_TIMEOUT": str(config.askpass_timeout),
        }
    )
    status_is_tty = sys.stderr.isatty()
    print(
        f"STATUS: Approval required; waiting up to {config.askpass_timeout} seconds. "
        f"The requested command has not started. {config.approval_wait_message}",
        end="" if status_is_tty else "\n",
        file=sys.stderr,
        flush=True,
    )
    try:
        result = subprocess.run(
            [str(askpass), prompt],
            input=json.dumps(payload),
            text=True,
            capture_output=True,
            timeout=config.askpass_timeout,
            env=env,
        )
    except subprocess.TimeoutExpired:
        return Approval(
            False, _approval_timeout_message(command, config.askpass_timeout)
        )
    except OSError:
        return Approval(
            False,
            _blocked_message(
                command,
                "Approval failed",
                "The approval service is unavailable.",
                "Retry later or use another safe solution.",
            ),
        )
    finally:
        if status_is_tty:
            print("\r\033[2K", end="", file=sys.stderr, flush=True)
    note = None
    if result.stdout.strip():
        try:
            response = json.loads(result.stdout)
        except json.JSONDecodeError:
            response = None
        if isinstance(response, dict):
            note_value = response.get("note")
            if isinstance(note_value, str) and note_value:
                note = note_value
            structured = response.get("decision")
            if structured not in (None, "allow", "deny"):
                return Approval(
                    False,
                    _blocked_message(
                        command,
                        "Approval failed",
                        note or "The approval service returned an invalid response.",
                        "Retry later or use another safe solution.",
                    ),
                )
            if structured == "deny":
                return Approval(False, _approval_denied_message(command, note))
            if structured == "allow" and result.returncode == 0:
                return Approval(True, note)
    if result.returncode == 0:
        return Approval(True, note)
    return Approval(False, _approval_denied_message(command, note))


def parse_marker(output: str, marker: str) -> int:
    match = re.search(
        rf"(?:^|\n)DIPPY_END_{re.escape(marker)}:(\d+)(?:\r?$)", output, re.M
    )
    if match is None:
        raise ValueError(f"missing completion marker DIPPY_END_{marker}")
    return int(match.group(1))


def _state_dir() -> Path:
    return Path.home() / ".dippy" / "run-state"


def _state_path(server: str) -> Path:
    return _state_dir() / f"{server}.json"


def _write_state(server: str, data: dict[str, object]) -> None:
    directory = _state_dir()
    directory.mkdir(parents=True, exist_ok=True)
    path = _state_path(server)
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(data, sort_keys=True))
    os.replace(temporary, path)


def _read_state(server: str) -> dict[str, object]:
    try:
        value = json.loads(_state_path(server).read_text())
    except (OSError, json.JSONDecodeError):
        return {}
    return value if isinstance(value, dict) else {}


@contextmanager
def _server_lock(server: str) -> Iterator[None]:
    directory = _state_dir()
    directory.mkdir(parents=True, exist_ok=True)
    with (directory / f"{server}.lock").open("w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        yield


def _shell_payload(command: str, marker: str) -> str:
    encoded = base64.b64encode(command.encode()).decode()
    return (
        f"printf 'DIPPY_START_{marker}\\n'; "
        f"printf %s {encoded} | base64 -d | bash; "
        f"dippy_status=$?; printf 'DIPPY_END_{marker}:%s\\n' \"$dippy_status\""
    )


def _run_checked(argv: list[str], **kwargs) -> subprocess.CompletedProcess:
    result = subprocess.run(argv, **kwargs)
    if result.returncode != 0:
        detail = getattr(result, "stderr", "") or "command failed"
        raise RuntimeError(f"{argv[0]} failed: {str(detail).strip()}")
    return result


def _poll_capture(capture, marker: str, timeout: float, interval: float) -> int:
    deadline = time.monotonic() + timeout
    last = ""
    while time.monotonic() < deadline:
        last = capture()
        try:
            code = parse_marker(last, marker)
        except ValueError:
            time.sleep(interval)
            continue
        start = re.search(rf"(?:^|\n)DIPPY_START_{re.escape(marker)}\r?\n", last)
        end = re.search(rf"(?:^|\n)DIPPY_END_{re.escape(marker)}:\d+\r?$", last, re.M)
        if end is not None:
            output_start = (
                start.end() if start is not None and end.start() >= start.end() else 0
            )
            output = last[output_start : end.start()]
            if output:
                print(output, end="" if output.endswith("\n") else "\n")
        return code
    raise TimeoutError(f"missing completion marker DIPPY_END_{marker}")


def _run_ssh(server: str, command: str, timeout: float) -> int:
    try:
        result = subprocess.run(
            ["ssh", "--", server, "bash", "-s"],
            input=command.encode(),
            timeout=timeout,
        )
    except subprocess.TimeoutExpired as error:
        raise TimeoutError(
            "SSH command timed out; remote status is indeterminate"
        ) from error
    if result.returncode == 255:
        raise TimeoutError("SSH connection ended without a reliable command result")
    return result.returncode


def _tmux_target(config: Config, server: str) -> str:
    if not _SESSION_RE.fullmatch(config.run_on_server_session):
        raise ValueError("invalid run-on-server session name")
    return f"={config.run_on_server_session}:={server}"


def _ensure_tmux(config: Config, server: str) -> str:
    session = config.run_on_server_session
    target = _tmux_target(config, server)
    exists = subprocess.run(
        ["tmux", "has-session", "-t", f"={session}"], capture_output=True
    )
    if exists.returncode != 0:
        _run_checked(
            [
                "tmux",
                "new-session",
                "-d",
                "-s",
                session,
                "-n",
                server,
                f"ssh -- {server}",
            ],
            capture_output=True,
            text=True,
        )
    else:
        windows = _run_checked(
            ["tmux", "list-windows", "-t", f"={session}", "-F", "#{window_name}"],
            capture_output=True,
            text=True,
        ).stdout.splitlines()
        if server not in windows:
            _run_checked(
                [
                    "tmux",
                    "new-window",
                    "-d",
                    "-t",
                    f"={session}",
                    "-n",
                    server,
                    f"ssh -- {server}",
                ],
                capture_output=True,
                text=True,
            )
    return target


def _tmux_send(target: str, text: str) -> None:
    _run_checked(["tmux", "send-keys", "-t", target, "-l", text], capture_output=True)
    _run_checked(["tmux", "send-keys", "-t", target, "Enter"], capture_output=True)


def _tmux_capture(target: str) -> str:
    return _run_checked(
        ["tmux", "capture-pane", "-p", "-S", "-", "-t", target],
        capture_output=True,
        text=True,
    ).stdout


def _run_tmux(config: Config, server: str, command: str, marker: str) -> int:
    target = _ensure_tmux(config, server)
    ready = uuid.uuid4().hex
    _tmux_send(target, f"printf 'DIPPY_END_{ready}:0\\n'")
    _poll_capture(
        lambda: _tmux_capture(target),
        ready,
        config.run_on_server_timeout,
        config.run_on_server_poll_interval,
    )
    _tmux_send(target, _shell_payload(command, marker))
    return _poll_capture(
        lambda: _tmux_capture(target),
        marker,
        config.run_on_server_timeout,
        config.run_on_server_poll_interval,
    )


def _herdr_command(config: Config, *args: str) -> list[str]:
    if not _SESSION_RE.fullmatch(config.run_on_server_session):
        raise ValueError("invalid run-on-server session name")
    return ["herdr", "--session", config.run_on_server_session, *args]


def _json_result(result: subprocess.CompletedProcess) -> dict[str, object]:
    try:
        data = json.loads(result.stdout)
    except (TypeError, json.JSONDecodeError) as error:
        raise RuntimeError("Herdr returned invalid JSON") from error
    if not isinstance(data, dict):
        raise RuntimeError("Herdr returned invalid JSON")
    return data


def _ensure_herdr(config: Config, server: str) -> str:
    state = _read_state(server)
    pane = state.get("herdr_pane")
    if isinstance(pane, str):
        probe = subprocess.run(
            _herdr_command(config, "pane", "get", pane), capture_output=True, text=True
        )
        if probe.returncode == 0:
            return pane
    created = _run_checked(
        _herdr_command(
            config, "workspace", "create", "--label", f"dippy-{server}", "--no-focus"
        ),
        capture_output=True,
        text=True,
    )
    data = _json_result(created)
    result = data.get("result")
    root = result.get("root_pane") if isinstance(result, dict) else None
    pane = root.get("pane_id") if isinstance(root, dict) else None
    if not isinstance(pane, str):
        raise RuntimeError("Herdr create response omitted root pane")
    _run_checked(
        _herdr_command(config, "pane", "run", pane, f"ssh -- {server}"),
        capture_output=True,
        text=True,
    )
    _write_state(server, {"herdr_pane": pane, "status": "ready"})
    return pane


def _herdr_send(config: Config, pane: str, text: str) -> None:
    _run_checked(
        _herdr_command(config, "pane", "run", pane, text),
        capture_output=True,
        text=True,
    )


def _herdr_capture(config: Config, pane: str) -> str:
    result = _run_checked(
        _herdr_command(
            config,
            "pane",
            "read",
            pane,
            "--source",
            "recent-unwrapped",
            "--lines",
            "10000",
        ),
        capture_output=True,
        text=True,
    )
    return result.stdout


def _run_herdr(config: Config, server: str, command: str, marker: str) -> int:
    pane = _ensure_herdr(config, server)
    ready = uuid.uuid4().hex
    _herdr_send(config, pane, f"printf 'DIPPY_END_{ready}:0\\n'")
    _poll_capture(
        lambda: _herdr_capture(config, pane),
        ready,
        config.run_on_server_timeout,
        config.run_on_server_poll_interval,
    )
    _herdr_send(config, pane, _shell_payload(command, marker))
    return _poll_capture(
        lambda: _herdr_capture(config, pane),
        marker,
        config.run_on_server_timeout,
        config.run_on_server_poll_interval,
    )


def execute(command: str, config: Config, cwd: Path, server: str | None = None) -> int:
    """Classify exactly once, approve if needed, and execute the original string."""
    if server is not None:
        try:
            server = validate_server(server)
        except ValueError as error:
            print(str(error), file=sys.stderr)
            return 1
        if server not in config.servers:
            print(f"server is not allowed: {server}", file=sys.stderr)
            return 1
        context = frozenset({"run-on-server", server})
        decision = analyze(command, config, cwd, context, remote=True)
    else:
        decision = analyze(command, config, cwd)
    action = "ask" if decision.action == "pass" else decision.action
    log_decision(
        action,
        message=decision.reason,
        command=command,
        cwd=cwd,
        context_flags=decision.context_flags,
        agent="run-on-server" if server else "run",
        suggestion=decision.suggestion,
    )
    if action == "deny":
        print(
            _blocked_message(
                command,
                "Execution denied",
                decision.reason,
                "Revise the request or use another safe solution.",
            ),
            file=sys.stderr,
        )
        return 1
    if action == "ask":
        approval = approve_with_askpass(config, command, decision, server, cwd)
        log_decision(
            "allow" if approval.allowed else "deny",
            message=approval.note or decision.reason,
            command=command,
            cwd=cwd,
            agent="run-on-server" if server else "run",
        )
        if not approval.allowed:
            print(approval.note or "approval denied", file=sys.stderr)
            return 1
    if server is None:
        return subprocess.run(["/bin/bash", "-c", command], cwd=cwd).returncode
    with _server_lock(server):
        state = _read_state(server)
        if state.get("status") == "indeterminate":
            print(
                f"previous command on {server} is indeterminate; recover it before retrying",
                file=sys.stderr,
            )
            return 1
        marker = uuid.uuid4().hex
        _write_state(
            server,
            {
                **state,
                "status": "running",
                "marker": marker,
                "backend": config.run_on_server_backend,
                "session": config.run_on_server_session,
            },
        )
        try:
            if config.run_on_server_backend == "ssh":
                code = _run_ssh(server, command, config.run_on_server_timeout)
            elif config.run_on_server_backend == "tmux":
                code = _run_tmux(config, server, command, marker)
            elif config.run_on_server_backend == "herdr":
                code = _run_herdr(config, server, command, marker)
            else:
                raise ValueError(f"unsupported backend: {config.run_on_server_backend}")
        except (KeyboardInterrupt, TimeoutError):
            _write_state(
                server,
                {**_read_state(server), "status": "indeterminate", "marker": marker},
            )
            print(
                f"command status on {server} is INDETERMINATE; it was not retried",
                file=sys.stderr,
            )
            return 125
        except (OSError, RuntimeError, ValueError) as error:
            _write_state(
                server,
                {
                    **_read_state(server),
                    "status": "indeterminate",
                    "marker": marker,
                    "error": str(error),
                },
            )
            print(
                f"command status on {server} is INDETERMINATE: {error}",
                file=sys.stderr,
            )
            return 125
        _write_state(
            server, {**_read_state(server), "status": "complete", "exit_code": code}
        )
        return code


def recover(server: str, config: Config, *, clear: bool = False) -> int:
    """Resolve or explicitly clear a target's indeterminate execution state."""
    server = validate_server(server)
    if server not in config.servers:
        print(f"server is not allowed: {server}", file=sys.stderr)
        return 1
    with _server_lock(server):
        state = _read_state(server)
        if state.get("status") != "indeterminate":
            return 0
        if clear:
            _write_state(server, {**state, "status": "cleared"})
            return 0
        marker = state.get("marker")
        if not isinstance(marker, str):
            print("indeterminate state has no recovery marker", file=sys.stderr)
            return 1
        if state.get("backend") != config.run_on_server_backend:
            print(
                "configured backend changed since the indeterminate command; restore it or use --clear",
                file=sys.stderr,
            )
            return 1
        try:
            if config.run_on_server_backend == "tmux":
                code = parse_marker(_tmux_capture(_tmux_target(config, server)), marker)
            elif config.run_on_server_backend == "herdr":
                pane = state.get("herdr_pane")
                if not isinstance(pane, str):
                    print("indeterminate Herdr state has no pane", file=sys.stderr)
                    return 1
                code = parse_marker(_herdr_capture(config, pane), marker)
            else:
                print(
                    "SSH state cannot be recovered automatically; inspect the server and use --clear",
                    file=sys.stderr,
                )
                return 1
        except (OSError, RuntimeError, ValueError) as error:
            print(str(error), file=sys.stderr)
            return 1
        _write_state(server, {**state, "status": "complete", "exit_code": code})
        return code


def configure_and_execute(
    command: str, cwd: Path, config: Config, server: str | None = None
) -> int:
    configure_logging(config)
    return execute(command, config, cwd, server)
