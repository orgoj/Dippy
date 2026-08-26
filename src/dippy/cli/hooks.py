"""
Dippy hooks management - install and uninstall Dippy hooks for AI coding assistants.

Provides functions to add/remove Dippy hook configuration from agent
settings files while preserving existing hooks and proper JSON formatting.
"""

from __future__ import annotations

import copy
import json
import re
import shutil
import sys
from dataclasses import dataclass, field
from datetime import datetime
from enum import Enum
from pathlib import Path

from dippy.cli.agents import AGENTS


# Hook entry point for different agents
# Minimal hooks: PreToolUse/BeforeTool + PostToolUse/AfterTool (default)
# Full hooks: Adds Notification, Stop, SubagentStop, AfterAgent (with --all flag)

MINIMAL_HOOKS = {
    "claude": {
        "hooks": {
            "PreToolUse": [
                {
                    "matcher": "Bash|Write|Edit|MultiEdit|Read|LS|Glob|Grep|Search|WebSearch|mcp__.*",
                    "hooks": [
                        {
                            "type": "command",
                            "command": "dippy --claude",
                        }
                    ],
                }
            ],
            "PostToolUse": [
                {
                    "matcher": "Bash|WebSearch|mcp__.*",
                    "hooks": [
                        {
                            "type": "command",
                            "command": "dippy --claude",
                        }
                    ],
                }
            ],
        }
    },
    "gemini": {
        "hooks": {
            "BeforeTool": [
                {
                    "matcher": "run_shell_command|write_file|replace|read_file|google_web_search",
                    "hooks": [
                        {
                            "name": "dippy-approval",
                            "type": "command",
                            "command": "dippy --gemini",
                        }
                    ],
                }
            ],
            "AfterTool": [
                {
                    "matcher": "run_shell_command|google_web_search",
                    "hooks": [
                        {
                            "name": "dippy-after",
                            "type": "command",
                            "command": "dippy --gemini",
                        }
                    ],
                }
            ],
        }
    },
    "agy": {
        "hooks": {
            "PreToolUse": [
                {
                    "matcher": "run_command|write_to_file|replace_file_content|view_file|search_web|call_mcp_tool",
                    "hooks": [
                        {
                            "name": "dippy-approval",
                            "type": "command",
                            "command": "dippy --agy",
                            "timeout": 1800,
                        }
                    ],
                }
            ],
            "PostToolUse": [
                {
                    "matcher": "run_command|search_web|call_mcp_tool",
                    "hooks": [
                        {
                            "name": "dippy-after",
                            "type": "command",
                            "command": "dippy --agy",
                        }
                    ],
                }
            ],
        }
    },
    "codex": {
        "hooks": {
            "PreToolUse": [
                {
                    "matcher": "^Bash$",
                    "hooks": [
                        {
                            "type": "command",
                            "command": "dippy --codex",
                        }
                    ],
                }
            ],
            "PermissionRequest": [
                {
                    "matcher": "^Bash$",
                    "hooks": [
                        {
                            "type": "command",
                            "command": "dippy --codex",
                        }
                    ],
                }
            ],
            "PostToolUse": [
                {
                    "matcher": "^Bash$",
                    "hooks": [
                        {
                            "type": "command",
                            "command": "dippy --codex",
                        }
                    ],
                }
            ],
        }
    },
}

ALL_HOOKS = {
    "claude": {
        "hooks": {
            "PreToolUse": [
                {
                    "matcher": "Bash|Write|Edit|MultiEdit|Read|LS|Glob|Grep|Search|WebSearch|mcp__.*",
                    "hooks": [
                        {
                            "type": "command",
                            "command": "dippy --claude",
                        }
                    ],
                }
            ],
            "PostToolUse": [
                {
                    "matcher": "Bash|WebSearch|mcp__.*",
                    "hooks": [
                        {
                            "type": "command",
                            "command": "dippy --claude",
                        }
                    ],
                }
            ],
            "Notification": [
                {
                    "matcher": "notification_type==idle_prompt",
                    "hooks": [
                        {
                            "type": "command",
                            "command": "dippy --claude",
                        }
                    ],
                }
            ],
            "Stop": [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": "dippy --claude",
                        }
                    ],
                }
            ],
            "SubagentStop": [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": "dippy --claude",
                        }
                    ],
                }
            ],
            "AfterAgent": [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": "dippy --claude",
                        }
                    ],
                }
            ],
        }
    },
    "gemini": {
        "hooks": {
            "BeforeTool": [
                {
                    "matcher": "run_shell_command|write_file|replace|read_file|google_web_search",
                    "hooks": [
                        {
                            "name": "dippy-approval",
                            "type": "command",
                            "command": "dippy --gemini",
                        }
                    ],
                }
            ],
            "AfterTool": [
                {
                    "matcher": "run_shell_command|google_web_search",
                    "hooks": [
                        {
                            "name": "dippy-after",
                            "type": "command",
                            "command": "dippy --gemini",
                        }
                    ],
                }
            ],
        }
    },
    "agy": {
        "hooks": {
            "PreToolUse": [
                {
                    "matcher": "run_command|write_to_file|replace_file_content|view_file|search_web|call_mcp_tool",
                    "hooks": [
                        {
                            "name": "dippy-approval",
                            "type": "command",
                            "command": "dippy --agy",
                            "timeout": 1800,
                        }
                    ],
                }
            ],
            "PostToolUse": [
                {
                    "matcher": "run_command|search_web|call_mcp_tool",
                    "hooks": [
                        {
                            "name": "dippy-after",
                            "type": "command",
                            "command": "dippy --agy",
                        }
                    ],
                }
            ],
            "Stop": [
                {
                    "hooks": [
                        {
                            "name": "dippy-stop",
                            "type": "command",
                            "command": "dippy --agy",
                        }
                    ],
                }
            ],
        }
    },
    "codex": {
        "hooks": {
            "PreToolUse": [
                {
                    "matcher": "^Bash$",
                    "hooks": [
                        {
                            "type": "command",
                            "command": "dippy --codex",
                        }
                    ],
                }
            ],
            "PermissionRequest": [
                {
                    "matcher": "^Bash$",
                    "hooks": [
                        {
                            "type": "command",
                            "command": "dippy --codex",
                        }
                    ],
                }
            ],
            "PostToolUse": [
                {
                    "matcher": "^Bash$",
                    "hooks": [
                        {
                            "type": "command",
                            "command": "dippy --codex",
                        }
                    ],
                }
            ],
            "Stop": [
                {
                    "hooks": [
                        {
                            "type": "command",
                            "command": "dippy --codex",
                        }
                    ],
                }
            ],
        }
    },
}

