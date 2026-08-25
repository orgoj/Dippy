"""Tests for CLI mode (--cmd, --stdin, --json)."""

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

import pytest


# Path to dippy-hook script
DIPPY_HOOK = Path(__file__).parent.parent / "bin" / "dippy-hook"


def run_dippy(*args, stdin_input=None):
    """Run dippy-hook with given arguments and return (stdout, stderr, returncode).

    Runs in an empty HOME and cwd so the developer's own ~/.dippy/config - and
    any .dippy found by walking up from the repo - cannot decide the outcome.
    These tests assert on built-in behaviour; without this they pass or fail
    depending on whose machine they run on.
    """
    cmd = [sys.executable, str(DIPPY_HOOK)] + list(args)
    env = dict(os.environ)
    env.pop("DIPPY_CONFIG", None)
    with tempfile.TemporaryDirectory() as sandbox:
        env["HOME"] = sandbox
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            input=stdin_input,
            env=env,
            cwd=sandbox,
        )
    return result.stdout.strip(), result.stderr.strip(), result.returncode


class TestCliModeBasic:
    """Basic CLI mode tests."""

    def test_cmd_allow_simple(self):
        """Test --cmd with a safe command returns allow and exit 0."""
        stdout, stderr, code = run_dippy("--cmd", "ls -la")
        assert code == 0
        assert stdout.startswith("allow:")

    def test_cmd_allow_git_status(self):
        """Test --cmd with git status returns allow."""
        stdout, stderr, code = run_dippy("--cmd", "git status")
        assert code == 0
        assert "allow" in stdout

    def test_cmd_ask_rm_rf(self):
        """Test --cmd with rm -rf returns ask and exit 2."""
        stdout, stderr, code = run_dippy("--cmd", "rm -rf /")
        assert code == 2
        assert stdout.startswith("ask:")

    def test_cmd_ask_docker(self):
        """Test --cmd with docker command returns ask."""
        stdout, stderr, code = run_dippy("--cmd", "docker run nginx")
        assert code == 2
        assert "ask" in stdout


class TestCliModeJson:
    """Tests for --json output."""

    def test_json_output_allow(self):
        """Test --json returns valid JSON with allow decision."""
        stdout, stderr, code = run_dippy("--cmd", "ls -la", "--json")
        assert code == 0
        data = json.loads(stdout)
        assert data["decision"] == "allow"
        assert "reason" in data

    def test_json_output_ask(self):
        """Test --json returns valid JSON with ask decision."""
        stdout, stderr, code = run_dippy("--cmd", "rm -rf /", "--json")
        assert code == 2
        data = json.loads(stdout)
        assert data["decision"] == "ask"
        assert "reason" in data


class TestCliModeStdin:
    """Tests for --stdin mode."""

    def test_stdin_allow(self):
        """Test --stdin reads command from stdin."""
        stdout, stderr, code = run_dippy("--stdin", stdin_input="ls -la")
        assert code == 0
        assert "allow" in stdout

    def test_stdin_ask(self):
        """Test --stdin with unsafe command."""
        stdout, stderr, code = run_dippy("--stdin", stdin_input="rm -rf /")
        assert code == 2
        assert "ask" in stdout

    def test_stdin_with_json(self):
        """Test --stdin combined with --json."""
        stdout, stderr, code = run_dippy("--stdin", "--json", stdin_input="git log")
        assert code == 0
        data = json.loads(stdout)
        assert data["decision"] == "allow"

    def test_stdin_empty(self):
        """Test --stdin with empty input returns error (exit 2 = input error)."""
        stdout, stderr, code = run_dippy("--stdin", stdin_input="")
        assert code == 2  # Input error, treated as "ask"
        assert "empty" in stderr.lower() or "empty" in stdout.lower()


class TestCliModeCwd:
    """Tests for --cwd option."""

    def test_cwd_option(self, tmp_path):
        """Test --cwd sets working directory."""
        stdout, stderr, code = run_dippy("--cmd", "pwd", "--cwd", str(tmp_path))
        # pwd is safe, should be allowed
        assert code == 0
        assert "allow" in stdout


