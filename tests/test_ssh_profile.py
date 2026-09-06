"""Optional SSH profiles must not inherit user authentication."""

import subprocess
import socket
import shutil
from pathlib import Path

import pytest

from dippy.core.config import (
    Config,
    ConfigError,
    _load_config_file,
    _merge_configs,
    parse_config,
)
from dippy.ssh_transport import build_transport


@pytest.fixture(autouse=True)
def isolated_environment(tmp_path, monkeypatch):
    monkeypatch.setenv("HOME", str(tmp_path))
    monkeypatch.setenv("DIPPY_ASKPASS", "/bin/false")
    monkeypatch.setenv("SSH_ASKPASS", "/bin/false")


def test_profile_paths_and_explicit_reset(tmp_path):
    config = parse_config(
        'set run-on-server-ssh-config "ssh settings/config"\n'
        "set run-on-server-ssh-auth-sock none",
        source=str(tmp_path / ".dippy"),
    )
    assert config.run_on_server_ssh_config == tmp_path / "ssh settings/config"
    assert config.run_on_server_ssh_auth_sock == "none"
    reset = parse_config(
        "set run-on-server-ssh-config none\nset run-on-server-ssh-auth-sock none"
    )
    merged = _merge_configs(config, reset)
    assert merged.run_on_server_ssh_config is None
    assert merged.run_on_server_ssh_auth_sock == "none"


@pytest.mark.parametrize("directive", ["set ", "SET\t", "set\t"])
def test_included_profile_path_uses_declaring_directory(tmp_path, directive):
    nested = tmp_path / "nested"
    nested.mkdir()
    (nested / "rules").write_text(f"{directive}run-on-server-ssh-config ssh-config\n")
    (tmp_path / ".dippy").write_text("include nested/rules\n")
    assert (
        _load_config_file(tmp_path / ".dippy").run_on_server_ssh_config
        == nested / "ssh-config"
    )


def test_default_transport_preserves_user_settings(tmp_path, monkeypatch):
    monkeypatch.setenv("SSH_AUTH_SOCK", "/user/agent")
    transport = build_transport(Config(), "srv", tmp_path)
    assert transport.argv == ["ssh", "--", "srv", "bash", "-s"]
    assert transport.env["SSH_AUTH_SOCK"] == "/user/agent"


@pytest.mark.parametrize(
    "settings",
    [
        {"run_on_server_ssh_config": Path("/missing")},
        {"run_on_server_ssh_auth_sock": "/missing"},
    ],
)
def test_partial_profile_fails_without_starting_ssh(tmp_path, monkeypatch, settings):
    def unexpected(*args, **kwargs):
        pytest.fail("partial profile launched a subprocess")

    monkeypatch.setattr(subprocess, "run", unexpected)
    with pytest.raises(ValueError, match="both"):
        build_transport(Config(**settings), "srv", tmp_path)


def test_profile_removes_user_agent_and_requires_selected_identity(
    tmp_path, monkeypatch
):
    profile = tmp_path / "ssh config"
    profile.write_text("Host *\n IdentityFile /project/key.pub\n")
    monkeypatch.setenv("SSH_AUTH_SOCK", "/user/agent")
    calls = []

    def probe(argv, **kwargs):
        calls.append((argv, kwargs))
        return subprocess.CompletedProcess(
            argv, 0, "identityfile none\nidentityfile /project/key.pub\n", ""
        )

    monkeypatch.setattr(subprocess, "run", probe)
    transport = build_transport(
        Config(run_on_server_ssh_config=profile, run_on_server_ssh_auth_sock="none"),
        "srv",
        tmp_path,
    )
    assert "SSH_AUTH_SOCK" not in transport.env
    assert "IdentityFile=none" in transport.argv
    assert "IdentityAgent=none" in transport.argv
    assert transport.argv[transport.argv.index("-F") + 1] == str(profile)
    assert calls[0][1]["cwd"] == tmp_path
    assert calls[0][0][-4:] == ["--", "srv", "bash", "-s"]


@pytest.mark.parametrize(
    "effective",
    [
        "identityfile none\n",
        "identityfile /project/key\nproxyjump bastion\n",
    ],
)
def test_unenforceable_profile_fails_closed(tmp_path, monkeypatch, effective):
    profile = tmp_path / "config"
    profile.write_text("Host *\n")
    monkeypatch.setattr(
        subprocess,
        "run",
        lambda argv, **kw: subprocess.CompletedProcess(argv, 0, effective, ""),
    )
    with pytest.raises(ValueError):
        build_transport(
            Config(
                run_on_server_ssh_config=profile, run_on_server_ssh_auth_sock="none"
            ),
            "srv",
            tmp_path,
        )