# Legacy HOOK_COMMANDS for backward compatibility (Cursor/Windsurf use this format)
HOOK_COMMANDS = {
    "claude": {
        "config": "~/.claude/settings.json",
        "project_config": ".claude/settings.json",
        "hook_entry": MINIMAL_HOOKS["claude"],
    },
    "gemini": {
        "config": "~/.gemini/settings.json",
        "project_config": ".gemini/settings.json",
        "hook_entry": MINIMAL_HOOKS["gemini"],
    },
    "agy": {
        "config": "~/.gemini/config/hooks.json",
        "project_config": ".agents/hooks.json",
        "hook_entry": MINIMAL_HOOKS["agy"],
    },
    "cursor": {
        "config": "~/.cursor/hooks.json",
        "project_config": ".cursor/hooks.json",
        "hook_entry": {
            "version": 1,
            "hooks": {
                # preToolUse, not beforeShellExecution: the latter ignores an
                # "allow" answer and prompts anyway. The matcher keeps Dippy
                # out of every non-shell tool call.
                "preToolUse": [{"matcher": "Shell", "command": "dippy --cursor"}],
                "afterShellExecution": [{"command": "dippy --cursor"}],
            },
        },
    },
    "windsurf": {
        "config": "~/.windsurf/hooks.json",
        "project_config": ".windsurf/hooks.json",
        "hook_entry": {
            "version": 1,
            "hooks": {
                "beforeShellExecution": [{"command": "dippy --windsurf"}],
                "afterShellExecution": [{"command": "dippy --windsurf"}],
            },
        },
    },
    "codex": {
        "config": "~/.codex/hooks.json",
        "project_config": ".codex/hooks.json",
        "hook_entry": MINIMAL_HOOKS["codex"],
    },
}


# Maximum number of backups to keep
MAX_BACKUPS = 5


def _ensure_codex_feature_flag(
    config_path: Path,
    dry_run: bool = False,
) -> tuple[bool, str]:
    """Ensure hooks = true is set in Codex config.toml.

    Codex requires a feature flag in config.toml alongside hooks.json.
    Current Codex uses `hooks`; older versions accepted the legacy alias
    `codex_hooks`.
    This handles creating/modifying the TOML file with simple text
    manipulation (no TOML writer dependency needed).

    Args:
        config_path: Path to config.toml (e.g. ~/.codex/config.toml)
        dry_run: If True, only report what would change

    Returns:
        Tuple of (was_modified, message)
    """
    if config_path.exists():
        content = config_path.read_text()
    else:
        content = ""

    # Check if feature flag already exists. Keep the legacy alias working for
    # older configs, but write the current canonical key on new installs.
    if _codex_feature_flag_enabled(config_path):
        return False, "Feature flag already enabled"

    has_any_hook_flag = re.search(r"(?m)^\s*(?:hooks|codex_hooks)\s*=", content)

    if dry_run:
        if has_any_hook_flag:
            return True, f"Would update hooks feature flag in {config_path}"
        return True, f"Would add [features] hooks = true to {config_path}"

    # Build the new content
    if not content.strip():
        # Empty or non-existent file
        new_content = "[features]\nhooks = true\n"
    elif re.search(r"(?m)^\s*hooks\s*=", content):
        new_content = re.sub(
            r"(?m)^(\s*)hooks\s*=\s*(?:true|false)(\s*(?:#.*)?)$",
            r"\1hooks = true\2",
            content,
            count=1,
        )
    elif re.search(r"(?m)^\s*codex_hooks\s*=", content):
        new_content = re.sub(
            r"(?m)^(\s*)codex_hooks\s*=\s*(?:true|false)(\s*(?:#.*)?)$",
            r"\1hooks = true\2",
            content,
            count=1,
        )
    elif "[features]" in content:
        # Features section exists, add hooks to it
        lines = content.split("\n")
        new_lines = []
        inserted = False
        for line in lines:
            new_lines.append(line)
            if line.strip() == "[features]" and not inserted:
                new_lines.append("hooks = true")
                inserted = True
        if not inserted:
            # [features] might have been on a line with other content
            new_lines.append("hooks = true")
        new_content = "\n".join(new_lines)
        if not new_content.endswith("\n"):
            new_content += "\n"
    else:
        # No [features] section, append it
        new_content = content.rstrip("\n") + "\n\n[features]\nhooks = true\n"

    # Backup config.toml if it exists
    if config_path.exists():
        _create_backup(config_path)

    config_path.parent.mkdir(parents=True, exist_ok=True)
    config_path.write_text(new_content)
    return True, f"Enabled hooks feature flag in {config_path}"


class HookStatus(Enum):
    """Hook installation status."""

    INSTALLED = "installed"
    LEGACY = "legacy"
    NOT_INSTALLED = "not_installed"
    NO_CONFIG = "no_config"
    ERROR = "error"


@dataclass
class HookInfo:
    """Information about a hook's status."""

    agent_id: str
    agent_name: str
    global_status: HookStatus
    global_path: Path
    global_matchers: list[str] = field(default_factory=list)
    global_command: str = ""
    global_legacy_command: str = ""
    project_status: HookStatus = HookStatus.NOT_INSTALLED
    project_path: Path | None = None
    project_matchers: list[str] = field(default_factory=list)
    project_command: str = ""
    project_legacy_command: str = ""
    global_feature_flag: bool | None = None
    global_feature_flag_path: Path | None = None
    project_feature_flag: bool | None = None
    project_feature_flag_path: Path | None = None
    pi_extension: bool = False

    def has_any_hook(self) -> bool:
        """Check if any hook is installed (global or project)."""
        return self.global_status in (HookStatus.INSTALLED, HookStatus.LEGACY) or (
            self.project_status in (HookStatus.INSTALLED, HookStatus.LEGACY)
        )

    def has_legacy(self) -> bool:
        """Check if any legacy hook exists."""
        return self.global_status == HookStatus.LEGACY or (
            self.project_status == HookStatus.LEGACY
        )


def _get_matchers_for_agent(agent: str, hook_type: str | None = None) -> list[str]:
    """Get matcher patterns for an agent's hook configuration.

    Args:
        agent: Agent ID (claude, gemini, cursor, windsurf)
        hook_type: Optional hook type filter (e.g., "PreToolUse", "beforeShellExecution")

    Returns:
        List of matcher patterns
    """
    hook_config = HOOK_COMMANDS.get(agent)
    if not hook_config:
        return []

    matchers = []
    hooks_data = hook_config["hook_entry"].get("hooks", {})

    for hook_name, hook_list in hooks_data.items():
        if hook_type and hook_name != hook_type:
            continue

        for hook_entry in hook_list:
            if "matcher" in hook_entry:
                matchers.append(hook_entry["matcher"])

    return matchers


def _get_hook_command_for_agent(agent: str) -> str:
    """Get the hook command for an agent.

    Args:
        agent: Agent ID (claude, gemini, cursor, windsurf)

    Returns:
        The hook command (e.g., "dippy --claude")
    """
    hook_config = HOOK_COMMANDS.get(agent)
    if not hook_config:
        return "dippy"

    hooks_data = hook_config["hook_entry"].get("hooks", {})

    # Check different hook formats
    for hook_list in hooks_data.values():
        for hook_entry in hook_list:
            if "command" in hook_entry:
                return hook_entry["command"]
            if "hooks" in hook_entry:
                for sub_hook in hook_entry["hooks"]:
                    if "command" in sub_hook:
                        return sub_hook["command"]
            if "run" in hook_entry and isinstance(hook_entry["run"], list):
                return " ".join(str(r) for r in hook_entry["run"])

    return f"dippy --{agent}"


