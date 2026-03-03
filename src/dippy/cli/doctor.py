"""
Dippy doctor command for installation and configuration diagnosis.

Provides comprehensive health checks for Dippy installation including
PATH configuration, hook status, config validation, log file health,
and agent-specific diagnostics.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from typing import Literal

from dippy.cli.agents import AGENTS, detect_agents
from dippy.cli.hooks import HOOK_COMMANDS, _has_dippy_hook, _has_legacy_dippy_hook


class HealthStatus(Enum):
    """Health status levels with corresponding indicators and exit codes."""

    OK = ("ok", 0, "✓", "OK")
    WARNING = ("warning", 1, "⚠", "WARNING")
    CRITICAL = ("critical", 2, "✗", "CRITICAL")

    @property
    def level(self) -> str:
        return self.value[0]

    @property
    def exit_code(self) -> int:
        return self.value[1]

    @property
    def symbol(self) -> str:
        return self.value[2]

    @property
    def label(self) -> str:
        return self.value[3]


@dataclass
class CheckResult:
    """Result of a single health check."""

    name: str
    status: HealthStatus
    message: str
    details: str | None = None

    def display(self, verbose: bool = False) -> None:
        """Display the check result to stdout."""
        print(f"{self.status.symbol} {self.name}: {self.message}")
        if self.details:
            for line in self.details.split("\n"):
                print(f"    {line}")


def run(
    agent: str | None = None,
    verbose: bool = False,
    cwd: str | None = None,
) -> int:
    """Run diagnostic checks and return overall exit code.

    Args:
        agent: Optional agent ID to filter checks for a specific agent
        verbose: Show detailed diagnostic information
        cwd: Current working directory

    Returns:
        Exit code: 0 (all OK), 1 (warnings), 2 (critical issues)
    """
    if cwd is None:
        cwd_path = Path.cwd()
    else:
        cwd_path = Path(cwd)

    checks: list[CheckResult] = []

    # 1. Installation check
    checks.append(check_installation())

    # 2. Hook status check (returns list of results)
    checks.extend(check_hook_status(cwd_path, verbose))

    # 3. Config validation
    checks.append(check_config_validation(cwd_path))

    # 4. Log health check
    checks.append(check_log_health(verbose))

    # 5. Agent-specific check (if --agent specified)
    if agent:
        checks.append(check_agent_specific(agent, cwd_path, verbose))

    # Display results
    print("Dippy Installation Check")
    print("=" * 40)

    for check in checks:
        check.display(verbose)

    # Return highest severity exit code
    max_status = max((check.status for check in checks), key=lambda s: s.exit_code)
    return max_status.exit_code


def check_installation() -> CheckResult:
    """Check if Dippy is properly installed on PATH."""
    # Check if dippy command is available
    dippy_path = shutil.which("dippy")
    if not dippy_path:
        return CheckResult(
            "Installation",
            HealthStatus.CRITICAL,
            "dippy not found on PATH",
            "Install Dippy: uv tool install dippy or pip install dippy",
        )

    # Check if we can run it
    try:
        result = subprocess.run(
            [dippy_path, "--version"],
            capture_output=True,
            text=True,
            timeout=5,
        )
        if result.returncode == 0:
            version = result.stdout.strip().split("\n")[0]
            return CheckResult(
                "Installation",
                HealthStatus.OK,
                f"Dippy installed ({version})",
                f"Location: {dippy_path}",
            )
        else:
            return CheckResult(
                "Installation",
                HealthStatus.WARNING,
                "dippy found but not executable",
                f"Return code: {result.returncode}",
            )
    except Exception as e:
        return CheckResult(
            "Installation",
            HealthStatus.WARNING,
            f"dippy check failed: {e}",
            str(e),
        )


def check_hook_status(cwd_path: Path, verbose: bool) -> list[CheckResult]:
    """Check status of hooks for all agents.

    Returns a CheckResult for each agent plus pi-mono extension.
    """
    results = []

    # Check each agent with hook support
    for agent_id in ("claude", "gemini", "cursor", "windsurf"):
        hook_config = HOOK_COMMANDS.get(agent_id)
        agent_info = AGENTS.get(agent_id)
        if not hook_config or not agent_info:
            continue

        # Check global config
        global_path = Path(hook_config["config"]).expanduser()
        global_config = None
        if global_path.exists():
            try:
                with open(global_path) as f:
                    global_config = json.load(f)
            except (json.JSONDecodeError, IOError):
                pass

        # Check project config
        project_path = cwd_path / hook_config["project_config"]
        project_config = None
        if project_path.exists():
            try:
                with open(project_path) as f:
                    project_config = json.load(f)
            except (json.JSONDecodeError, IOError):
                pass

        # Determine agent status
        agent_exists = global_config is not None or project_config is not None

        if not agent_exists:
            # Agent not installed
            results.append(CheckResult(
                f"Hook: {agent_info.name}",
                HealthStatus.WARNING,
                "Not installed",
                f"Config not found at:\n  {global_path}\n  {project_path}",
            ))
            continue

        # Check for dippy hook in configs
        has_hook = False
        hook_type = None
        locations = []
        legacy_path = None

        for config, label in [(global_config, "global"), (project_config, "project")]:
            if config is None:
                continue
            if _has_dippy_hook(config, agent_id):
                has_hook = True
                locations.append(label)
                # Determine hook type (new vs legacy) and extract command path
                config_str = json.dumps(config)
                if 'dippy-hook' in config_str or '/dippy' in config_str:
                    hook_type = "legacy (full path)"
                    # Extract the actual command path from config
                    import re
                    match = re.search(r'"command":\s*"([^"]*dippy[^"]*)"', config_str)
                    if match:
                        legacy_path = match.group(1)

        if not has_hook:
            results.append(CheckResult(
                f"Hook: {agent_info.name}",
                HealthStatus.WARNING,
                "Agent present, hook not installed",
                f"Install with: dippy hooks install {agent_id} --global",
            ))
        elif hook_type == "legacy (full path)":
            details = f"Legacy command: {legacy_path}\nUpdate with: dippy hooks install {agent_id} --global" if legacy_path else f"Update with: dippy hooks install {agent_id} --global"
            results.append(CheckResult(
                f"Hook: {agent_info.name}",
                HealthStatus.WARNING,
                f"Legacy hook ({', '.join(locations)})",
                details,
            ))
        else:
            results.append(CheckResult(
                f"Hook: {agent_info.name}",
                HealthStatus.OK,
                f"Installed ({', '.join(locations)})",
                None,
            ))

    # Check pi-mono extension
    pi_extension = Path.home() / ".pi" / "agent" / "extensions" / "dippy-extension.ts"
    if pi_extension.exists():
        # Check file type and symlink target
        file_type = "file"
        target_info = str(pi_extension)

        if pi_extension.is_symlink():
            target = pi_extension.resolve()
            file_type = "symlink"
            target_info = f"{pi_extension} -> {target}"

        # Add wrapper info only in verbose mode
        details = None
        if verbose:
            from dippy.cli.agents import _find_pi_wrapper
            wrapper_path = _find_pi_wrapper()
            if wrapper_path:
                details = f"{target_info}\nBridge: {wrapper_path}"
            else:
                details = f"{target_info}\nBridge: not found (pi-mono may not work)"

        results.append(CheckResult(
            "Hook: pi-mono",
            HealthStatus.OK,
            f"Extension installed ({file_type})",
            details if verbose else target_info,
        ))
    else:
        results.append(CheckResult(
            "Hook: pi-mono",
            HealthStatus.WARNING,
            "Extension not found",
            f"Expected: {pi_extension}",
        ))

    return results


def check_config_validation(cwd_path: Path) -> CheckResult:
    """Validate Dippy configuration files."""
    from dippy.core.config import ConfigError, load_config

    errors = []

    # Check global config
    global_config = Path.home() / ".dippy" / "config"
    if global_config.exists():
        try:
            load_config(cwd_path, config_path=str(global_config))
        except ConfigError as e:
            errors.append(f"Global config: {_format_config_error(e, global_config)}")

    # Check project config
    project_config = cwd_path / ".dippy"
    if project_config.exists():
        try:
            load_config(cwd_path, config_path=None)
        except ConfigError as e:
            errors.append(f"Project config: {_format_config_error(e, project_config)}")

    if errors:
        return CheckResult(
            "Configuration",
            HealthStatus.CRITICAL,
            f"{len(errors)} error(s) found",
            "\n".join(errors),
        )

    return CheckResult(
        "Configuration",
        HealthStatus.OK,
        "Configuration is valid",
        None,
    )


def _format_config_error(error: Exception, config_path: Path) -> str:
    """Format a config error with context."""
    msg = str(error)
    # Try to extract line number and provide context
    if "line" in msg.lower():
        # Error already has line info
        return f"{config_path}: {msg}"
    else:
        return f"{config_path}: {msg}"


def check_log_health(verbose: bool) -> CheckResult:
    """Check health of Dippy log files."""
    log_paths = [
        (Path.home() / ".claude" / "hook-approvals.log", "Claude Code"),
        (Path.home() / ".gemini" / "hook-approvals.log", "Gemini CLI"),
        (Path.home() / ".dippy" / "audit.log", "Dippy audit"),
    ]

    issues = []
    writable = []
    sizes = []

    for log_path, name in log_paths:
        # Check if parent directory exists and is writable
        if log_path.parent.exists():
            test_file = log_path.parent / ".dippy_write_test"
            try:
                test_file.touch()
                test_file.unlink()
                writable.append(name)
            except PermissionError:
                issues.append(f"{name}: log directory not writable ({log_path.parent})")
            except OSError:
                issues.append(f"{name}: cannot write to log directory ({log_path.parent})")

        # Check log file size
        if log_path.exists():
            size_mb = log_path.stat().st_size / (1024 * 1024)
            if size_mb > 10:
                issues.append(f"{name}: log file is {size_mb:.1f}MB (consider rotation)")
                sizes.append(f"{name}: {size_mb:.1f}MB")
            elif verbose:
                sizes.append(f"{name}: {size_mb:.2f}MB")

    if issues:
        return CheckResult(
            "Logs",
            HealthStatus.WARNING,
            f"{len(issues)} issue(s) detected",
            "\n".join(issues),
        )

    details = None
    if verbose and writable:
        details = f"Writable: {', '.join(writable)}"
        if sizes:
            details += f"\nSizes: {', '.join(sizes)}"

    if writable:
        return CheckResult(
            "Logs",
            HealthStatus.OK,
            "Log directories are writable",
            details,
        )

    return CheckResult(
        "Logs",
        HealthStatus.OK,
        "Log files not created yet",
        "This is normal for new installations",
    )


def check_agent_specific(agent_id: str, cwd: Path, verbose: bool) -> CheckResult:
    """Run agent-specific diagnostic checks."""
    import json

    agent = AGENTS.get(agent_id)
    if not agent:
        return CheckResult(
            agent_id.capitalize(),
            HealthStatus.CRITICAL,
            f"Unknown agent: {agent_id}",
            f"Valid agents: {', '.join(AGENTS.keys())}",
        )

    details = []
    issues = []

    # Check if agent is installed (config exists)
    global_config = Path(agent.global_config).expanduser()
    if global_config.exists():
        details.append(f"Global config: {global_config}")

        # Check if Dippy hook is installed
        hook_config = HOOK_COMMANDS.get(agent_id)
        if hook_config:
            try:
                with open(global_config) as f:
                    config = json.load(f)
                if _has_dippy_hook(config, agent_id):
                    details.append("Dippy hook: installed")
                elif _has_legacy_dippy_hook(config):
                    details.append("Dippy hook: legacy (old 'dippy-hook')")
                    issues.append("Legacy hook detected - consider updating")
                else:
                    details.append("Dippy hook: not installed")
                    issues.append("Dippy hook not found in config")
            except (json.JSONDecodeError, IOError):
                details.append("Dippy hook: unable to check (config read error)")
    else:
        issues.append(f"{agent.name} not installed (no config found)")

    # Check project config
    project_config = cwd / agent.project_config
    if project_config.exists():
        details.append(f"Project config: {project_config}")
        if hook_config:
            try:
                with open(project_config) as f:
                    config = json.load(f)
                if _has_dippy_hook(config, agent_id):
                    details.append("Dippy hook in project: installed")
            except (json.JSONDecodeError, IOError):
                pass

    # Hook format info
    format_info = {
        "claude": "PreToolUse/PostToolUse hooks",
        "cursor": "beforeShellExecution hook",
        "gemini": "BeforeTool/AfterTool hooks",
        "windsurf": "beforeShellExecution hook",
        "pi": "TypeScript extension",
    }.get(agent_id, "Unknown")

    if format_info:
        details.append(f"Hook format: {format_info}")

    if issues:
        return CheckResult(
            agent.name,
            HealthStatus.WARNING,
            f"Issues: {'; '.join(issues)}",
            "\n".join(details) if verbose else None,
        )

    return CheckResult(
        agent.name,
        HealthStatus.OK,
        f"{agent.name} is configured",
        "\n".join(details) if verbose else None,
    )
