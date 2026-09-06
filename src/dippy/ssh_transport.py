"""Backend-independent SSH configuration and authentication selection."""

from __future__ import annotations

import hashlib
import os
import stat
import subprocess
import tempfile
import uuid
from dataclasses import dataclass
from pathlib import Path

from dippy.core.config import Config


@dataclass(frozen=True)
class SSHTransport:
    argv: list[str]
    env: dict[str, str]
    cwd: Path
    identity: str


def shell_quote(value: str) -> str:
    """Quote one literal POSIX shell argument without interpreting its contents."""
    return "'" + value.replace("'", "'\"'\"'") + "'"


def _option_path(value: str) -> str:
    # OpenSSH expands these even in quoted option arguments. Reject rather than
    # accidentally selecting another socket or environment-derived location.
    if any(c in value for c in ("%", "$", '"', "\\", "\n", "\r", "\x00")):
        raise ValueError(
            "SSH socket path contains unsupported OpenSSH expansion characters"
        )
    return '"' + value + '"'


def _private_control_path() -> str:
    directory = Path(tempfile.gettempdir()) / f"dippy-ssh-{os.getuid()}"
    directory.mkdir(mode=0o700, exist_ok=True)
    metadata = directory.lstat()
    if (
        not stat.S_ISDIR(metadata.st_mode)
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != 0o700
    ):
        raise ValueError(f"unsafe SSH control directory: {directory}")
    path = str(directory / uuid.uuid4().hex)
    if len(os.fsencode(path)) > 100:
        raise ValueError(
            "SSH control socket path is too long; use a shorter temporary directory"
        )
    return _option_path(path)


def build_transport(config: Config, server: str, cwd: Path) -> SSHTransport:
    """Resolve an optional profile before any approved command is launched."""
    env = os.environ.copy()
    profile = config.run_on_server_ssh_config
    socket = config.run_on_server_ssh_auth_sock
    argv = ["ssh"]
    profile_configured = (
        profile is not None or "run_on_server_ssh_config" in config.configured_settings
    )
    if (not profile_configured and socket is None) or (
        profile_configured and profile is None and socket == "none"
    ):
        return SSHTransport(argv + ["--", server, "bash", "-s"], env, cwd, "user")
    if profile is None or socket is None:
        raise ValueError(
            "SSH profile requires both ssh-config and ssh-auth-sock (or none)"
        )
    profile = profile.absolute()
    if not profile.is_file():
        raise ValueError(f"SSH config does not exist: {profile}")
    profile.read_bytes()  # Fail before execution when the config cannot be read.
    agent = "none"
    if socket == "none":
        env.pop("SSH_AUTH_SOCK", None)
    else:
        agent = _option_path(socket)
        if not stat.S_ISSOCK(Path(socket).stat().st_mode):
            raise ValueError(f"SSH auth socket is not a Unix socket: {socket}")
        env["SSH_AUTH_SOCK"] = socket
    env.pop("SSH_AGENT_PID", None)
    argv += ["-F", str(profile)]
    for option in (
        "IdentityFile=none",
        f"IdentityAgent={agent}",
        "IdentitiesOnly=yes",
        "BatchMode=yes",
        "ForwardAgent=no",
        "ControlPath=none",
        "ControlPersist=no",
        "PKCS11Provider=none",
        "PreferredAuthentications=publickey",
        "PermitLocalCommand=no",
    ):
        argv += ["-o", option]
    destination = ["--", server, "bash", "-s"]
    probe = subprocess.run(
        argv + ["-G"] + destination,
        cwd=cwd,
        env=env,
        capture_output=True,
        text=True,
        timeout=10,
    )
    if probe.returncode:
        raise ValueError(f"SSH profile validation failed: {probe.stderr.strip()}")
    options: dict[str, list[str]] = {}
    for line in probe.stdout.splitlines():
        key, _, value = line.partition(" ")
        options.setdefault(key.lower(), []).append(value.strip())
    if not any(value != "none" for value in options.get("identityfile", [])):
        raise ValueError(
            "SSH profile must explicitly select an IdentityFile (a public key is sufficient)"
        )
    if any(value != "none" for value in options.get("proxyjump", [])):
        raise ValueError(
            "Isolated SSH profiles do not support ProxyJump; use an explicitly isolated ProxyCommand"
        )
    identity = hashlib.sha256(
        (str(profile) + "\0" + socket + "\0" + probe.stdout).encode()
    ).hexdigest()
    # Honor the profile's ControlMaster mode, but never attach to authentication
    # cached by another operation (including an earlier version of this profile).
    # No persistent master is left after the operation finishes.
    argv[argv.index("ControlPath=none")] = "ControlPath=" + _private_control_path()
    return SSHTransport(argv + destination, env, cwd, identity)