def _detect_legacy_hook_command(config: dict) -> str | None:
    """Detect legacy dippy-hook command in configuration.

    Args:
        config: Parsed configuration dict

    Returns:
        The legacy command if found, None otherwise
    """
    config_str = json.dumps(config)

    # Look for various legacy patterns
    patterns = [
        r'"command":\s*"/[^"]*dippy[^"]*"',  # Full path to dippy
        r'"command":\s*"[^"]*dippy-hook[^"]*"',  # dippy-hook command
    ]

    for pattern in patterns:
        match = re.search(pattern, config_str)
        if match:
            # Extract the command value
            cmd_match = re.search(r'"command":\s*"([^"]+)"', match.group(0))
            if cmd_match:
                return cmd_match.group(1)

    return None


def _create_backup(config_path: Path) -> Path | None:
    """Create a backup of the config file.

    Args:
        config_path: Path to the config file

    Returns:
        Path to the backup file, or None if backup failed
    """
    if not config_path.exists():
        return None

    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    backup_path = config_path.parent / f"{config_path.name}.dippy-backup-{timestamp}"

    try:
        shutil.copy2(config_path, backup_path)
        _cleanup_old_backups(config_path)
        return backup_path
    except (IOError, OSError) as e:
        print(f"Warning: Could not create backup: {e}", file=sys.stderr)
        return None


def _cleanup_old_backups(config_path: Path) -> None:
    """Clean up old backups, keeping only the most recent MAX_BACKUPS.

    Args:
        config_path: Path to the config file (used to find backups)
    """
    backup_pattern = f"{config_path.name}.dippy-backup-*"
    backups = sorted(
        config_path.parent.glob(backup_pattern),
        key=lambda p: p.stat().st_mtime,
        reverse=True,
    )

    # Remove old backups beyond MAX_BACKUPS
    for old_backup in backups[MAX_BACKUPS:]:
        try:
            old_backup.unlink()
        except OSError:
            pass  # Ignore cleanup failures


def _diff_configs(old_config: dict, new_config: dict, config_path: Path) -> str:
    """Generate a unified diff between old and new config.

    Args:
        old_config: Original configuration
        new_config: New configuration
        config_path: Path to config file (for display)

    Returns:
        Unified diff string
    """
    import difflib

    old_json = json.dumps(old_config, indent=2, sort_keys=True).splitlines(
        keepends=True
    )
    new_json = json.dumps(new_config, indent=2, sort_keys=True).splitlines(
        keepends=True
    )

    diff = difflib.unified_diff(
        old_json,
        new_json,
        fromfile=f"a/{config_path}",
        tofile=f"b/{config_path}",
        lineterm="",
    )

    return "".join(diff)


def install(
    agent: str | None,
    global_config: bool = False,
    cwd: str | None = None,
    force: bool = False,
    dry_run: bool = False,
    no_backup: bool = False,
    all_hooks: bool = False,
) -> int:
    """Install Dippy hooks for the specified agent.

    Args:
        agent: Agent ID (claude, gemini, cursor, windsurf, codex). If omitted,
               --all must be set and hooks will be installed for every agent.
        global_config: Install to global config (default: project-local)
        cwd: Current working directory (for project-local installs)
        force: Replace existing/legacy hooks
        dry_run: Show what would be done without making changes
        no_backup: Skip config backup before install
        all_hooks: If True, install ALL supported hooks (PreToolUse, PostToolUse,
                   Notification, Stop, etc.). If False, install minimal set only.

    Returns:
        Exit code: 0 for success, 1 for errors
    """
    if agent is None:
        if not all_hooks:
            print(
                "Error: agent is required unless --all is specified",
                file=sys.stderr,
            )
            return 1

        exit_code = 0
        for agent_id in HOOK_COMMANDS:
            result = install(
                agent=agent_id,
                global_config=global_config,
                cwd=cwd,
                force=force,
                dry_run=dry_run,
                no_backup=no_backup,
                all_hooks=True,
            )
            if result != 0:
                exit_code = result
        return exit_code

    agent_info = AGENTS.get(agent)
    if not agent_info:
        print(f"Error: Unknown agent '{agent}'", file=sys.stderr)
        print(f"Valid agents: {', '.join(AGENTS.keys())}", file=sys.stderr)
        return 1

    hook_config = HOOK_COMMANDS.get(agent)
    if not hook_config:
        print(
            f"Error: Hook installation not yet supported for '{agent}'", file=sys.stderr
        )
        return 1

    # Determine config path
    if global_config:
        config_path = Path(hook_config["config"]).expanduser()
    else:
        if cwd is None:
            cwd = str(Path.cwd())
        config_path = Path(cwd) / hook_config["project_config"]

    if agent == "codex":
        codex_root = config_path.parent
        if codex_root.exists() and not codex_root.is_dir():
            print(
                f"Error: Expected Codex config directory at {codex_root}, but found a file.",
                file=sys.stderr,
            )
            print(
                "Remove or rename that file so Dippy can create .codex/hooks.json.",
                file=sys.stderr,
            )
            return 1

    # Check if agent's config directory exists
    if not config_path.parent.exists():
        if global_config:
            print(f"Error: Agent config directory not found: {config_path.parent}")
            print(f"  {agent_info.name} may not be installed.")
            print(f"  Run: dippy hooks install {agent} --global")
            return 1
        else:
            # Create project config directory
            config_path.parent.mkdir(parents=True, exist_ok=True)

    # Read existing config
    try:
        if config_path.exists():
            with open(config_path) as f:
                existing_config = json.load(f)
        else:
            # Start with empty config
            existing_config = {}
    except json.JSONDecodeError as e:
        print(f"Error: Invalid JSON in {config_path}: {e}", file=sys.stderr)
        return 1

    # Select hook entry based on --all flag
    if all_hooks and agent in ALL_HOOKS:
        hook_entry = ALL_HOOKS[agent]
    else:
        hook_entry = hook_config["hook_entry"]

    # Check for existing Dippy hook
    has_hook = _has_dippy_hook(existing_config, agent)
    legacy_command = _detect_legacy_hook_command(existing_config) if has_hook else None

    # Determine if the requested hook set is already installed
    requested_hook_types = set(hook_entry.get("hooks", {}).keys())
    installed_hook_types = _get_installed_dippy_hook_types(existing_config, agent)

    if has_hook and not force:
        if legacy_command:
            print(f"Legacy Dippy hook detected for {agent_info.name}")
            print(f"Config: {config_path}")
            print(f"Current command: {legacy_command}")
            print(f"Expected command: {_get_hook_command_for_agent(agent)}")
            print(
                f"To upgrade, run: dippy hooks install {agent} {'--global' if global_config else ''} --force"
            )
            return 0
        # Allow upgrade if user is requesting more hooks than currently installed
        if not requested_hook_types.issubset(installed_hook_types):
            # Requesting hooks that aren't installed - proceed with upgrade
            pass
        elif requested_hook_types == installed_hook_types:
            # Same hooks already installed
            print(f"Dippy hook already installed for {agent_info.name}")
            print(f"Config: {config_path}")
            return 0
        # else: requesting subset of installed hooks (downgrade) - require --force
        print(f"Dippy hook already installed for {agent_info.name}")
        print(f"Config: {config_path}")
        print("Use --force to replace existing hooks")
        return 0

    # Merge hook entry into config
    updated_config = _merge_hook_entry(existing_config, hook_entry, agent)

    # Dry run: show diff and exit
    if dry_run:
        print(f"Would update: {config_path}")
        if legacy_command:
            print("\nRemoving legacy hook:")
            print(f"  - {legacy_command}")
        print("\nAdding hooks:")
        _print_hook_summary(agent, updated_config)
        print()
        diff = _diff_configs(existing_config, updated_config, config_path)
        if diff:
            print("Diff:")
            print("=" * 60)
            print(diff)
        else:
            print("(no changes)")
        # Codex: show feature flag dry-run
        if agent == "codex":
            toml_path = _codex_config_toml_path(global_config, cwd)
            flag_modified, flag_msg = _ensure_codex_feature_flag(
                toml_path, dry_run=True
            )
            if flag_modified:
                print(f"\n{flag_msg}")
        return 0

    # Create backup unless --no-backup
    backup_path = None
    if not no_backup and config_path.exists():
        backup_path = _create_backup(config_path)

    # Write updated config
    try:
        with open(config_path, "w") as f:
            json.dump(updated_config, f, indent=2, sort_keys=True)
    except IOError as e:
        print(f"Error: Could not write to {config_path}: {e}", file=sys.stderr)
        return 1

    # Print success message
    if legacy_command:
        print(f"Upgraded Dippy hook for {agent_info.name}")
    else:
        print(f"Installed Dippy hook for {agent_info.name}")
    print(f"Config: {config_path}")
    if backup_path:
        print(f"Backup: {backup_path}")
    print()
    _print_hook_summary(agent, updated_config)

    # Codex: enable feature flag in config.toml
    if agent == "codex":
        toml_path = _codex_config_toml_path(global_config, cwd)
        flag_modified, flag_msg = _ensure_codex_feature_flag(toml_path)
        print(f"\n{flag_msg}")

    return 0