def test_private_control_socket_is_fresh_for_each_operation(tmp_path, monkeypatch):
    profile = tmp_path / "config"
    profile.write_text("Host *\n IdentityFile /project/key.pub\n ControlMaster auto\n")
    monkeypatch.setattr(
        subprocess,
        "run",
        lambda argv, **kw: subprocess.CompletedProcess(
            argv,
            0,
            "identityfile none\nidentityfile /project/key.pub\ncontrolmaster auto\n",
            "",
        ),
    )
    config = Config(
        run_on_server_ssh_config=profile, run_on_server_ssh_auth_sock="none"
    )
    a = build_transport(config, "srv", tmp_path)
    b = build_transport(config, "srv", tmp_path)
    assert a.identity == b.identity
    first = next(arg for arg in a.argv if arg.startswith("ControlPath="))
    second = next(arg for arg in b.argv if arg.startswith("ControlPath="))
    assert first != second
    assert "ControlMaster=no" not in a.argv
    assert "ControlPersist=no" in a.argv


def test_explicit_agent_socket_replaces_inherited_socket(tmp_path, monkeypatch):
    profile = tmp_path / "config"
    profile.write_text("Host *\n")
    agent_path = str(tmp_path / "agent ' socket")
    monkeypatch.setenv("SSH_AUTH_SOCK", "/user/agent")
    with socket.socket(socket.AF_UNIX) as agent:
        agent.bind(agent_path)
        monkeypatch.setattr(
            subprocess,
            "run",
            lambda argv, **kw: subprocess.CompletedProcess(
                argv, 0, "identityfile /project/key.pub\n", ""
            ),
        )
        transport = build_transport(
            Config(
                run_on_server_ssh_config=profile, run_on_server_ssh_auth_sock=agent_path
            ),
            "srv",
            tmp_path,
        )
    assert transport.env["SSH_AUTH_SOCK"] == agent_path
    assert f'IdentityAgent="{agent_path}"' in transport.argv


@pytest.mark.parametrize(
    "name", ["agent%h", "agent${HOME}", "agent\\socket", 'agent"socket']
)
def test_socket_expansion_is_rejected(tmp_path, name):
    profile = tmp_path / "config"
    profile.write_text("Host *\n")
    with pytest.raises(ValueError, match="expansion"):
        build_transport(
            Config(
                run_on_server_ssh_config=profile,
                run_on_server_ssh_auth_sock=str(tmp_path / name),
            ),
            "srv",
            tmp_path,
        )


@pytest.mark.skipif(shutil.which("ssh") is None, reason="OpenSSH is not installed")
def test_openssh_effective_profile_has_no_default_identities(tmp_path):
    profile = tmp_path / "ssh '$% config"
    key = tmp_path / "selected.pub"
    profile.write_text(f'Host *\n IdentityFile "{key}"\n ControlMaster auto\n')
    transport = build_transport(
        Config(run_on_server_ssh_config=profile, run_on_server_ssh_auth_sock="none"),
        "fictional-server.invalid",
        tmp_path,
    )
    argv = transport.argv.copy()
    argv.insert(1, "-G")
    result = subprocess.run(
        argv, cwd=tmp_path, env=transport.env, capture_output=True, text=True, timeout=5
    )
    assert result.returncode == 0
    identities = [
        line for line in result.stdout.splitlines() if line.startswith("identityfile ")
    ]
    assert identities == ["identityfile none", f"identityfile {key}"]
    assert "identityagent none\n" in result.stdout
    assert "controlmaster auto\n" in result.stdout
    assert "controlpersist no\n" in result.stdout
    assert "forwardagent no\n" in result.stdout


def test_missing_socket_fails_before_any_probe(tmp_path, monkeypatch):
    profile = tmp_path / "config"
    profile.write_text("Host *\n")
    monkeypatch.setattr(subprocess, "run", lambda *a, **kw: pytest.fail("started ssh"))
    with pytest.raises(OSError):
        build_transport(
            Config(
                run_on_server_ssh_config=profile,
                run_on_server_ssh_auth_sock=str(tmp_path / "missing"),
            ),
            "srv",
            tmp_path,
        )


@pytest.mark.parametrize(
    "text",
    [
        "set run-on-server-ssh-config",
        'set run-on-server-ssh-config ""',
        "set run-on-server-ssh-auth-sock",
        "set run-on-server-ssh-confg /project/config",
    ],
)
def test_invalid_profile_directive_cannot_silently_restore_user_mode(text):
    with pytest.raises(ConfigError, match="SSH profile"):
        parse_config(text)


@pytest.mark.parametrize("setting", ["ssh-config", "ssh-auth-sock"])
def test_partial_reset_cannot_silently_select_user_mode(tmp_path, setting):
    config = parse_config(f"set run-on-server-{setting} none")
    with pytest.raises(ValueError, match="both"):
        build_transport(config, "srv", tmp_path)


def test_complete_reset_selects_user_mode(tmp_path):
    config = parse_config(
        "set run-on-server-ssh-config none\nset run-on-server-ssh-auth-sock none"
    )
    assert build_transport(config, "srv", tmp_path).identity == "user"


def test_empty_loaded_profile_setting_is_invalid(tmp_path):
    config = tmp_path / ".dippy"
    config.write_text('set run-on-server-ssh-config ""\n')
    with pytest.raises(ConfigError, match="SSH profile"):
        _load_config_file(config)