class TestCliModeConfig:
    """Tests for --config option."""

    def test_config_file_not_found(self, tmp_path):
        """Test --config with non-existent file returns error."""
        stdout, stderr, code = run_dippy(
            "--cmd", "ls", "--config", str(tmp_path / "nonexistent.conf")
        )
        assert code == 2  # ask (config error)
        assert "config" in stdout.lower() or "not found" in stdout.lower()

    def test_config_custom_deny(self, tmp_path):
        """Test --config with custom deny rule."""
        # Use a fictional command not in any allowlist
        config_file = tmp_path / "test.conf"
        config_file.write_text('deny myapp * "custom deny message"')

        stdout, stderr, code = run_dippy(
            "--cmd", "myapp run", "--config", str(config_file), "--json"
        )
        assert code == 1  # deny
        data = json.loads(stdout)
        assert data["decision"] == "deny"
        assert "custom deny message" in data["reason"]

    def test_config_custom_allow(self, tmp_path):
        """Test --config with custom allow rule."""
        config_file = tmp_path / "test.conf"
        config_file.write_text("allow docker *")

        stdout, stderr, code = run_dippy(
            "--cmd", "docker run nginx", "--config", str(config_file), "--json"
        )
        assert code == 0  # allow
        data = json.loads(stdout)
        assert data["decision"] == "allow"


class TestCliModeExitCodes:
    """Tests for exit code correctness."""

    def test_exit_0_allow(self):
        """Exit code 0 for allowed commands."""
        _, _, code = run_dippy("--cmd", "echo hello")
        assert code == 0

    def test_exit_2_ask(self):
        """Exit code 2 for commands needing approval."""
        # docker run needs approval (not in simple safe, no explicit allow rule)
        _, _, code = run_dippy("--cmd", "docker run nginx")
        assert code == 2

    def test_exit_1_deny_with_config(self, tmp_path):
        """Exit code 1 for denied commands."""
        # Use a fictional command that can be denied by config
        config_file = tmp_path / "test.conf"
        config_file.write_text('deny mycommand * "no mycommand allowed"')

        _, _, code = run_dippy("--cmd", "mycommand foo", "--config", str(config_file))
        assert code == 1


class TestCliModeHelp:
    """Tests for --help."""

    def test_help(self):
        """Test --help shows usage information."""
        stdout, stderr, code = run_dippy("--help")
        assert code == 0
        assert "dippy" in stdout.lower()
        assert "--cmd" in stdout
        assert "--stdin" in stdout
        assert "--json" in stdout


class TestCliModeHookCompatibility:
    """Ensure CLI mode doesn't break hook mode."""

    def test_hook_mode_still_works(self):
        """Test that hook mode (JSON stdin) still works."""
        input_json = json.dumps(
            {"tool_name": "Bash", "tool_input": {"command": "ls -la"}}
        )
        cmd = [sys.executable, str(DIPPY_HOOK)]
        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            input=input_json,
        )
        assert result.returncode == 0
        output = json.loads(result.stdout)
        assert "hookSpecificOutput" in output
        assert output["hookSpecificOutput"]["permissionDecision"] == "allow"


class TestDirectExecution:
    def test_run_returns_command_exit_code(self):
        stdout, stderr, code = run_dippy("run", "exit 7")
        assert code == 7
        assert stdout == ""

    def test_run_executes_in_requested_cwd(self, tmp_path):
        stdout, _, code = run_dippy("--cwd", str(tmp_path), "run", "pwd")

        assert code == 0
        assert stdout == str(tmp_path)

    def test_run_accepts_exactly_one_command_string(self):
        _, stderr, code = run_dippy("run", "echo one", "echo two")
        assert code == 2
        assert "unrecognized arguments" in stderr

    def test_run_honors_config_override(self, tmp_path):
        config = tmp_path / "config"
        config.write_text('deny frob "blocked by override"\n')

        _, stderr, code = run_dippy("--config", str(config), "run", "frob")

        assert code == 1
        assert "blocked by override" in stderr

    def test_run_on_server_rejects_injection_before_ssh(self):
        _, stderr, code = run_dippy("run-on-server", "host;false", "echo safe")
        assert code == 1
        assert "invalid server alias" in stderr

    def test_run_on_server_rejects_undeclared_server(self):
        _, stderr, code = run_dippy("run-on-server", "srv", "frob")
        assert code == 1
        assert "server is not allowed" in stderr

    def test_recover_and_clear_complete_target(self, tmp_path):
        config = tmp_path / "config"
        config.write_text("server srv\nset run-on-server-backend tmux\n")

        recover_result = run_dippy("--config", str(config), "recover", "srv")
        clear_result = run_dippy("--config", str(config), "recover", "srv", "--clear")

        assert recover_result[2] == 0, recover_result
        assert clear_result[2] == 0, clear_result

    @pytest.mark.parametrize(
        "subcommand",
        [
            ("run", "frob"),
            ("run-on-server", "srv", "frob"),
            ("recover", "srv"),
        ],
    )
    def test_subcommands_report_missing_config(self, tmp_path, subcommand):
        _, stderr, code = run_dippy("--config", str(tmp_path / "missing"), *subcommand)

        assert code == 1
        assert "config error:" in stderr