def _print_hook_summary(agent: str, config: dict) -> None:
    """Print a summary of installed hooks.

    Args:
        agent: Agent ID
        config: The configuration dict
    """
    hook_config = HOOK_COMMANDS.get(agent)
    if not hook_config:
        return

    hooks_data = config.get("dippy", {}) if agent == "agy" else config.get("hooks", {})

    # Determine which hook types to show based on agent
    # Show ALL hook types that are present, not just the minimal set
    if agent == "claude":
        # All possible Claude hook types
        hook_types = [
            ("PreToolUse", "PreToolUse"),
            ("PostToolUse", "PostToolUse"),
            ("Notification", "Notification"),
            ("Stop", "Stop"),
            ("SubagentStop", "SubagentStop"),
            ("AfterAgent", "AfterAgent"),
        ]
    elif agent == "gemini":
        hook_types = [("BeforeTool", "BeforeTool"), ("AfterTool", "AfterTool")]
    elif agent == "agy":
        hook_types = [
            ("PreToolUse", "PreToolUse"),
            ("PostToolUse", "PostToolUse"),
            ("Stop", "Stop"),
        ]
    elif agent == "codex":
        hook_types = [
            ("PreToolUse", "PreToolUse"),
            ("PermissionRequest", "PermissionRequest"),
            ("PostToolUse", "PostToolUse"),
            ("Stop", "Stop"),
        ]
    elif agent in ("cursor", "windsurf"):
        hook_types = [
            ("preToolUse", "preToolUse"),
            ("beforeShellExecution", "beforeShellExecution"),
            ("afterShellExecution", "afterShellExecution"),
        ]
    else:
        return

    for config_key, display_name in hook_types:
        if config_key in hooks_data:
            hook_list = hooks_data[config_key]
            for hook_entry in hook_list:
                # Only show Dippy hooks
                has_dippy_hook = False
                if "hooks" in hook_entry:
                    for h in hook_entry["hooks"]:
                        if _is_dippy_hook(h):
                            has_dippy_hook = True
                            break
                elif _is_dippy_hook(hook_entry):
                    has_dippy_hook = True

                if not has_dippy_hook:
                    continue

                if "matcher" in hook_entry:
                    matcher = hook_entry["matcher"]
                    if agent == "codex":
                        print(f"  + {display_name}: {matcher}")
                    else:
                        tools = _count_tools_in_matcher(matcher)
                        print(f"  + {display_name}: {matcher[:60]}... ({tools} tools)")
                elif "command" in hook_entry:
                    # Cursor/Windsurf format
                    print(f"  + {display_name}: {hook_entry['command']}")
                elif "run" in hook_entry:
                    print(f"  + {display_name}: legacy flat Codex entry")
                else:
                    # Hook without matcher or command (e.g., Stop, SubagentStop)
                    print(f"  + {display_name}: (all)")

    print(f"\nCommand: {_get_hook_command_for_agent(agent)}")


def _count_tools_in_matcher(matcher: str) -> str:
    """Count the number of tools in a matcher pattern.

    Args:
        matcher: Matcher pattern string

    Returns:
        Approximate number of tools as string (e.g., "10", "10+MCP")
    """
    # Split by | and count
    parts = matcher.split("|")
    # Subtract 1 for the mcp__.* pattern (covers many tools)
    mcp_count = sum(1 for p in parts if "mcp__" in p)
    non_mcp = len(parts) - mcp_count
    return f"{non_mcp}+MCP" if mcp_count else str(non_mcp)


def _codex_config_toml_path(global_config: bool, cwd: str | None = None) -> Path:
    """Get the Codex config.toml path for feature flag management.

    Args:
        global_config: Whether to use global or project-local path
        cwd: Current working directory (for project-local)

    Returns:
        Path to config.toml
    """
    if global_config:
        return Path.home() / ".codex" / "config.toml"
    if cwd is None:
        cwd = str(Path.cwd())
    return Path(cwd) / ".codex" / "config.toml"


def _codex_feature_flag_enabled(config_path: Path) -> bool:
    """Check whether Codex hooks feature flag is enabled in config.toml."""
    if not config_path.exists():
        return False

    try:
        content = config_path.read_text()
    except OSError:
        return False

    return bool(re.search(r"(?m)^\s*(?:hooks|codex_hooks)\s*=\s*true\s*$", content))


