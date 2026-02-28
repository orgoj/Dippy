"""
Agent detection and configuration for AI coding assistant hook systems.

Provides centralized registry of supported agents with their config paths,
hook formats, and detection logic. Used by CLI commands for hook management.
"""

from __future__ import annotations

import os
from dataclasses import dataclass
from pathlib import Path
from typing import Literal


@dataclass(frozen=True)
class AgentInfo:
    """Information about a supported AI coding assistant.

    Attributes:
        id: Internal identifier for the agent
        name: Human-readable display name
        global_config: Path to global config directory (may be file or dir)
        project_config: Name of project-local config directory/file
        hook_format: Hook system format type
        config_format: Config file format (json, toml, etc.)
        env_flags: Environment variables that can indicate this agent
        cli_flags: Command-line flags that can indicate this agent
    """

    id: str
    name: str
    global_config: Path
    project_config: str
    hook_format: Literal["claude", "cursor", "gemini", "pi", "none"]
    config_format: Literal["json", "toml"]
    env_flags: tuple[str, ...]
    cli_flags: tuple[str, ...]

    def is_installed(self) -> bool:
        """Check if this agent appears to be installed."""
        # Check config directory existence
        if self.global_config.parent.exists():
            # For file-based configs (like settings.json), check if file exists
            # For dir-based configs, check if directory exists
            if "." in self.global_config.name:
                # It's a file like settings.json
                return self.global_config.exists()
            else:
                # It's a directory like .claude/
                return self.global_config.exists()
        return False


# Registry of all supported agents
AGENTS: dict[str, AgentInfo] = {
    "claude": AgentInfo(
        id="claude",
        name="Claude Code",
        global_config=Path.home() / ".claude" / "settings.json",
        project_config=".claude/settings.json",
        hook_format="claude",
        config_format="json",
        env_flags=("DIPPY_CLAUDE",),
        cli_flags=("--claude",),
    ),
    "gemini": AgentInfo(
        id="gemini",
        name="Gemini CLI",
        global_config=Path.home() / ".gemini" / "settings.json",
        project_config=".gemini/settings.json",
        hook_format="gemini",
        config_format="json",
        env_flags=("DIPPY_GEMINI",),
        cli_flags=("--gemini",),
    ),
    "cursor": AgentInfo(
        id="cursor",
        name="Cursor IDE",
        global_config=Path.home() / ".cursor" / "hooks.json",
        project_config=".cursor/hooks.json",
        hook_format="cursor",
        config_format="json",
        env_flags=("DIPPY_CURSOR",),
        cli_flags=("--cursor",),
    ),
    "windsurf": AgentInfo(
        id="windsurf",
        name="Windsurf",
        global_config=Path.home() / ".windsurf" / "hooks.json",
        project_config=".windsurf/hooks.json",
        hook_format="cursor",  # Windsurf uses Cursor-compatible format
        config_format="json",
        env_flags=("DIPPY_WINDSURF",),
        cli_flags=("--windsurf",),
    ),
    "pi": AgentInfo(
        id="pi",
        name="pi-mono",
        global_config=Path.home() / ".pi" / "config.json",
        project_config=".pi/hooks.json",
        hook_format="pi",
        config_format="json",
        env_flags=("DIPPY_PI",),
        cli_flags=("--pi",),
    ),
    "moltbot": AgentInfo(
        id="moltbot",
        name="Moltbot",
        global_config=Path.home() / ".moltbot" / "config.json",
        project_config=".moltbot/hooks.json",
        hook_format="pi",  # Moltbot uses pi-mono compatible format
        config_format="json",
        env_flags=("DIPPY_MOLTBOT",),
        cli_flags=("--moltbot",),
    ),
    "codex": AgentInfo(
        id="codex",
        name="OpenAI Codex CLI",
        global_config=Path.home() / ".codex" / "config.toml",
        project_config=".codex/config.toml",
        hook_format="none",  # Codex has no full hook system
        config_format="toml",
        env_flags=("DIPPY_CODEX",),
        cli_flags=("--codex",),
    ),
    "pearai": AgentInfo(
        id="pearai",
        name="PearAI",
        global_config=Path.home() / ".pearai" / "hooks.json",
        project_config=".pearai/hooks.json",
        hook_format="claude",  # PearAI uses Claude-compatible format
        config_format="json",
        env_flags=("DIPPY_PEARAI",),
        cli_flags=("--pearai",),
    ),
}


def detect_agents() -> dict[str, AgentInfo]:
    """Detect which AI coding assistants are installed on this system.

    Returns:
        Dictionary mapping agent IDs to their AgentInfo for installed agents.
    """
    import sys

    installed = {}
    for agent_id, info in AGENTS.items():
        # Check config directory existence
        if info.is_installed():
            installed[agent_id] = info
            continue

        # Check environment variables
        if any(os.environ.get(flag) for flag in info.env_flags):
            installed[agent_id] = info
            continue

        # Check CLI flags in sys.argv
        if any(flag in sys.argv for flag in info.cli_flags):
            installed[agent_id] = info
            continue

    return installed


def resolve_dippy_command(agent_id: str, cwd: Path | None = None) -> str:
    """Get the dippy hook command for a specific agent.

    Args:
        agent_id: The agent identifier (e.g., "claude", "cursor")
        cwd: Current working directory (for resolving relative paths)

    Returns:
        The appropriate dippy command for that agent's hook system.
    """
    agent = AGENTS.get(agent_id)
    if not agent:
        return "dippy"

    if cwd is None:
        cwd = Path.cwd()

    # Resolve dippy executable path
    dippy_path = _find_dippy_executable()

    # pi-mono and moltbot use pi_wrapper.py
    if agent_id in ("pi", "moltbot"):
        wrapper_path = _find_pi_wrapper()
        if wrapper_path:
            return f'python3 {wrapper_path}'
        return dippy_path

    # All other agents use the dippy command with their specific flag
    # The flag matches the agent_id (e.g., --claude, --cursor, --windsurf)
    if agent_id == "pearai":
        # PearAI uses Claude-compatible format
        return f"{dippy_path} --claude"
    return f"{dippy_path} --{agent_id}"


def _find_dippy_executable() -> str:
    """Find the dippy executable in the current environment.

    Returns:
        Path to dippy executable (could be "dippy", full path, or python module)
    """
    import shutil

    # Try to find dippy in PATH
    dippy_in_path = shutil.which("dippy")
    if dippy_in_path:
        return dippy_in_path

    # Fall back to module invocation
    return "python3 -m dippy.dippy"


def _find_pi_wrapper() -> str | None:
    """Find the pi_wrapper.py script for pi-mono integration.

    Returns:
        Path to pi_wrapper.py if found, None otherwise
    """
    # Try relative to this module first
    here = Path(__file__).parent.parent
    wrapper = here / "pi_wrapper.py"
    if wrapper.exists():
        return str(wrapper)

    # Try in Python site-packages
    import sys

    for site_dir in sys.path:
        candidate = Path(site_dir) / "dippy" / "pi_wrapper.py"
        if candidate.exists():
            return str(candidate)

    return None


def get_agent_info(agent_id: str) -> AgentInfo | None:
    """Get AgentInfo for a specific agent ID.

    Args:
        agent_id: The agent identifier

    Returns:
        AgentInfo if found, None otherwise
    """
    return AGENTS.get(agent_id)


def list_all_agents() -> list[AgentInfo]:
    """Get all supported agents regardless of installation status.

    Returns:
        List of all AgentInfo objects
    """
    return list(AGENTS.values())
