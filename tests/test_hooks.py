"""Tests for dippy hooks installation functionality."""

import json

from dippy.cli.hooks import _is_dippy_hook, _remove_dippy_hook


class TestIsDippyHook:
    """Tests for _is_dippy_hook() helper function."""

    def test_new_style_hook_with_space(self):
        """New style: 'dippy --claude' starts with 'dippy '."""
        hook = {"command": "dippy --claude"}
        assert _is_dippy_hook(hook) is True

    def test_new_style_hook_gemini(self):
        """New style: 'dippy --gemini'."""
        hook = {"command": "dippy --gemini"}
        assert _is_dippy_hook(hook) is True

    def test_new_style_hook_cursor(self):
        """New style: 'dippy --cursor'."""
        hook = {"command": "dippy --cursor"}
        assert _is_dippy_hook(hook) is True

    def test_new_style_hook_windsurf(self):
        """New style: 'dippy --windsurf'."""
        hook = {"command": "dippy --windsurf"}
        assert _is_dippy_hook(hook) is True

    def test_bare_dippy_command(self):
        """Bare 'dippy' command."""
        hook = {"command": "dippy"}
        assert _is_dippy_hook(hook) is True

    def test_legacy_hook_full_path_ends_with_dippy_hook(self):
        """Legacy: /path/to/dippy-hook ends with 'dippy-hook'."""
        hook = {"command": "/home/michael/.local/bin/dippy-hook"}
        assert _is_dippy_hook(hook) is True

    def test_legacy_hook_full_path_ends_with_dippy(self):
        """Legacy: /path/to/dippy ends with '/dippy'."""
        hook = {"command": "/home/michael/work/ai/CLAUDE/TOOLS/dippy-dev/dippy"}
        assert _is_dippy_hook(hook) is True

    def test_legacy_hook_with_dippy_in_path(self):
        """Legacy: /path/to/dippy-hook contains '/dippy'."""
        hook = {"command": "/home/user/dippy-scripts/dippy-hook.sh"}
        assert _is_dippy_hook(hook) is True

    def test_windows_style_path(self):
        """Windows path: C:\\Users\\...\\dippy-hook contains '\\dippy'."""
        hook = {"command": "C:\\Users\\user\\dippy-hook"}
        assert _is_dippy_hook(hook) is True

    def test_not_a_match_dippy_in_middle(self):
        """NOT a match: random command with 'dippy' in middle."""
        hook = {"command": "/home/user/adippy-workspace/script.sh"}
        assert _is_dippy_hook(hook) is False

    def test_not_a_match_adippy_command(self):
        """NOT a match: 'adippy' is not 'dippy'."""
        hook = {"command": "adippy"}
        assert _is_dippy_hook(hook) is False

    def test_not_a_match_dippyxx_command(self):
        """NOT a match: 'dippyxx' is not 'dippy'."""
        hook = {"command": "dippyxx"}
        assert _is_dippy_hook(hook) is False

    def test_not_a_match_memorix_hook(self):
        """NOT a match: memorix hook should not be detected as dippy."""
        hook = {"command": "memorix hook", "type": "command"}
        assert _is_dippy_hook(hook) is False

    def test_not_a_match_random_command(self):
        """NOT a match: random command."""
        hook = {"command": "ls -la"}
        assert _is_dippy_hook(hook) is False

    def test_empty_command(self):
        """Empty command."""
        hook = {"command": ""}
        assert _is_dippy_hook(hook) is False

    def test_no_command_key(self):
        """Hook dict without 'command' key."""
        hook = {"type": "command"}
        assert _is_dippy_hook(hook) is False

    def test_not_a_dict(self):
        """Hook is not a dict."""
        assert _is_dippy_hook("not a dict") is False
        assert _is_dippy_hook(None) is False
        assert _is_dippy_hook(123) is False


