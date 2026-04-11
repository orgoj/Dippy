"""Tests for dippy doctor module."""

from __future__ import annotations

import json
from pathlib import Path
from dippy.cli.doctor import (
    CheckResult,
    HealthStatus,
    _format_json_output,
    apply_auto_fixes,
    check_config_validation,
    check_hook_status,
    check_installation,
    check_log_health,
    run,
)


class TestHealthStatus:
    def test_ok_exit_code(self):
        assert HealthStatus.OK.exit_code == 0

    def test_warning_exit_code(self):
        assert HealthStatus.WARNING.exit_code == 1

    def test_critical_exit_code(self):
        assert HealthStatus.CRITICAL.exit_code == 2


class TestCheckInstallation:
    def test_not_found_returns_critical(self, monkeypatch):
        monkeypatch.setattr("shutil.which", lambda x: None)
        result = check_installation()
        assert result.status == HealthStatus.CRITICAL
        assert "not found" in result.message.lower() or "PATH" in result.message

    def test_found_and_working_returns_ok(self, monkeypatch, tmp_path):
        fake_dippy = tmp_path / "dippy"
        fake_dippy.write_text("#!/bin/sh\necho 'dippy 1.0.0'")
        fake_dippy.chmod(0o755)
        monkeypatch.setattr("shutil.which", lambda x: str(fake_dippy))

        import subprocess

        real_run = subprocess.run

        def mock_run(cmd, **kwargs):
            if cmd and "dippy" in str(cmd[0]):

                class FakeResult:
                    returncode = 0
                    stdout = "dippy 1.0.0\n"
                    stderr = ""

                return FakeResult()
            return real_run(cmd, **kwargs)

        monkeypatch.setattr("subprocess.run", mock_run)
        result = check_installation()
        assert result.status == HealthStatus.OK

    def test_found_but_nonzero_exit_returns_warning(self, monkeypatch, tmp_path):
        fake_dippy = tmp_path / "dippy"
        fake_dippy.write_text("#!/bin/sh\nexit 1")
        fake_dippy.chmod(0o755)
        monkeypatch.setattr("shutil.which", lambda x: str(fake_dippy))

        import subprocess

        real_run = subprocess.run

        def mock_run(cmd, **kwargs):
            if cmd and "dippy" in str(cmd[0]):

                class FakeResult:
                    returncode = 1
                    stdout = ""
                    stderr = "error"

                return FakeResult()
            return real_run(cmd, **kwargs)

        monkeypatch.setattr("subprocess.run", mock_run)
        result = check_installation()
        assert result.status == HealthStatus.WARNING


class TestCheckConfigValidation:
    def test_no_config_returns_warning(self, tmp_path, monkeypatch):
        # No config files exist in tmp_path — global config missing triggers warning
        monkeypatch.setattr(Path, "home", lambda: tmp_path)
        import dippy.core.config as _config_mod

        monkeypatch.setattr(_config_mod, "USER_CONFIG", tmp_path / ".dippy" / "config")
        result = check_config_validation(tmp_path)
        assert result.status == HealthStatus.WARNING

    def test_valid_project_config_returns_warning_no_global(
        self, tmp_path, monkeypatch
    ):
        monkeypatch.setattr(Path, "home", lambda: tmp_path)
        import dippy.core.config as _config_mod

        monkeypatch.setattr(_config_mod, "USER_CONFIG", tmp_path / ".dippy" / "config")
        # Valid project config but no global config → warning for missing global
        config_file = tmp_path / ".dippy"
        config_file.write_text("allow zork\n")
        result = check_config_validation(tmp_path)
        # Global config still missing → WARNING, not OK
        assert result.status == HealthStatus.WARNING

    def test_invalid_project_config_returns_critical_on_parse_error(
        self, tmp_path, monkeypatch
    ):
        monkeypatch.setattr(Path, "home", lambda: tmp_path)
        import dippy.core.config as _config_mod

        monkeypatch.setattr(_config_mod, "USER_CONFIG", tmp_path / ".dippy" / "config")
        config_file = tmp_path / ".dippy"
        config_file.write_text("allow zork\n")
        # load_config is imported inside check_config_validation, so patch at source
        from dippy.core.config import ConfigError

        def raise_error(*a, **kw):
            raise ConfigError("parse error")

        monkeypatch.setattr(_config_mod, "load_config", raise_error)
        result = check_config_validation(tmp_path)
        assert result.status == HealthStatus.CRITICAL
        assert isinstance(result, CheckResult)


