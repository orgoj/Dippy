"""Tests for Antigravity CLI / AGY hook integration."""

from __future__ import annotations

import json
from io import StringIO
from pathlib import Path

import pytest

import dippy.dippy as dippy_mod
import dippy.core.config as config_mod


class TestAgyModeDetection:
    """Test AGY mode detection from flags, env vars, and JSON input."""

    def test_detect_mode_from_flags_agy(self, monkeypatch):
        monkeypatch.setattr("sys.argv", ["dippy", "--agy"])
        assert dippy_mod._detect_mode_from_flags() == "agy"

    def test_detect_mode_from_flags_antigravity(self, monkeypatch):
        monkeypatch.setattr("sys.argv", ["dippy", "--antigravity"])
        assert dippy_mod._detect_mode_from_flags() == "agy"

    def test_detect_mode_from_env_agy(self, monkeypatch):
        monkeypatch.setattr("sys.argv", ["dippy"])
        monkeypatch.setenv("DIPPY_AGY", "1")
        assert dippy_mod._detect_mode_from_flags() == "agy"

    def test_detect_mode_from_env_antigravity(self, monkeypatch):
        monkeypatch.setattr("sys.argv", ["dippy"])
        monkeypatch.setenv("DIPPY_ANTIGRAVITY", "1")
        assert dippy_mod._detect_mode_from_flags() == "agy"

    def test_detect_mode_from_input_tool_call(self):
        input_data = {
            "toolCall": {
                "name": "run_command",
                "args": {"CommandLine": "git status"},
            }
        }
        assert dippy_mod._detect_mode_from_input(input_data) == "agy"


class TestAgyResponses:
    """Test response generation for AGY mode."""

    def test_approve_agy_format(self, monkeypatch):
        monkeypatch.setattr(dippy_mod, "MODE", "agy")
        resp = dippy_mod.approve("all commands safe", command="true")
        assert resp["decision"] == "allow"
        assert resp["reason"] == "🐤 all commands safe"
        assert resp["permissionOverrides"] == ["command(true)"]

    def test_ask_agy_without_askpass_fails_closed(self, monkeypatch):
        monkeypatch.setattr(dippy_mod, "MODE", "agy")
        resp = dippy_mod.ask("needs approval")
        assert resp["decision"] == "deny"

    def test_ask_agy_with_askpass_allow(self, tmp_path, monkeypatch):
        fake_askpass = tmp_path / "fake_askpass.py"
        fake_askpass.write_text("#!/usr/bin/env python3\nimport sys\nsys.exit(0)\n")
        fake_askpass.chmod(0o755)

        monkeypatch.setattr(dippy_mod, "MODE", "agy")
        monkeypatch.setenv("DIPPY_ASKPASS", str(fake_askpass))

        from dippy.core.config import Config

        config = Config(askpass=fake_askpass)

        resp = dippy_mod.ask("needs approval", config=config, command="fictional_cmd")
        assert resp["decision"] == "allow"
        assert "approved by user" in resp["reason"]
        assert resp["permissionOverrides"] == ["command(fictional_cmd)"]

    def test_ask_agy_with_askpass_deny(self, tmp_path, monkeypatch):
        fake_askpass = tmp_path / "fake_askpass.py"
        fake_askpass.write_text("#!/usr/bin/env python3\nimport sys\nsys.exit(1)\n")
        fake_askpass.chmod(0o755)

        monkeypatch.setattr(dippy_mod, "MODE", "agy")
        monkeypatch.setenv("DIPPY_ASKPASS", str(fake_askpass))

        from dippy.core.config import Config

        config = Config(askpass=fake_askpass)

        resp = dippy_mod.ask("needs approval", config=config, command="fictional_cmd")
        assert resp["decision"] == "deny"

    def test_deny_agy_format(self, monkeypatch):
        monkeypatch.setattr(dippy_mod, "MODE", "agy")
        resp = dippy_mod.deny("denied by config")
        assert resp == {
            "decision": "deny",
            "reason": "🐤 denied by config",
        }

    def test_pass_agy_format_default_allow(self, monkeypatch):
        monkeypatch.setattr(dippy_mod, "MODE", "agy")
        from dippy.core.config import Config

        config = Config(default="allow")
        resp = dippy_mod.pass_("passing through", config=config)
        assert resp == {
            "decision": "allow",
            "reason": "🐤 passing through",
        }

    def test_pass_agy_format_default_ask_fails_closed(self, monkeypatch):
        monkeypatch.setattr(dippy_mod, "MODE", "agy")
        from dippy.core.config import Config

        config = Config(default="ask")
        resp = dippy_mod.pass_("passing through", config=config)
        assert resp["decision"] == "deny"

    def test_post_tool_response_agy_format(self, monkeypatch):
        monkeypatch.setattr(dippy_mod, "MODE", "agy")
        resp = dippy_mod.post_tool_response("done")
        assert resp == {}