def uninstall(
    agent: str,
    global_config: bool = False,
    cwd: str | None = None,
    dry_run: bool = False,
) -> int:
    """Uninstall Dippy hooks for the specified agent.

    Args:
        agent: Agent ID (claude, gemini, cursor, windsurf)
        global_config: Uninstall from global config (default: project-local)
        cwd: Current working directory (for project-local installs)
        dry_run: Show what would be done without making changes

    Returns:
        Exit code: 0 for success, 1 for errors
    """
    agent_info = AGENTS.get(agent)
    if not agent_info:
        print(f"Error: Unknown agent '{agent}'", file=sys.stderr)
        print(f"Valid agents: {', '.join(AGENTS.keys())}", file=sys.stderr)
        return 1

    hook_config = HOOK_COMMANDS.get(agent)
    if not hook_config:
        print(
            f"Error: Hook uninstallation not yet supported for '{agent}'",
            file=sys.stderr,
        )
        return 1

    # Determine config path
    if global_config:
        config_path = Path(hook_config["config"]).expanduser()
    else:
        if cwd is None:
            cwd = str(Path.cwd())
        config_path = Path(cwd) / hook_config["project_config"]

    # Check if config exists
    if not config_path.exists():
        print(f"Config file not found: {config_path}")
        return 0  # Not an error, just nothing to do

    # Read existing config
    try:
        with open(config_path) as f:
            existing_config = json.load(f)
    except json.JSONDecodeError as e:
        print(f"Error: Invalid JSON in {config_path}: {e}", file=sys.stderr)
        return 1

    # Check for Dippy hook
    if not _has_dippy_hook(existing_config, agent):
        print(f"Dippy hook not found for {agent_info.name}")
        return 0

    # Remove Dippy hook from config
    updated_config = _remove_dippy_hook(existing_config, agent)

    # Dry run: show what would change
    if dry_run:
        print(f"Would update: {config_path}")
        diff = _diff_configs(existing_config, updated_config, config_path)
        if diff:
            print("\nDiff:")
            print("=" * 60)
            print(diff)
        else:
            print("(no changes)")
        return 0

    # Write updated config
    try:
        with open(config_path, "w") as f:
            json.dump(updated_config, f, indent=2, sort_keys=True)
    except IOError as e:
        print(f"Error: Could not write to {config_path}: {e}", file=sys.stderr)
        return 1

    print(f"Uninstalled Dippy hook for {agent_info.name}")
    print(f"Config: {config_path}")
    return 0


def setup_gemini_yolo(
    global_config: bool = False,
    cwd: str | None = None,
    disable: bool = False,
    dry_run: bool = False,
) -> int:
    """Enable or disable Gemini YOLO mode for Pure Dippy Control.

    Args:
        global_config: If True, update global settings.json, otherwise project-local
        cwd: Current working directory (for project-local config)
        disable: If True, set mode back to "default", otherwise set to "yolo"
        dry_run: If True, only report what would change

    Returns:
        Exit code: 0 for success, 1 for errors
    """
    agent_id = "gemini"
    hook_config = HOOK_COMMANDS.get(agent_id)
    if not hook_config:
        print(f"Error: Unknown agent {agent_id}", file=sys.stderr)
        return 1

    if global_config:
        config_path = Path(hook_config["config"]).expanduser()
    else:
        if cwd is None:
            cwd_path = Path.cwd()
        else:
            cwd_path = Path(cwd)
        config_path = cwd_path / hook_config["project_config"]

    target_mode = "default" if disable else "yolo"

    if not config_path.exists():
        if disable:
            print(f"No Gemini configuration found at {config_path}")
            return 0
        existing_config = {}
    else:
        try:
            with open(config_path) as f:
                existing_config = json.load(f)
        except (json.JSONDecodeError, IOError) as e:
            print(f"Error: Could not read {config_path}: {e}", file=sys.stderr)
            return 1

    # Check current mode
    current_mode = existing_config.get("approvalMode")
    if not current_mode and "policyEngineConfig" in existing_config:
        current_mode = existing_config["policyEngineConfig"].get("approvalMode")

    if current_mode == target_mode:
        print(f"Gemini approval mode is already '{target_mode}' in {config_path}")
        return 0

    if dry_run:
        print(f"Would set Gemini approval mode to '{target_mode}' in {config_path}")
        return 0

    # Update config
    updated_config = copy.deepcopy(existing_config)
    updated_config["approvalMode"] = target_mode

    # Remove from policyEngineConfig if it exists there to avoid confusion
    if (
        "policyEngineConfig" in updated_config
        and "approvalMode" in updated_config["policyEngineConfig"]
    ):
        del updated_config["policyEngineConfig"]["approvalMode"]
        if not updated_config["policyEngineConfig"]:
            del updated_config["policyEngineConfig"]

    # Write updated config
    try:
        config_path.parent.mkdir(parents=True, exist_ok=True)
        with open(config_path, "w") as f:
            json.dump(updated_config, f, indent=2, sort_keys=True)
    except IOError as e:
        print(f"Error: Could not write to {config_path}: {e}", file=sys.stderr)
        return 1

    action = "Disabled" if disable else "Enabled"
    print(f"{action} Gemini YOLO mode (Pure Dippy Control) in {config_path}")
    return 0


def list_hooks(
    global_config: bool = False,
    cwd: str | None = None,
    verbose: bool = False,
    json_output: bool = False,
    quiet: bool = False,
) -> int:
    """List Dippy hook status for all agents.

    Shows both global and project-local installation status in one view.

    Args:
        global_config: Ignored (both scopes are shown)
        cwd: Current working directory (for project-local checks)
        verbose: Show detailed information (matchers, commands, paths)
        json_output: Output as structured JSON
        quiet: Minimal output, exit code only

    Returns:
        Exit code: 0 for success, 1 for errors
    """
    if quiet:
        return 0  # Exit silently with success code

    if cwd is None:
        cwd_path = Path.cwd()
    else:
        cwd_path = Path(cwd)

    # Gather hook information for all agents
    hook_infos = []
    for agent_id, hook_config in HOOK_COMMANDS.items():
        agent_info = AGENTS.get(agent_id)
        if not agent_info:
            continue

        hook_info = _get_hook_info(agent_id, agent_info, hook_config, cwd_path)
        hook_infos.append(hook_info)

    # Check pi-mono extension
    pi_extension = Path.home() / ".pi" / "agent" / "extensions" / "dippy-extension.ts"
    pi_exists = pi_extension.exists()

    # Output based on format
    if json_output:
        print(_format_json_output(hook_infos, pi_exists, pi_extension))
    else:
        _format_text_output(hook_infos, pi_exists, pi_extension, verbose)

    return 0