class TestCheckLogHealth:
    def test_no_log_dirs_returns_ok(self, tmp_path, monkeypatch):
        monkeypatch.setattr(Path, "home", lambda: tmp_path)
        result = check_log_health(verbose=False)
        assert result.status == HealthStatus.OK

    def test_small_log_file_returns_ok(self, tmp_path, monkeypatch):
        monkeypatch.setattr(Path, "home", lambda: tmp_path)
        # Create .claude dir with a small log file
        claude_dir = tmp_path / ".claude"
        claude_dir.mkdir()
        log_file = claude_dir / "hook-approvals.log"
        log_file.write_text("small log\n")
        result = check_log_health(verbose=False)
        assert result.status == HealthStatus.OK

    def test_large_log_file_returns_warning(self, tmp_path, monkeypatch):
        monkeypatch.setattr(Path, "home", lambda: tmp_path)
        claude_dir = tmp_path / ".claude"
        claude_dir.mkdir()
        log_file = claude_dir / "hook-approvals.log"
        # Write >10MB
        log_file.write_bytes(b"x" * (11 * 1024 * 1024))
        result = check_log_health(verbose=False)
        assert result.status == HealthStatus.WARNING


class TestFormatJsonOutput:
    def test_returns_valid_json(self):
        checks = [
            CheckResult("Test", HealthStatus.OK, "All good"),
        ]
        summary = {"ok": 1, "warnings": 0, "critical": 0}
        output = _format_json_output(checks, summary)
        parsed = json.loads(output)
        assert "summary" in parsed
        assert "checks" in parsed

    def test_overall_status_ok_when_no_issues(self):
        checks = [CheckResult("Test", HealthStatus.OK, "Fine")]
        summary = {"ok": 1, "warnings": 0, "critical": 0}
        output = _format_json_output(checks, summary)
        parsed = json.loads(output)
        assert parsed["overall_status"] == "ok"

    def test_overall_status_warning_when_warnings(self):
        checks = [CheckResult("Test", HealthStatus.WARNING, "Some warning")]
        summary = {"ok": 0, "warnings": 1, "critical": 0}
        output = _format_json_output(checks, summary)
        parsed = json.loads(output)
        assert parsed["overall_status"] == "warning"

    def test_overall_status_critical_when_critical(self):
        checks = [CheckResult("Test", HealthStatus.CRITICAL, "Critical issue")]
        summary = {"ok": 0, "warnings": 0, "critical": 1}
        output = _format_json_output(checks, summary)
        parsed = json.loads(output)
        assert parsed["overall_status"] == "critical"

    def test_checks_list_contains_all_checks(self):
        checks = [
            CheckResult("A", HealthStatus.OK, "ok"),
            CheckResult("B", HealthStatus.WARNING, "warn"),
        ]
        summary = {"ok": 1, "warnings": 1, "critical": 0}
        output = _format_json_output(checks, summary)
        parsed = json.loads(output)
        assert len(parsed["checks"]) == 2


class TestApplyAutoFixes:
    def test_no_fixable_checks_returns_false(self):
        checks = [CheckResult("Test", HealthStatus.OK, "Fine")]
        result = apply_auto_fixes(checks, Path("/tmp"))
        assert result is False

    def test_critical_checks_not_auto_fixed(self):
        # apply_auto_fixes only handles WARNING with fix_command
        checks = [
            CheckResult(
                "Installation",
                HealthStatus.CRITICAL,
                "not found",
                fix_command="dippy hooks install claude --global",
            )
        ]
        result = apply_auto_fixes(checks, Path("/tmp"))
        assert result is False

    def test_warning_with_hooks_install_calls_install(self, tmp_path, monkeypatch):
        checks = [
            CheckResult(
                "Hook: Claude Code",
                HealthStatus.WARNING,
                "Not installed",
                fix_command="dippy hooks install claude",
            )
        ]
        called_args = []

        def fake_install(agent, global_config, cwd, force, dry_run):
            called_args.append(agent)
            return 0

        # apply_auto_fixes does `from dippy.cli.hooks import install as hooks_install`
        # so patching the module attribute works
        import dippy.cli.hooks as _hooks_mod

        monkeypatch.setattr(_hooks_mod, "install", fake_install)
        result = apply_auto_fixes(checks, tmp_path)
        assert result is True
        assert called_args == ["claude"]