class TestRemoveDippyHook:
    """Tests for _remove_dippy_hook() function."""

    def test_removes_new_style_hook_from_pretooluse(self):
        """Remove 'dippy --claude' from PreToolUse."""
        config = {
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash|Write|Edit",
                        "hooks": [{"type": "command", "command": "dippy --claude"}],
                    }
                ]
            }
        }
        result = _remove_dippy_hook(config, "claude")
        assert "hooks" not in result or not result["hooks"].get("PreToolUse")

    def test_removes_legacy_hook_from_pretooluse(self):
        """Remove '/path/to/dippy-hook' from PreToolUse."""
        config = {
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash|Write|Edit",
                        "hooks": [
                            {"type": "command", "command": "/home/user/dippy-hook"}
                        ],
                    }
                ]
            }
        }
        result = _remove_dippy_hook(config, "claude")
        assert "hooks" not in result or not result["hooks"].get("PreToolUse")

    def test_preserves_non_dippy_hooks(self):
        """Preserve memorix and other non-Dippy hooks."""
        config = {
            "hooks": {
                "PostToolUse": [
                    {"hooks": [{"command": "memorix hook", "type": "command"}]},
                    {
                        "matcher": "Bash|WebSearch",
                        "hooks": [{"command": "dippy --claude"}],
                    },
                ]
            }
        }
        result = _remove_dippy_hook(config, "claude")
        # Should preserve memorix hook
        assert len(result["hooks"]["PostToolUse"]) == 1
        assert (
            result["hooks"]["PostToolUse"][0]["hooks"][0]["command"] == "memorix hook"
        )

    def test_removes_from_all_hook_types_claude(self):
        """Remove Dippy hooks from ALL Claude hook types."""
        config = {
            "hooks": {
                "PreToolUse": [{"hooks": [{"command": "dippy --claude"}]}],
                "PostToolUse": [
                    {"hooks": [{"command": "memorix hook"}]},
                    {"hooks": [{"command": "dippy --claude"}]},
                ],
                "Notification": [{"hooks": [{"command": "dippy --claude"}]}],
                "Stop": [{"hooks": [{"command": "dippy --claude"}]}],
                "SubagentStop": [{"hooks": [{"command": "dippy --claude"}]}],
                "AfterAgent": [{"hooks": [{"command": "dippy --claude"}]}],
            }
        }
        result = _remove_dippy_hook(config, "claude")
        # All dippy hooks should be removed
        for hook_type in [
            "PreToolUse",
            "PostToolUse",
            "Notification",
            "Stop",
            "SubagentStop",
            "AfterAgent",
        ]:
            if hook_type in result["hooks"]:
                for entry in result["hooks"][hook_type]:
                    for hook in entry.get("hooks", []):
                        assert not _is_dippy_hook(hook)

    def test_removes_legacy_path_with_dippy_in_directory(self):
        """Remove hook from path like /home/user/dippy-scripts/hook."""
        config = {
            "hooks": {
                "PreToolUse": [
                    {"hooks": [{"command": "/home/user/dippy-scripts/hook"}]}
                ]
            }
        }
        result = _remove_dippy_hook(config, "claude")
        assert "hooks" not in result or not result["hooks"].get("PreToolUse")

    def test_does_not_remove_user_dippy_named_directory(self):
        """Do NOT remove /home/user/my-dippy-scripts/hook (user's custom directory)."""
        config = {
            "hooks": {
                "PreToolUse": [
                    {"hooks": [{"command": "/home/user/my-dippy-scripts/hook"}]}
                ]
            }
        }
        result = _remove_dippy_hook(config, "claude")
        # Should NOT remove this (dippy is part of dir name, not /dippy path component)
        assert "hooks" in result
        assert len(result["hooks"]["PreToolUse"]) == 1

    def test_does_not_remove_adippy_workspace_script(self):
        """Do NOT remove /home/user/adippy-workspace/script.sh."""
        config = {
            "hooks": {
                "PreToolUse": [
                    {"hooks": [{"command": "/home/user/adippy-workspace/script.sh"}]}
                ]
            }
        }
        result = _remove_dippy_hook(config, "claude")
        # Should NOT remove this (it's not a dippy hook)
        assert "hooks" in result
        assert len(result["hooks"]["PreToolUse"]) == 1

    def test_cursor_format_hooks(self):
        """Remove Dippy hooks from Cursor format."""
        config = {
            "version": 1,
            "hooks": {
                "beforeShellExecution": [{"command": "dippy --cursor"}],
                "afterShellExecution": [
                    {"command": "dippy --cursor"},
                    {"command": "other-hook"},
                ],
            },
        }
        result = _remove_dippy_hook(config, "cursor")
        # Dippy hooks removed, other-hook preserved
        # beforeShellExecution should be removed (empty after cleanup)
        assert "beforeShellExecution" not in result["hooks"]
        assert len(result["hooks"]["afterShellExecution"]) == 1
        assert result["hooks"]["afterShellExecution"][0]["command"] == "other-hook"

    def test_empty_config(self):
        """Handle empty config."""
        config = {}
        result = _remove_dippy_hook(config, "claude")
        assert result == {}

    def test_no_hooks_key(self):
        """Handle config without hooks key."""
        config = {"other": "value"}
        result = _remove_dippy_hook(config, "claude")
        assert "other" in result

    def test_removes_multiple_dippy_hooks_same_type(self):
        """Remove multiple Dippy hooks from the same hook type."""
        config = {
            "hooks": {
                "PostToolUse": [
                    {"hooks": [{"command": "dippy --claude"}]},
                    {"hooks": [{"command": "memorix hook"}]},
                    {"hooks": [{"command": "/path/to/dippy-hook"}]},
                ]
            }
        }
        result = _remove_dippy_hook(config, "claude")
        # Both dippy hooks removed, memorix preserved
        assert len(result["hooks"]["PostToolUse"]) == 1
        assert (
            result["hooks"]["PostToolUse"][0]["hooks"][0]["command"] == "memorix hook"
        )

    def test_cleans_up_empty_hooks_dict(self):
        """Remove empty hooks dict after cleanup."""
        config = {
            "hooks": {"PreToolUse": [{"hooks": [{"command": "dippy --claude"}]}]},
            "other": "value",
        }
        result = _remove_dippy_hook(config, "claude")
        # hooks dict should be removed if empty
        assert "hooks" not in result
        assert result["other"] == "value"