def _get_hook_info(
    agent_id: str,
    agent_info,
    hook_config: dict,
    cwd_path: Path,
) -> HookInfo:
    """Get detailed hook information for an agent.

    Args:
        agent_id: Agent ID
        agent_info: AgentInfo object
        hook_config: Hook configuration dict
        cwd_path: Current working directory path

    Returns:
        HookInfo with status details
    """
    # Check global config
    global_path = Path(hook_config["config"]).expanduser()
    global_status = HookStatus.NOT_INSTALLED
    global_matchers = []
    global_command = ""
    global_legacy_command = ""

    if global_path.exists():
        try:
            with open(global_path) as f:
                config = json.load(f)
            if _has_dippy_hook(config, agent_id):
                global_legacy_command = _detect_legacy_hook_command(config)
                if global_legacy_command:
                    global_status = HookStatus.LEGACY
                    global_command = global_legacy_command
                else:
                    global_status = HookStatus.INSTALLED
                    global_command = _get_hook_command_for_agent(agent_id)
                global_matchers = _extract_matchers_from_config(config, agent_id)
        except (json.JSONDecodeError, IOError):
            global_status = HookStatus.ERROR
    else:
        global_status = HookStatus.NO_CONFIG

    # Check project config
    project_path = cwd_path / hook_config["project_config"]
    project_status = HookStatus.NOT_INSTALLED
    project_matchers = []
    project_command = ""
    project_legacy_command = ""
    global_feature_flag = None
    global_feature_flag_path = None
    project_feature_flag = None
    project_feature_flag_path = None

    if project_path.exists():
        try:
            with open(project_path) as f:
                config = json.load(f)
            if _has_dippy_hook(config, agent_id):
                project_legacy_command = _detect_legacy_hook_command(config)
                if project_legacy_command:
                    project_status = HookStatus.LEGACY
                    project_command = project_legacy_command
                else:
                    project_status = HookStatus.INSTALLED
                    project_command = _get_hook_command_for_agent(agent_id)
                project_matchers = _extract_matchers_from_config(config, agent_id)
        except (json.JSONDecodeError, IOError):
            project_status = HookStatus.ERROR

    if agent_id == "codex":
        global_feature_flag_path = _codex_config_toml_path(global_config=True)
        project_feature_flag_path = _codex_config_toml_path(
            global_config=False, cwd=str(cwd_path)
        )
        global_feature_flag = _codex_feature_flag_enabled(global_feature_flag_path)
        project_feature_flag = _codex_feature_flag_enabled(project_feature_flag_path)

    return HookInfo(
        agent_id=agent_id,
        agent_name=agent_info.name,
        global_status=global_status,
        global_path=global_path,
        global_matchers=global_matchers,
        global_command=global_command,
        global_legacy_command=global_legacy_command,
        project_status=project_status,
        project_path=project_path,
        project_matchers=project_matchers,
        project_command=project_command,
        project_legacy_command=project_legacy_command,
        global_feature_flag=global_feature_flag,
        global_feature_flag_path=global_feature_flag_path,
        project_feature_flag=project_feature_flag,
        project_feature_flag_path=project_feature_flag_path,
    )


def _extract_matchers_from_config(config: dict, agent_id: str) -> list[str]:
    """Extract matcher patterns from an agent's config.

    Only extracts matchers from Dippy hooks, filtering out memorix and other hooks.

    Args:
        config: Parsed configuration dict
        agent_id: Agent ID

    Returns:
        List of matcher patterns
    """
    matchers = []

    # Get hooks section
    hooks = config.get("hooks", {})

    # Different agents use different hook names
    if agent_id == "claude":
        # All possible Claude hook types
        hook_names = [
            "PreToolUse",
            "PostToolUse",
            "Notification",
            "Stop",
            "SubagentStop",
            "AfterAgent",
        ]
    elif agent_id == "gemini":
        hook_names = ["BeforeTool", "AfterTool"]
    elif agent_id == "agy":
        hook_names = ["PreToolUse", "PostToolUse", "Stop"]
        dippy_block = config.get("dippy", {})
        if isinstance(dippy_block, dict):
            for hook_name in hook_names:
                if hook_name in dippy_block:
                    for hook_entry in dippy_block[hook_name]:
                        if not isinstance(hook_entry, dict):
                            continue
                        has_dippy = _is_dippy_hook(hook_entry) or any(
                            _is_dippy_hook(h) for h in hook_entry.get("hooks", [])
                        )
                        if not has_dippy:
                            continue
                        if "matcher" in hook_entry:
                            matchers.append(f"{hook_name}: {hook_entry['matcher']}")
                        else:
                            matchers.append(f"{hook_name}: (all)")
        return matchers
    elif agent_id == "codex":
        hook_names = ["PreToolUse", "PermissionRequest", "PostToolUse", "Stop"]
    elif agent_id in ("cursor", "windsurf"):
        # These don't use matchers in the same way
        return ["(all shell commands)"]
    else:
        return []

    for hook_name in hook_names:
        if hook_name in hooks:
            for hook_entry in hooks[hook_name]:
                if agent_id == "codex":
                    nested_hooks = hook_entry.get("hooks", [])
                    if isinstance(nested_hooks, list) and any(
                        _is_dippy_hook(h) for h in nested_hooks
                    ):
                        if "matcher" in hook_entry:
                            matchers.append(f"{hook_name}: {hook_entry['matcher']}")
                        else:
                            matchers.append(f"{hook_name}: (all)")
                    elif _is_legacy_codex_run_hook(hook_entry):
                        matchers.append(f"{hook_name}: legacy flat entry")
                else:
                    # Claude/Gemini format: nested hooks structure
                    has_dippy = False
                    if "hooks" in hook_entry:
                        for h in hook_entry["hooks"]:
                            if _is_dippy_hook(h):
                                has_dippy = True
                                break

                    if not has_dippy:
                        continue

                    if "matcher" in hook_entry:
                        matchers.append(f"{hook_name}: {hook_entry['matcher']}")
                    else:
                        matchers.append(f"{hook_name}: (all)")

    return matchers


