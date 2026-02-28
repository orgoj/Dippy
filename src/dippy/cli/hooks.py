"""
Dippy hooks management - install and uninstall Dippy hooks for AI coding assistants.

Provides functions to add/remove Dippy hook configuration from agent
settings files while preserving existing hooks and proper JSON formatting.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Literal

from dippy.cli.agents import AGENTS


# Hook entry point for different agents
HOOK_COMMANDS = {
    "claude": {
        "config": "~/.claude/settings.json",
        "project_config": ".claude/settings.json",
        "hook_entry": {
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash|Read|Write|Edit|MultiEdit",
                        "hooks": [
                            {
                                "type": "command",
                                "command": "dippy --claude",
                            }
                        ],
                    }
                ]
            }
        },
    },
    "gemini": {
        "config": "~/.gemini/settings.json",
        "project_config": ".gemini/settings.json",
        "hook_entry": {
            "hooks": {
                "BeforeTool": [
                    {
                        "matcher": "run_shell_command",
                        "hooks": [
                            {
                                "name": "dippy-approval",
                                "type": "command",
                                "command": "dippy --gemini",
                            }
                        ],
                    }
                ]
            }
        },
    },
    "cursor": {
        "config": "~/.cursor/hooks.json",
        "project_config": ".cursor/hooks.json",
        "hook_entry": {
            "version": 1,
            "hooks": {
                "beforeShellExecution": [
                    {"command": "dippy --cursor"}
                ]
            }
        },
    },
    "windsurf": {
        "config": "~/.windsurf/hooks.json",
        "project_config": ".windsurf/hooks.json",
        "hook_entry": {
            "version": 1,
            "hooks": {
                "beforeShellExecution": [
                    {"command": "dippy --windsurf"}
                ]
            }
        },
    },
}


def install(
    agent: str,
    global_config: bool = False,
    cwd: str | None = None,
) -> int:
    """Install Dippy hooks for the specified agent.

    Args:
        agent: Agent ID (claude, gemini, cursor, windsurf)
        global_config: Install to global config (default: project-local)
        cwd: Current working directory (for project-local installs)

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
        print(f"Error: Hook installation not yet supported for '{agent}'", file=sys.stderr)
        return 1

    # Determine config path
    if global_config:
        config_path = Path(hook_config["config"]).expanduser()
    else:
        if cwd is None:
            cwd = Path.cwd()
        config_path = Path(cwd) / hook_config["project_config"]

    # Check if agent's config directory exists
    if not config_path.parent.exists():
        if global_config:
            print(f"Error: Agent config directory not found: {config_path.parent}")
            print(f"  {agent_info.name} may not be installed.")
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

    # Check for existing Dippy hook
    if _has_dippy_hook(existing_config, agent):
        print(f"Dippy hook already installed for {agent_info.name}")
        print(f"Config: {config_path}")
        return 0

    # Merge hook entry into config
    updated_config = _merge_hook_entry(existing_config, hook_config["hook_entry"], agent)

    # Write updated config
    try:
        with open(config_path, "w") as f:
            json.dump(updated_config, f, indent=2)
    except IOError as e:
        print(f"Error: Could not write to {config_path}: {e}", file=sys.stderr)
        return 1

    print(f"Installed Dippy hook for {agent_info.name}")
    print(f"Config: {config_path}")
    return 0