class TestAgyMainHookExecution:
    """Test main() hook execution with AGY payload format."""

    @pytest.fixture(autouse=True)
    def isolate_policy(self, tmp_path, monkeypatch):
        monkeypatch.setenv("HOME", str(tmp_path))
        monkeypatch.setenv("DIPPY_POLICY_CWD", str(tmp_path))
        monkeypatch.setattr(config_mod, "USER_CONFIG", tmp_path / ".dippy" / "config")
        askpass = tmp_path / "fake_askpass"
        askpass.write_text("#!/bin/sh\nexit 1\n")
        askpass.chmod(0o755)
        monkeypatch.setenv("DIPPY_ASKPASS", str(askpass))

    def test_tool_cwd_cannot_select_another_project_policy(
        self, tmp_path, monkeypatch, capsys
    ):
        policy_dir = tmp_path / "wiki"
        other_dir = tmp_path / "other"
        policy_dir.mkdir()
        other_dir.mkdir()
        (policy_dir / ".dippy").write_text("allow fictional_hcom_send *\n")
        (other_dir / ".dippy").write_text("deny fictional_hcom_send *\n")
        monkeypatch.setenv("DIPPY_POLICY_CWD", str(policy_dir))
        payload = {
            "toolCall": {
                "name": "run_command",
                "args": {
                    "CommandLine": "fictional_hcom_send message",
                    "Cwd": str(other_dir),
                },
            },
            "workspacePaths": [str(other_dir), str(policy_dir)],
        }
        monkeypatch.setattr("sys.stdin", StringIO(json.dumps(payload)))
        monkeypatch.setattr("sys.argv", ["dippy", "--agy"])

        dippy_mod.main()

        out, _ = capsys.readouterr()
        assert json.loads(out.strip())["decision"] == "allow"

    def test_web_search_uses_launch_policy_with_reordered_workspaces(
        self, tmp_path, monkeypatch, capsys
    ):
        policy_dir = tmp_path / "wiki"
        other_dir = tmp_path / "bundle"
        policy_dir.mkdir()
        other_dir.mkdir()
        (policy_dir / ".dippy").write_text("allow-web *\n")
        (other_dir / ".dippy").write_text("deny-web *\n")
        monkeypatch.setenv("DIPPY_POLICY_CWD", str(policy_dir))
        monkeypatch.chdir(other_dir)
        payload = {
            "toolCall": {"name": "search_web", "args": {"query": "fictional query"}},
            "workspacePaths": [str(other_dir), str(policy_dir)],
        }
        monkeypatch.setattr("sys.stdin", StringIO(json.dumps(payload)))
        monkeypatch.setattr("sys.argv", ["dippy", "--agy"])

        dippy_mod.main()

        out, _ = capsys.readouterr()
        assert json.loads(out.strip())["decision"] == "allow"

    def test_web_search_audit_records_launch_policy(
        self, tmp_path, monkeypatch, capsys
    ):
        policy_dir = tmp_path / "wiki"
        policy_dir.mkdir()
        audit_file = tmp_path / "audit.log"
        (policy_dir / ".dippy").write_text(f"set log {audit_file}\nallow-web *\n")
        monkeypatch.delenv("DIPPY_TEST_NO_LOG")
        monkeypatch.setenv("DIPPY_POLICY_CWD", str(policy_dir))
        payload = {
            "toolCall": {"name": "search_web", "args": {"query": "fictional query"}},
            "workspacePaths": [str(tmp_path), str(policy_dir)],
        }
        monkeypatch.setattr("sys.stdin", StringIO(json.dumps(payload)))
        monkeypatch.setattr("sys.argv", ["dippy", "--agy"])

        dippy_mod.main()

        out, _ = capsys.readouterr()
        assert json.loads(out.strip())["decision"] == "allow"
        entry = json.loads(audit_file.read_text().splitlines()[-1])
        assert entry["policy_cwd"] == str(policy_dir)

    def test_relative_file_rule_stays_anchored_to_launch_workspace(
        self, tmp_path, monkeypatch, capsys
    ):
        policy_dir = tmp_path / "wiki"
        other_dir = tmp_path / "other"
        policy_dir.mkdir()
        other_dir.mkdir()
        (policy_dir / ".dippy").write_text("allow-edit ./**\n")
        monkeypatch.setenv("DIPPY_POLICY_CWD", str(policy_dir))
        monkeypatch.setattr("sys.argv", ["dippy", "--agy"])
        payload = {
            "toolCall": {
                "name": "write_to_file",
                "args": {
                    "TargetFile": str(other_dir / "report.md"),
                    "Cwd": str(other_dir),
                },
            },
            "workspacePaths": [str(other_dir), str(policy_dir)],
        }
        monkeypatch.setattr("sys.stdin", StringIO(json.dumps(payload)))

        dippy_mod.main()

        out, _ = capsys.readouterr()
        assert json.loads(out.strip())["decision"] == "deny"

        payload["toolCall"]["args"]["TargetFile"] = str(policy_dir / "report.md")
        monkeypatch.setattr("sys.stdin", StringIO(json.dumps(payload)))
        dippy_mod.main()

        out, _ = capsys.readouterr()
        assert json.loads(out.strip())["decision"] == "allow"

    def test_relative_redirect_rule_stays_anchored_to_launch_workspace(
        self, tmp_path, monkeypatch, capsys
    ):
        policy_dir = tmp_path / "wiki"
        other_dir = tmp_path / "other"
        policy_dir.mkdir()
        other_dir.mkdir()
        (policy_dir / ".dippy").write_text("allow-redirect ./**\n")
        monkeypatch.setenv("DIPPY_POLICY_CWD", str(policy_dir))
        payload = {
            "toolCall": {
                "name": "run_command",
                "args": {"CommandLine": "echo note > report.md", "Cwd": str(other_dir)},
            },
            "workspacePaths": [str(other_dir), str(policy_dir)],
        }
        monkeypatch.setattr("sys.stdin", StringIO(json.dumps(payload)))
        monkeypatch.setattr("sys.argv", ["dippy", "--agy"])

        dippy_mod.main()

        out, _ = capsys.readouterr()
        assert json.loads(out.strip())["decision"] == "deny"

    def test_relative_tool_cwd_is_based_on_launch_workspace(
        self, tmp_path, monkeypatch, capsys
    ):
        policy_dir = tmp_path / "wiki"
        report_dir = policy_dir / "reports"
        ambient_dir = tmp_path / "ambient"
        report_dir.mkdir(parents=True)
        ambient_dir.mkdir()
        (policy_dir / ".dippy").write_text("allow-edit ./reports/**\n")
        monkeypatch.setenv("DIPPY_POLICY_CWD", str(policy_dir))
        monkeypatch.chdir(ambient_dir)
        payload = {
            "toolCall": {
                "name": "write_to_file",
                "args": {"TargetFile": "result.md", "Cwd": "reports"},
            }
        }
        monkeypatch.setattr("sys.stdin", StringIO(json.dumps(payload)))
        monkeypatch.setattr("sys.argv", ["dippy", "--agy"])

        dippy_mod.main()

        out, _ = capsys.readouterr()
        assert json.loads(out.strip())["decision"] == "allow"

    def test_relative_command_rule_stays_anchored_to_launch_workspace(
        self, tmp_path, monkeypatch, capsys
    ):
        policy_dir = tmp_path / "wiki"
        other_dir = tmp_path / "other"
        policy_dir.mkdir()
        other_dir.mkdir()
        (policy_dir / ".dippy").write_text("allow fictional_runner _scripts/**\n")
        monkeypatch.setenv("DIPPY_POLICY_CWD", str(policy_dir))
        monkeypatch.setattr("sys.argv", ["dippy", "--agy"])
        payload = {
            "toolCall": {
                "name": "run_command",
                "args": {
                    "CommandLine": "fictional_runner _scripts/task",
                    "Cwd": str(other_dir),
                },
            }
        }
        monkeypatch.setattr("sys.stdin", StringIO(json.dumps(payload)))

        dippy_mod.main()

        out, _ = capsys.readouterr()
        assert json.loads(out.strip())["decision"] == "deny"

        payload["toolCall"]["args"]["Cwd"] = str(policy_dir)
        monkeypatch.setattr("sys.stdin", StringIO(json.dumps(payload)))
        dippy_mod.main()

        out, _ = capsys.readouterr()
        assert json.loads(out.strip())["decision"] == "allow"

    def test_launch_command_allow_does_not_approve_destructive_compound(
        self, tmp_path, monkeypatch, capsys
    ):
        (tmp_path / ".dippy").write_text("allow fictional_hcom_send *\n")
        payload = {
            "toolCall": {
                "name": "run_command",
                "args": {
                    "CommandLine": "fictional_hcom_send message; rm -rf /",
                    "Cwd": str(tmp_path),
                },
            }
        }
        monkeypatch.setattr("sys.stdin", StringIO(json.dumps(payload)))
        monkeypatch.setattr("sys.argv", ["dippy", "--agy"])

        dippy_mod.main()

        out, _ = capsys.readouterr()
        assert json.loads(out.strip())["decision"] == "deny"

    def test_standalone_agy_uses_workspace_policy_not_tool_cwd(
        self, tmp_path, monkeypatch, capsys
    ):
        project = tmp_path / "project"
        other = tmp_path / "other"
        project.mkdir()
        other.mkdir()
        (project / ".dippy").write_text("allow fictional_safe_cmd *\n")
        (other / ".dippy").write_text("deny fictional_safe_cmd *\n")
        monkeypatch.delenv("DIPPY_POLICY_CWD")
        monkeypatch.chdir(tmp_path)
        payload = {
            "toolCall": {
                "name": "run_command",
                "args": {"CommandLine": "fictional_safe_cmd x", "Cwd": str(other)},
            },
            "workspacePaths": [str(project)],
        }
        monkeypatch.setattr("sys.stdin", StringIO(json.dumps(payload)))
        monkeypatch.setattr("sys.argv", ["dippy", "--agy"])

        dippy_mod.main()

        out, _ = capsys.readouterr()
        assert json.loads(out.strip())["decision"] == "allow"

    def test_standalone_agy_uses_process_cwd_without_workspace_paths(
        self, tmp_path, monkeypatch, capsys
    ):
        (tmp_path / ".dippy").write_text("allow-web *\n")
        monkeypatch.delenv("DIPPY_POLICY_CWD")
        monkeypatch.chdir(tmp_path)
        payload = {
            "toolCall": {"name": "search_web", "args": {"query": "fictional query"}}
        }
        monkeypatch.setattr("sys.stdin", StringIO(json.dumps(payload)))
        monkeypatch.setattr("sys.argv", ["dippy", "--agy"])

        dippy_mod.main()

        out, _ = capsys.readouterr()
        assert json.loads(out.strip())["decision"] == "allow"

    def test_standalone_agy_prefers_process_cwd_in_workspace_paths(
        self, tmp_path, monkeypatch, capsys
    ):
        project = tmp_path / "project"
        other = tmp_path / "other"
        project.mkdir()
        other.mkdir()
        (project / ".dippy").write_text("allow-web *\n")
        (other / ".dippy").write_text("deny-web *\n")
        monkeypatch.delenv("DIPPY_POLICY_CWD")
        monkeypatch.chdir(project)
        payload = {
            "toolCall": {"name": "search_web", "args": {"query": "fictional query"}},
            "workspacePaths": [str(other), str(project)],
        }
        monkeypatch.setattr("sys.stdin", StringIO(json.dumps(payload)))
        monkeypatch.setattr("sys.argv", ["dippy", "--agy"])

        dippy_mod.main()

        out, _ = capsys.readouterr()
        assert json.loads(out.strip())["decision"] == "allow"

    @pytest.mark.parametrize("policy_value", ["relative/path", "/no/such/path"])
    def test_invalid_launch_policy_fails_closed(
        self, policy_value, tmp_path, monkeypatch, capsys
    ):
        (tmp_path / ".dippy").write_text("allow fictional_safe_cmd *\n")
        if policy_value is None:
            monkeypatch.delenv("DIPPY_POLICY_CWD")
        else:
            monkeypatch.setenv("DIPPY_POLICY_CWD", policy_value)
        payload = {
            "toolCall": {
                "name": "run_command",
                "args": {"CommandLine": "fictional_safe_cmd x", "Cwd": str(tmp_path)},
            },
            "workspacePaths": [str(tmp_path)],
        }
        monkeypatch.setattr("sys.stdin", StringIO(json.dumps(payload)))
        monkeypatch.setattr("sys.argv", ["dippy", "--agy"])

        dippy_mod.main()

        out, _ = capsys.readouterr()
        assert json.loads(out.strip())["decision"] == "deny"

    def test_agy_run_command_allowed(self, tmp_path, monkeypatch, capsys):
        config_file = tmp_path / ".dippy"
        config_file.write_text("allow fictional_safe_cmd *\n")

        payload = {
            "toolCall": {
                "name": "run_command",
                "args": {
                    "CommandLine": "fictional_safe_cmd --flag",
                    "Cwd": str(tmp_path),
                },
            },
            "workspacePaths": [str(tmp_path)],
        }

        monkeypatch.setattr("sys.stdin", StringIO(json.dumps(payload)))
        monkeypatch.setattr("sys.argv", ["dippy", "--agy"])
        monkeypatch.setattr(Path, "home", lambda: tmp_path)

        dippy_mod.main()

        out, _ = capsys.readouterr()
        data = json.loads(out.strip())
        assert data["decision"] == "allow"

    def test_agy_run_command_denied(self, tmp_path, monkeypatch, capsys):
        config_file = tmp_path / ".dippy"
        config_file.write_text("deny fictional_blocked_cmd *\n")

        payload = {
            "toolCall": {
                "name": "run_command",
                "args": {
                    "CommandLine": "fictional_blocked_cmd arg",
                    "Cwd": str(tmp_path),
                },
            },
            "workspacePaths": [str(tmp_path)],
        }

        monkeypatch.setattr("sys.stdin", StringIO(json.dumps(payload)))
        monkeypatch.setattr("sys.argv", ["dippy", "--agy"])
        monkeypatch.setattr(Path, "home", lambda: tmp_path)

        dippy_mod.main()

        out, _ = capsys.readouterr()
        data = json.loads(out.strip())
        assert data["decision"] == "deny"

    def test_agy_view_file_read_rule(self, tmp_path, monkeypatch, capsys):
        config_file = tmp_path / ".dippy"
        config_file.write_text("allow-read src/**\ndeny-read secret/**\n")

        # Allow read
        payload_allow = {
            "toolCall": {
                "name": "view_file",
                "args": {
                    "AbsolutePath": str(tmp_path / "src" / "main.py"),
                },
            },
            "workspacePaths": [str(tmp_path)],
        }

        monkeypatch.setattr("sys.stdin", StringIO(json.dumps(payload_allow)))
        monkeypatch.setattr("sys.argv", ["dippy", "--agy"])
        monkeypatch.setattr(Path, "home", lambda: tmp_path)

        dippy_mod.main()

        out, _ = capsys.readouterr()
        data = json.loads(out.strip())
        assert data["decision"] == "allow"

        # Deny read
        payload_deny = {
            "toolCall": {
                "name": "view_file",
                "args": {
                    "AbsolutePath": str(tmp_path / "secret" / "keys.txt"),
                },
            },
            "workspacePaths": [str(tmp_path)],
        }

        monkeypatch.setattr("sys.stdin", StringIO(json.dumps(payload_deny)))
        monkeypatch.setattr("sys.argv", ["dippy", "--agy"])

        dippy_mod.main()

        out, _ = capsys.readouterr()
        data = json.loads(out.strip())
        assert data["decision"] == "deny"

    def test_agy_write_to_file_edit_rule(self, tmp_path, monkeypatch, capsys):
        config_file = tmp_path / ".dippy"
        config_file.write_text("allow-edit src/**\ndeny-edit .env*\n")

        # Allow edit
        payload_allow = {
            "toolCall": {
                "name": "write_to_file",
                "args": {
                    "TargetFile": str(tmp_path / "src" / "app.py"),
                    "CodeContent": "print('hello')",
                },
            },
            "workspacePaths": [str(tmp_path)],
        }

        monkeypatch.setattr("sys.stdin", StringIO(json.dumps(payload_allow)))
        monkeypatch.setattr("sys.argv", ["dippy", "--agy"])
        monkeypatch.setattr(Path, "home", lambda: tmp_path)

        dippy_mod.main()

        out, _ = capsys.readouterr()
        data = json.loads(out.strip())
        assert data["decision"] == "allow"

        # Deny edit
        payload_deny = {
            "toolCall": {
                "name": "replace_file_content",
                "args": {
                    "TargetFile": str(tmp_path / ".env"),
                    "ReplacementContent": "SECRET=1",
                },
            },
            "workspacePaths": [str(tmp_path)],
        }

        monkeypatch.setattr("sys.stdin", StringIO(json.dumps(payload_deny)))
        monkeypatch.setattr("sys.argv", ["dippy", "--agy"])

        dippy_mod.main()

        out, _ = capsys.readouterr()
        data = json.loads(out.strip())
        assert data["decision"] == "deny"

    def test_agy_search_web_rule(self, tmp_path, monkeypatch, capsys):
        config_file = tmp_path / ".dippy"
        config_file.write_text("allow-web *python*\ndeny-web *exploit*\n")

        payload = {
            "toolCall": {
                "name": "search_web",
                "args": {
                    "query": "python asyncio docs",
                },
            },
            "workspacePaths": [str(tmp_path)],
        }

        monkeypatch.setattr("sys.stdin", StringIO(json.dumps(payload)))
        monkeypatch.setattr("sys.argv", ["dippy", "--agy"])
        monkeypatch.setattr(Path, "home", lambda: tmp_path)

        dippy_mod.main()

        out, _ = capsys.readouterr()
        data = json.loads(out.strip())
        assert data["decision"] == "allow"

    def test_agy_call_mcp_tool_rule(self, tmp_path, monkeypatch, capsys):
        config_file = tmp_path / ".dippy"
        config_file.write_text("allow-mcp mcp__github__get_*\n")

        payload = {
            "toolCall": {
                "name": "call_mcp_tool",
                "args": {
                    "ServerName": "github",
                    "ToolName": "get_issue",
                    "Arguments": {"issue_id": 1},
                },
            },
            "workspacePaths": [str(tmp_path)],
        }

        monkeypatch.setattr("sys.stdin", StringIO(json.dumps(payload)))
        monkeypatch.setattr("sys.argv", ["dippy", "--agy"])
        monkeypatch.setattr(Path, "home", lambda: tmp_path)

        dippy_mod.main()

        out, _ = capsys.readouterr()
        data = json.loads(out.strip())
        assert data["decision"] == "allow"

    def test_agy_stop_hook_idle(self, tmp_path, monkeypatch, capsys):
        config_file = tmp_path / ".dippy"
        config_file.write_text("set notifier-command 'echo note'\n")

        payload = {
            "executionNum": 1,
            "terminationReason": "model_stop",
            "fullyIdle": True,
        }

        monkeypatch.setattr("sys.stdin", StringIO(json.dumps(payload)))
        monkeypatch.setattr("sys.argv", ["dippy", "--agy"])
        monkeypatch.setattr(Path, "home", lambda: tmp_path)

        dippy_mod.main()

        out, _ = capsys.readouterr()
        data = json.loads(out.strip())
        assert data["decision"] == "continue"
        assert "note --idle" in data["reason"]
