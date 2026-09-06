"""Tests for execution settings and comment-preserving config administration."""

from argparse import Namespace

from dippy.config_admin import edit_config
from dippy.core.config import Config, _merge_configs, parse_config
from dippy.dippy import handle_subcommand


def test_invalid_ssh_setting_admin_reports_error_without_writing(
    tmp_path, monkeypatch, capsys
):
    monkeypatch.chdir(tmp_path)
    args = Namespace(
        subcommand="config",
        project=True,
        config_action="set",
        key="run-on-server-ssh-config",
        value="",
    )
    assert handle_subcommand(args) == 1
    assert "SSH profile" in capsys.readouterr().err
    assert not (tmp_path / ".dippy").exists()


def test_admin_accepts_project_profile_keys(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    for key, value in [
        ("run-on-server-ssh-config", "ssh/config"),
        ("run-on-server-ssh-auth-sock", "none"),
    ]:
        assert (
            handle_subcommand(
                Namespace(
                    subcommand="config",
                    project=True,
                    config_action="set",
                    key=key,
                    value=value,
                )
            )
            == 0
        )
    parsed = parse_config(
        (tmp_path / ".dippy").read_text(), source=str(tmp_path / ".dippy")
    )
    assert parsed.run_on_server_ssh_config == tmp_path / "ssh/config"
    assert parsed.run_on_server_ssh_auth_sock == "none"


def test_execution_settings_and_servers_parse():
    config = parse_config(
        """
server alpha
server beta
set run-on-server-backend herdr
set run-on-server-session managed
set run-on-server-timeout 42
set run-on-server-poll-interval 0.25
"""
    )
    assert config.servers == ["alpha", "beta"]
    assert config.run_on_server_backend == "herdr"
    assert config.run_on_server_session == "managed"
    assert config.run_on_server_timeout == 42
    assert config.run_on_server_poll_interval == 0.25


def test_project_execution_setting_overrides_user_default_value():
    user = parse_config("set run-on-server-backend herdr")
    project = parse_config("set run-on-server-backend ssh")
    assert _merge_configs(user, project).run_on_server_backend == "ssh"


def test_project_can_explicitly_restore_default_askpass_timeout():
    user = parse_config("set askpass-timeout 1800")
    project = parse_config("set askpass-timeout 59")

    assert _merge_configs(user, project).askpass_timeout == 59


def test_approval_wait_message_parses_and_project_overrides_user():
    user = parse_config('set approval-wait-message "Wait for the user."')
    project = parse_config(
        'set approval-wait-message "Contact the supervising agent and wait."'
    )

    assert user.approval_wait_message == "Wait for the user."
    assert (
        _merge_configs(user, project).approval_wait_message
        == "Contact the supervising agent and wait."
    )


def test_empty_approval_wait_message_keeps_safe_default():
    config = parse_config('set approval-wait-message ""')

    assert "wait for the user" in config.approval_wait_message.lower()


def test_edit_config_preserves_rules_and_comments(tmp_path):
    path = tmp_path / ".dippy"
    path.write_text("# keep me\nallow frob status\nset askpass old\nserver alpha\n")
    edit_config(path, "set", "askpass", "new-provider")
    edit_config(path, "server-add", "beta")
    edit_config(path, "server-remove", "alpha")
    assert path.read_text() == (
        "# keep me\nallow frob status\nset askpass new-provider\nserver beta\n"
    )


def test_edit_config_rewrites_underscore_key_to_canonical_hyphens(tmp_path):
    path = tmp_path / "config"
    path.write_text("set askpass_timeout 30\n")

    edit_config(path, "set", "askpass-timeout", "59")

    assert path.read_text() == "set askpass-timeout 59\n"


def test_edit_config_writes_atomically(tmp_path, monkeypatch):
    path = tmp_path / "config"
    path.write_text("# original\n")
    replaced = []
    monkeypatch.setattr(
        "dippy.config_admin.os.replace",
        lambda source, target: replaced.append((source, target)),
    )
    edit_config(path, "set", "run-on-server-backend", "tmux")
    assert replaced and replaced[0][1] == path


def test_config_dataclass_defaults_are_stable():
    config = Config()
    assert config.run_on_server_backend == "ssh"
    assert config.run_on_server_timeout > 0
    assert config.run_on_server_poll_interval > 0
    assert "wait for the user" in config.approval_wait_message.lower()


def test_injection_server_directive_is_skipped():
    assert parse_config("server host;touch-x").servers == []