def _format_text_output(
    hook_infos: list[HookInfo],
    pi_exists: bool,
    pi_extension: Path,
    verbose: bool,
) -> None:
    """Format hook status as text output.

    Args:
        hook_infos: List of HookInfo objects
        pi_exists: Whether pi-mono extension exists
        pi_extension: Path to pi-mono extension
        verbose: Show detailed information
    """
    print("Dippy Hook Status")
    print("=" * 60)
    print()

    for info in hook_infos:
        # Determine status indicator
        codex_flag_missing = info.agent_id == "codex" and (
            (info.has_any_hook() and info.global_feature_flag is False)
            or (
                info.project_status == HookStatus.INSTALLED
                and info.project_feature_flag is False
            )
        )
        if info.has_any_hook() and not info.has_legacy() and not codex_flag_missing:
            status_indicator = "+"
        elif info.has_legacy() or codex_flag_missing:
            status_indicator = "?"
        else:
            status_indicator = " "

        # Format global status
        global_status_str = _format_status(info.global_status)
        if verbose and info.global_command:
            global_status_str += f" ({info.global_command})"

        # Format project status
        project_status_str = _format_status(info.project_status)
        if verbose and info.project_command:
            project_status_str += f" ({info.project_command})"

        # Print basic info
        print(f"[{status_indicator}] {info.agent_name}")
        print(f"    global:  {global_status_str:20} {info.global_path}")
        print(f"    project: {project_status_str:20} {info.project_path}")

        # Print actionable message if needed
        if info.global_status == HookStatus.LEGACY:
            print(f"    Run: dippy hooks install {info.agent_id} --global --force")
        elif info.global_status == HookStatus.NO_CONFIG:
            print(f"    Run: dippy hooks install {info.agent_id} --global")
        elif info.agent_id == "codex" and info.global_feature_flag is False:
            print(f"    Run: dippy hooks install {info.agent_id} --global --force")

        # Verbose details
        if verbose:
            if info.global_matchers:
                print("    Matchers:")
                for m in info.global_matchers:
                    print(f"      - {m}")
            if info.global_legacy_command:
                print(f"    Legacy command: {info.global_legacy_command}")

        if info.agent_id == "codex":
            global_flag_status = (
                "enabled"
                if info.global_feature_flag
                else ("missing" if info.global_feature_flag is False else "-")
            )
            project_flag_status = (
                "enabled"
                if info.project_feature_flag
                else ("missing" if info.project_feature_flag is False else "-")
            )
            print(
                f"    global feature:  {global_flag_status:20} {info.global_feature_flag_path}"
            )
            print(
                f"    project feature: {project_flag_status:20} {info.project_feature_flag_path}"
            )

        print()

    # pi-mono extension
    if pi_exists:
        print("[+] pi-mono: extension installed")
        print(f"    {pi_extension}")
    else:
        print("[ ] pi-mono: extension not found")
        print(f"    Expected: {pi_extension}")


def _format_status(status: HookStatus) -> str:
    """Format a HookStatus for display.

    Args:
        status: HookStatus enum

    Returns:
        Formatted status string
    """
    return {
        HookStatus.INSTALLED: "installed",
        HookStatus.LEGACY: "legacy",
        HookStatus.NOT_INSTALLED: "-",
        HookStatus.NO_CONFIG: "-",
        HookStatus.ERROR: "error",
    }.get(status, "-")


def _format_json_output(
    hook_infos: list[HookInfo],
    pi_exists: bool,
    pi_extension: Path,
) -> str:
    """Format hook status as JSON output.

    Args:
        hook_infos: List of HookInfo objects
        pi_exists: Whether pi-mono extension exists
        pi_extension: Path to pi-mono extension

    Returns:
        JSON string
    """
    output = {
        "agents": [],
        "pi_mono": {
            "installed": pi_exists,
            "path": str(pi_extension),
        },
    }

    for info in hook_infos:
        agent_data = {
            "id": info.agent_id,
            "name": info.agent_name,
            "global": {
                "status": info.global_status.value,
                "path": str(info.global_path),
                "command": info.global_command if info.global_command else None,
                "legacy_command": info.global_legacy_command
                if info.global_legacy_command
                else None,
                "matchers": info.global_matchers,
                "feature_flag_enabled": info.global_feature_flag,
                "feature_flag_path": str(info.global_feature_flag_path)
                if info.global_feature_flag_path
                else None,
            },
            "project": {
                "status": info.project_status.value,
                "path": str(info.project_path) if info.project_path else None,
                "command": info.project_command if info.project_command else None,
                "legacy_command": info.project_legacy_command
                if info.project_legacy_command
                else None,
                "matchers": info.project_matchers,
                "feature_flag_enabled": info.project_feature_flag,
                "feature_flag_path": str(info.project_feature_flag_path)
                if info.project_feature_flag_path
                else None,
            },
        }
        output["agents"].append(agent_data)

    return json.dumps(output, indent=2)


def _get_installed_dippy_hook_types(config: dict, agent: str) -> set[str]:
    """Get the set of hook types that have Dippy hooks installed.

    Args:
        config: Parsed configuration dict
        agent: Agent ID

    Returns:
        Set of hook type names (e.g., {"PreToolUse", "PostToolUse"})
    """
    hook_types = set()

    if agent in ("cursor", "windsurf"):
        # Cursor/Windsurf format
        hooks = config.get("hooks", {})
        for hook_type, hooks_list in hooks.items():
            for h in hooks_list:
                if _is_dippy_hook(h):
                    hook_types.add(hook_type)
    elif agent == "agy":
        dippy_block = config.get("dippy", {})
        if isinstance(dippy_block, dict):
            for hook_type, hook_list in dippy_block.items():
                if isinstance(hook_list, list):
                    for entry in hook_list:
                        if not isinstance(entry, dict):
                            continue
                        if _is_dippy_hook(entry) or any(
                            _is_dippy_hook(h) for h in entry.get("hooks", [])
                        ):
                            hook_types.add(hook_type)
                            break
    elif agent == "codex":
        # Codex format uses nested hooks like Claude/Gemini. Also scrub old flat entries.
        hooks = config.get("hooks", {})
        for hook_type, hook_list in hooks.items():
            for entry in hook_list:
                if not isinstance(entry, dict):
                    continue
                nested = entry.get("hooks", [])
                if isinstance(nested, list) and any(_is_dippy_hook(h) for h in nested):
                    hook_types.add(hook_type)
                    continue
                if _is_legacy_codex_run_hook(entry):
                    hook_types.add(hook_type)
    else:
        # Claude/Gemini format: nested hooks structure
        hooks = config.get("hooks", {})
        for hook_type, hook_list in hooks.items():
            for entry in hook_list:
                if "hooks" in entry:
                    for h in entry["hooks"]:
                        if _is_dippy_hook(h):
                            hook_types.add(hook_type)
                            break
                elif _is_dippy_hook(entry):
                    hook_types.add(hook_type)

    return hook_types


def _has_dippy_hook(config: dict, agent: str) -> bool:
    """Check if Dippy hook is installed in the given config.

    Detects both new-style 'dippy --agent' and legacy 'dippy-hook' commands.

    Args:
        config: Parsed configuration dict
        agent: Agent ID

    Returns:
        True if any Dippy hook is found, False otherwise
    """
    if agent == "agy":
        dippy_block = config.get("dippy")
        if isinstance(dippy_block, dict):
            for hook_list in dippy_block.values():
                if isinstance(hook_list, list):
                    for entry in hook_list:
                        if not isinstance(entry, dict):
                            continue
                        if _is_dippy_hook(entry) or any(
                            _is_dippy_hook(h) for h in entry.get("hooks", [])
                        ):
                            return True

    hooks = config.get("hooks", {})
    if not isinstance(hooks, dict):
        return False

    for hook_list in hooks.values():
        if not isinstance(hook_list, list):
            continue
        for entry in hook_list:
            if not isinstance(entry, dict):
                continue
            if _is_dippy_hook(entry):
                return True
            nested = entry.get("hooks", [])
            if isinstance(nested, list) and any(_is_dippy_hook(h) for h in nested):
                return True

    return False


