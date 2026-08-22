"""Tests for CLI mode (--cmd, --stdin, --json)."""

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path


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