def uninstall(
    agent: str,
    global_config: bool = False,
    cwd: str | None = None,
) -> int:
    """Uninstall Dippy hooks for the specified agent.

    Args:
        agent: Agent ID (claude, gemini, cursor, windsurf)
        global_config: Uninstall from global config (default: project-local)
        cwd: Current working directory (for project-local installs)

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
        print(f"Error: Hook uninstallation not yet supported for '{agent}'", file=sys.stderr)
        return 1

    # Determine config path
    if global_config:
        config_path = Path(hook_config["config"]).expanduser()
    else:
        if cwd is None:
            cwd = Path.cwd()
        config_path = Path(cwd) / hook_config["project_config"]

    # Check if config exists
    if not config_path.exists():
        print(f"Error: Config file not found: {config_path}")
        return 1

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

    # Write updated config
    try:
        with open(config_path, "w") as f:
            json.dump(updated_config, f, indent=2)
    except IOError as e:
        print(f"Error: Could not write to {config_path}: {e}", file=sys.stderr)
        return 1

    print(f"Uninstalled Dippy hook for {agent_info.name}")
    print(f"Config: {config_path}")
    return 0


def list_hooks(
    global_config: bool = False,
    cwd: str | None = None,
) -> int:
    """List Dippy hook status for all agents.

    Shows both global and project-local installation status in one view.

    Args:
        global_config: Ignored (both scopes are shown)
        cwd: Current working directory (for project-local checks)

    Returns:
        Exit code: 0 for success, 1 for errors
    """
    if cwd is None:
        cwd_path = Path.cwd()
    else:
        cwd_path = Path(cwd)

    print("Dippy Hook Status")
    print("=" * 60)
    print()

    # Only show agents that have hook support defined
    for agent_id, hook_config in HOOK_COMMANDS.items():
        agent_info = AGENTS.get(agent_id)
        if not agent_info:
            continue

        # Check global config
        global_path = Path(hook_config["config"]).expanduser()
        global_installed = False
        global_legacy = False
        if global_path.exists():
            try:
                with open(global_path) as f:
                    config = json.load(f)
                global_installed = _has_dippy_hook(config, agent_id)
                global_legacy = _has_legacy_dippy_hook(config)
            except (json.JSONDecodeError, IOError):
                pass

        # Check project config
        project_path = cwd_path / hook_config["project_config"]
        project_installed = False
        project_legacy = False
        if project_path.exists():
            try:
                with open(project_path) as f:
                    config = json.load(f)
                project_installed = _has_dippy_hook(config, agent_id)
                project_legacy = _has_legacy_dippy_hook(config)
            except (json.JSONDecodeError, IOError):
                pass

        # Build status string
        global_status = ""
        if global_legacy and not global_installed:
            global_status = "legacy"
        elif global_installed:
            global_status = "installed"
        else:
            global_status = "-"

        project_status = ""
        if project_legacy and not project_installed:
            project_status = "legacy"
        elif project_installed:
            project_status = "installed"
        else:
            project_status = "-"

        # Format output
        status_indicator = " "
        if global_installed or project_installed:
            status_indicator = "+"
        elif global_legacy or project_legacy:
            status_indicator = "?"

        print(f"[{status_indicator}] {agent_info.name}")
        print(f"       global:   {global_status:12} {global_path}")
        print(f"       project:  {project_status:12} {project_path}")
        print()

    # Check pi-mono extension
    pi_extension = Path.home() / ".pi" / "agent" / "extensions" / "dippy-extension.ts"
    if pi_extension.exists():
        print(f"[+] pi-mono: extension installed")
        print(f"       {pi_extension}")
    else:
        print(f"[ ] pi-mono: extension not found")
        print(f"       Expected: {pi_extension}")

    return 0


def _has_dippy_hook(config: dict, agent: str) -> bool:
    """Check if new-style Dippy hook is installed in the given config.

    Args:
        config: Parsed configuration dict
        agent: Agent ID

    Returns:
        True if new-style Dippy hook is found, False otherwise
    """
    config_str = json.dumps(config)
    # Check for dippy command (new style)
    return '"command": "dippy' in config_str or '"command":"dippy' in config_str


def _has_legacy_dippy_hook(config: dict) -> bool:
    """Check if old-style 'dippy-hook' command is installed.

    Args:
        config: Parsed configuration dict

    Returns:
        True if legacy dippy-hook command is found, False otherwise
    """
    config_str = json.dumps(config)
    # Check for old dippy-hook command
    return '"command": "dippy-hook' in config_str or '"command":"dippy-hook' in config_str


def _merge_hook_entry(config: dict, hook_entry: dict, agent: str) -> dict:
    """Merge Dippy hook entry into existing config.

    Args:
        config: Existing configuration dict
        hook_entry: Hook entry to insert
        agent: Agent ID (for special handling)

    Returns:
        Merged configuration dict
    """
    import copy

    result = copy.deepcopy(config)

    # Special handling for different agent formats
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


def _remove_dippy_hook(config: dict, agent: str) -> dict:
    """Remove Dippy hook from configuration.

    Args:
        config: Existing configuration dict
        agent: Agent ID

    Returns:
        Configuration dict with Dippy hook removed
    """
    import copy

    result = copy.deepcopy(config)

    if agent == "cursor":
        # Cursor: remove from hooks list
        if "hooks" in result:
            for hook_type, hooks_list in result["hooks"].items():
                # Filter out hooks that contain "dippy"
                result["hooks"][hook_type] = [
                    h for h in hooks_list
                    if isinstance(h, dict) and "dippy" not in str(h).lower()
                ]
                if not result["hooks"][hook_type]:
                    del result["hooks"][hook_type]
    else:
        # Claude/Gemini: remove from hooks structure
        if "hooks" in result:
            for hook_type, hooks_list in result["hooks"].items():
                # Filter out hooks that contain "dippy" in command
                result["hooks"][hook_type] = [
                    h for h in hooks_list
                    if not (
                        isinstance(h, dict)
                        and h.get("command", "").lower().startswith("dippy")
                    )
                ]
                if not result["hooks"][hook_type]:
                    del result["hooks"][hook_type]

    # Clean up empty hooks dict
    if "hooks" in result and not result["hooks"]:
        del result["hooks"]

    return result