class TestHooksInstallUninstall:
    """Integration-level tests for install/uninstall flows."""

    def test_install_claude_creates_hooks_config(self, tmp_path):
        from dippy.cli.hooks import install

        result = install(agent="claude", global_config=False, cwd=str(tmp_path))
        assert result == 0
        config_path = tmp_path / ".claude" / "settings.json"
        assert config_path.exists()
        config = json.loads(config_path.read_text())
        assert "hooks" in config
        assert "PreToolUse" in config["hooks"]

    def test_install_gemini_creates_hooks_config(self, tmp_path):
        from dippy.cli.hooks import install

        result = install(agent="gemini", global_config=False, cwd=str(tmp_path))
        assert result == 0
        config_path = tmp_path / ".gemini" / "settings.json"
        assert config_path.exists()

    def test_install_idempotent(self, tmp_path):
        from dippy.cli.hooks import install

        install(agent="claude", global_config=False, cwd=str(tmp_path))
        result = install(agent="claude", global_config=False, cwd=str(tmp_path))
        assert result == 0

    def test_install_then_uninstall_removes_hooks(self, tmp_path):
        from dippy.cli.hooks import _has_dippy_hook, install, uninstall

        install(agent="claude", global_config=False, cwd=str(tmp_path))
        result = uninstall(agent="claude", global_config=False, cwd=str(tmp_path))
        assert result == 0
        config_path = tmp_path / ".claude" / "settings.json"
        if config_path.exists():
            config = json.loads(config_path.read_text())
            assert not _has_dippy_hook(config, "claude")

    def test_install_dry_run_does_not_create_file(self, tmp_path):
        from dippy.cli.hooks import install

        result = install(
            agent="claude", global_config=False, cwd=str(tmp_path), dry_run=True
        )
        assert result == 0
        config_path = tmp_path / ".claude" / "settings.json"
        assert not config_path.exists()

    def test_install_codex_creates_hooks_json_with_correct_format(self, tmp_path):
        from dippy.cli.hooks import install

        result = install(agent="codex", global_config=False, cwd=str(tmp_path))
        assert result == 0
        config_path = tmp_path / ".codex" / "hooks.json"
        assert config_path.exists()
        config = json.loads(config_path.read_text())
        assert "hooks" in config
        assert "PreToolUse" in config["hooks"]