class TestGeminiErrorBranches:
    """Tests for Gemini-specific error/fallback paths in doctor checks."""

    def test_check_hook_status_gemini_invalid_json_no_crash(
        self, tmp_path, monkeypatch
    ):
        """Gemini config with invalid JSON does not crash check_hook_status."""
        from dippy.cli.doctor import check_hook_status

        monkeypatch.setattr(Path, "home", lambda: tmp_path)
        # Create invalid Gemini config
        gemini_dir = tmp_path / ".gemini"
        gemini_dir.mkdir()
        (gemini_dir / "settings.json").write_text("{invalid json!!!")
        results = check_hook_status(tmp_path, verbose=False)
        # Must return a list without raising
        assert isinstance(results, list)
        assert len(results) > 0

    def test_check_hook_status_gemini_corrupted_json_returns_warning_not_critical(
        self, tmp_path, monkeypatch
    ):
        """Corrupted Gemini config yields WARNING (not installed) not CRITICAL."""
        from dippy.cli.doctor import check_hook_status

        monkeypatch.setattr(Path, "home", lambda: tmp_path)
        gemini_dir = tmp_path / ".gemini"
        gemini_dir.mkdir()
        (gemini_dir / "settings.json").write_text("not json at all")
        results = check_hook_status(tmp_path, verbose=False)
        gemini_result = next((r for r in results if "Gemini" in r.name), None)
        # Gemini hook not installed (can't parse config = treat as not configured)
        if gemini_result:
            assert gemini_result.status != HealthStatus.CRITICAL

    def test_check_hook_status_gemini_empty_config_no_crash(
        self, tmp_path, monkeypatch
    ):
        """Empty Gemini settings.json does not crash."""
        from dippy.cli.doctor import check_hook_status

        monkeypatch.setattr(Path, "home", lambda: tmp_path)
        gemini_dir = tmp_path / ".gemini"
        gemini_dir.mkdir()
        (gemini_dir / "settings.json").write_text("{}")
        results = check_hook_status(tmp_path, verbose=False)
        assert isinstance(results, list)


class TestCodexDoctor:
    def test_check_hook_status_includes_codex_when_feature_flag_exists(
        self, tmp_path, monkeypatch
    ):
        monkeypatch.setattr(Path, "home", lambda: tmp_path)
        monkeypatch.setenv("HOME", str(tmp_path))
        codex_dir = tmp_path / ".codex"
        codex_dir.mkdir()
        (codex_dir / "config.toml").write_text("[features]\ncodex_hooks = true\n")
        workspace = tmp_path / "workspace"
        workspace.mkdir()

        results = check_hook_status(workspace, verbose=False)

        codex_result = next((r for r in results if r.name == "Hook: OpenAI Codex CLI"), None)
        assert codex_result is not None
        assert codex_result.status == HealthStatus.WARNING
        assert "hook not installed" in codex_result.message.lower()

    def test_check_hook_status_warns_when_codex_feature_flag_missing(
        self, tmp_path, monkeypatch
    ):
        monkeypatch.setattr(Path, "home", lambda: tmp_path)
        monkeypatch.setenv("HOME", str(tmp_path))
        workspace = tmp_path / "workspace"
        workspace.mkdir()
        codex_dir = workspace / ".codex"
        codex_dir.mkdir()
        (codex_dir / "hooks.json").write_text(
            json.dumps(
                {
                    "hooks": {
                        "PreToolUse": [
                            {
                                "matcher": "^Bash$",
                                "hooks": [
                                    {"type": "command", "command": "dippy --codex"}
                                ],
                            }
                        ]
                    }
                }
            )
        )

        results = check_hook_status(workspace, verbose=False)

        codex_result = next((r for r in results if r.name == "Hook: OpenAI Codex CLI"), None)
        assert codex_result is not None
        assert codex_result.status == HealthStatus.WARNING
        assert "feature flag missing" in codex_result.message.lower()

class TestRun:
    def test_returns_int(self, tmp_path, monkeypatch, capsys):
        monkeypatch.setattr("shutil.which", lambda x: None)
        monkeypatch.setattr(Path, "home", lambda: tmp_path)
        result = run(cwd=str(tmp_path), quiet=True)
        assert isinstance(result, int)

    def test_returns_nonzero_when_not_installed(self, tmp_path, monkeypatch):
        monkeypatch.setattr("shutil.which", lambda x: None)
        monkeypatch.setattr(Path, "home", lambda: tmp_path)
        result = run(cwd=str(tmp_path), quiet=True)
        assert result > 0

    def test_json_output_parseable(self, tmp_path, monkeypatch, capsys):
        monkeypatch.setattr("shutil.which", lambda x: None)
        monkeypatch.setattr(Path, "home", lambda: tmp_path)
        run(cwd=str(tmp_path), json_output=True)
        captured = capsys.readouterr()
        parsed = json.loads(captured.out)
        assert "summary" in parsed
        assert "checks" in parsed
        assert "overall_status" in parsed