class TestConfigCli:
    def test_user_set_get_unset(self, tmp_path):
        env = dict(os.environ)
        env.pop("DIPPY_CONFIG", None)
        env["HOME"] = str(tmp_path)
        commands = [
            ("set", "run-on-server-backend", "herdr"),
            ("get", "run-on-server-backend"),
            ("unset", "run-on-server-backend"),
        ]
        results = []
        for command in commands:
            results.append(
                subprocess.run(
                    [sys.executable, str(DIPPY_HOOK), "config", *command, "--user"],
                    capture_output=True,
                    text=True,
                    env=env,
                    cwd=tmp_path,
                )
            )
        assert [result.returncode for result in results] == [0, 0, 0]
        assert results[1].stdout.strip() == "herdr"
        assert not (tmp_path / ".dippy" / "config").read_text().strip()

    def test_set_invalid_value_returns_error(self):
        _, stderr, code = run_dippy(
            "config", "set", "askpass-timeout", "invalid", "--user"
        )

        assert code == 1
        assert "invalid value for askpass-timeout" in stderr

    def test_user_set_get_approval_wait_message(self, tmp_path):
        env = {**os.environ, "HOME": str(tmp_path)}
        message = "Contact the supervising agent and wait."

        set_result = subprocess.run(
            [
                sys.executable,
                str(DIPPY_HOOK),
                "config",
                "set",
                "--user",
                "approval-wait-message",
                message,
            ],
            capture_output=True,
            text=True,
            env=env,
            cwd=tmp_path,
        )
        get_result = subprocess.run(
            [
                sys.executable,
                str(DIPPY_HOOK),
                "config",
                "get",
                "--user",
                "approval-wait-message",
            ],
            capture_output=True,
            text=True,
            env=env,
            cwd=tmp_path,
        )

        assert set_result.returncode == 0
        assert get_result.returncode == 0
        assert get_result.stdout.strip() == message

    def test_cli_normalizes_underscore_setting_key(self, tmp_path):
        env = {**os.environ, "HOME": str(tmp_path)}

        set_result = subprocess.run(
            [
                sys.executable,
                str(DIPPY_HOOK),
                "config",
                "set",
                "--user",
                "askpass_timeout",
                "59",
            ],
            capture_output=True,
            text=True,
            env=env,
            cwd=tmp_path,
        )

        assert set_result.returncode == 0
        assert (tmp_path / ".dippy" / "config").read_text() == (
            "set askpass-timeout 59\n"
        )

    def test_get_without_key_prints_selected_config(self, tmp_path):
        config = tmp_path / ".dippy"
        config.write_text("# project\nserver alpha\n")
        env = {**os.environ, "HOME": str(tmp_path / "empty-home")}
        result = subprocess.run(
            [sys.executable, str(DIPPY_HOOK), "config", "get", "--project"],
            capture_output=True,
            text=True,
            env=env,
            cwd=tmp_path,
        )
        assert result.returncode == 0
        assert result.stdout == "# project\nserver alpha\n"

    def test_project_server_add_list_remove(self, tmp_path):
        env = dict(os.environ)
        env["HOME"] = str(tmp_path / "empty-home")
        actions = [("add", "alpha"), ("list",), ("remove", "alpha")]
        results = [
            subprocess.run(
                [
                    sys.executable,
                    str(DIPPY_HOOK),
                    "config",
                    "server",
                    *action,
                    "--project",
                ],
                capture_output=True,
                text=True,
                env=env,
                cwd=tmp_path,
            )
            for action in actions
        ]
        assert [result.returncode for result in results] == [0, 0, 0]
        assert results[1].stdout.strip() == "alpha"
        assert (tmp_path / ".dippy").read_text() == ""