def _has_legacy_dippy_hook(config: dict) -> bool:
    """Check if old-style 'dippy-hook' command is installed.

    Args:
        config: Parsed configuration dict

    Returns:
        True if legacy dippy-hook command is found, False otherwise
    """
    config_str = json.dumps(config)
    # Check for old dippy-hook command
    return (
        '"command": "dippy-hook' in config_str or '"command":"dippy-hook' in config_str
    )


def _merge_hook_entry(config: dict, hook_entry: dict, agent: str) -> dict:
    """Merge Dippy hook entry into existing config.

    First removes any existing Dippy hooks, then adds the new ones.

    Args:
        config: Existing configuration dict
        hook_entry: Hook entry to insert
        agent: Agent ID (for special handling)

    Returns:
        Merged configuration dict
    """
    result = copy.deepcopy(config)

    # First, remove any existing Dippy hooks
    result = _remove_dippy_hook(result, agent)

    if agent == "agy":
        dippy_block = copy.deepcopy(hook_entry.get("hooks", {}))
        result["dippy"] = dippy_block
        return result

    # Then add the new hooks
    if agent == "cursor":
        # Cursor uses simple format - append to hooks list
        if "hooks" not in result:
            result["hooks"] = {}
        if "version" not in result:
            result["version"] = 1

        # Get the hook type (beforeShellExecution for cursor)
        hook_entry_data = hook_entry.get("hooks", {})
        for hook_type, hooks_list in hook_entry_data.items():
            if hook_type not in result["hooks"]:
                result["hooks"][hook_type] = []
            result["hooks"][hook_type].extend(hooks_list)
    else:
        # Claude/Gemini use nested hooks structure
        if "hooks" not in result:
            result["hooks"] = {}

        for hook_type, hooks_list in hook_entry.get("hooks", {}).items():
            if hook_type not in result["hooks"]:
                result["hooks"][hook_type] = []
            result["hooks"][hook_type].extend(hooks_list)

    return result


def _is_dippy_hook(hook_obj: dict) -> bool:
    """Check if a hook object is a Dippy hook.

    Must distinguish between:
    - New style: dippy --claude, dippy --gemini, etc. (command key)
    - Codex style: run array with dippy as first element
    - Legacy: /path/to/dippy-hook, /path/to/dippy
    - NOT a match: random command with "dippy" in path like /home/user/adippy-workspace/script.sh

    Args:
        hook_obj: Hook dictionary to check

    Returns:
        True if this is a Dippy hook, False otherwise
    """
    if not isinstance(hook_obj, dict):
        return False

    # Claude/Gemini/Cursor format: "command" key
    command = hook_obj.get("command", "")
    if command:
        cmd_lower = command.lower()
        return (
            # New style: "dippy --claude", "dippy --gemini", "dippy" alone
            cmd_lower.startswith("dippy ")
            or cmd_lower == "dippy"
            # Legacy: any path ending with dippy-hook
            or cmd_lower.endswith("dippy-hook")
            # Legacy in path: /path/to/dippy, /my-dippy-scripts/hook, /path/to/dippy-hook
            or "/dippy" in cmd_lower
            or "\\dippy" in cmd_lower
        )

    return False


def _is_legacy_codex_run_hook(hook_obj: dict) -> bool:
    """Detect old broken Codex flat hook entries so install/uninstall can clean them up."""
    if not isinstance(hook_obj, dict):
        return False

    run = hook_obj.get("run", [])
    if not isinstance(run, list) or not run:
        return False

    first = str(run[0]).lower()
    return first == "dippy" or first.endswith("/dippy") or first.endswith("\\dippy")


def _remove_dippy_hook(config: dict, agent: str) -> dict:
    """Remove Dippy hook from configuration.

    Removes ALL Dippy hooks from ALL supported hook types, while preserving
    non-Dippy hooks like memorix.

    Args:
        config: Existing configuration dict
        agent: Agent ID (claude, gemini, cursor, windsurf)

    Returns:
        Configuration dict with Dippy hook removed
    """
    result = copy.deepcopy(config)

    if agent == "agy":
        if "dippy" in result:
            del result["dippy"]
        if "hooks" in result and isinstance(result["hooks"], dict):
            for hook_type, hook_list in list(result["hooks"].items()):
                if isinstance(hook_list, list):
                    result["hooks"][hook_type] = [
                        entry
                        for entry in hook_list
                        if not (
                            _is_dippy_hook(entry)
                            or any(_is_dippy_hook(h) for h in entry.get("hooks", []))
                        )
                    ]
                    if not result["hooks"][hook_type]:
                        del result["hooks"][hook_type]
            if not result["hooks"]:
                del result["hooks"]
        return result

    if agent in ("cursor", "windsurf"):
        # Cursor/Windsurf format: simple list of hook dicts with "command" key
        if "hooks" in result:
            empty_hook_types = []
            for hook_type, hooks_list in result["hooks"].items():
                result["hooks"][hook_type] = [
                    h for h in hooks_list if not _is_dippy_hook(h)
                ]
                if not result["hooks"][hook_type]:
                    empty_hook_types.append(hook_type)
            # Remove empty hook types after iteration
            for hook_type in empty_hook_types:
                del result["hooks"][hook_type]
    elif agent == "codex":
        # Codex format: nested hooks. Also remove old broken flat run-array entries.
        hook_types = ["PreToolUse", "PermissionRequest", "PostToolUse", "Stop"]
        if "hooks" in result:
            for hook_type in hook_types:
                if hook_type in result["hooks"]:
                    kept_entries = []
                    for entry in result["hooks"][hook_type]:
                        if not isinstance(entry, dict):
                            kept_entries.append(entry)
                            continue

                        if _is_legacy_codex_run_hook(entry):
                            continue

                        if "hooks" in entry:
                            entry = copy.deepcopy(entry)
                            entry["hooks"] = [
                                h for h in entry["hooks"] if not _is_dippy_hook(h)
                            ]
                            if entry["hooks"]:
                                kept_entries.append(entry)
                            continue

                        kept_entries.append(entry)

                    result["hooks"][hook_type] = kept_entries
                    if not result["hooks"][hook_type]:
                        del result["hooks"][hook_type]
    else:
        # Claude/Gemini format - nested hooks structure
        hook_types = [
            "PreToolUse",
            "PostToolUse",
            "Notification",
            "Stop",
            "SubagentStop",
            "AfterAgent",
        ]

        if "hooks" in result:
            for hook_type in hook_types:
                if hook_type in result["hooks"]:
                    for entry in result["hooks"][hook_type]:
                        if "hooks" in entry:
                            entry["hooks"] = [
                                h for h in entry["hooks"] if not _is_dippy_hook(h)
                            ]

            # Clean up empty hook entries
            for hook_type in hook_types:
                if hook_type in result["hooks"]:
                    result["hooks"][hook_type] = [
                        entry
                        for entry in result["hooks"][hook_type]
                        if entry.get("hooks")  # Keep only entries with non-empty hooks
                    ]
                    if not result["hooks"][hook_type]:
                        del result["hooks"][hook_type]

    # Clean up empty hooks dict
    if "hooks" in result and not result["hooks"]:
        del result["hooks"]

    return result
