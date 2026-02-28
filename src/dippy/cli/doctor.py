"""
Dippy doctor command for installation and configuration diagnosis.

Provides comprehensive health checks for Dippy installation including
PATH configuration, hook status, config validation, log file health,
and agent-specific diagnostics.
"""

from __future__ import annotations

import os
import shutil
import sys
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from typing import Literal

from dippy.cli.agents import AGENTS, detect_agents


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
        if verbose and self.details:
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

    # 2. Hook status check
    if agent:
        checks.append(check_agent_hook_status(agent))
    else:
        checks.append(check_hook_status())

    # 3. Config validation
    checks.append(check_config_validation(cwd_path))

    # 4. Log health check
    checks.append(check_log_health())

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
            "Install Dippy or add it to your PATH",
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


def check_hook_status() -> CheckResult:
    """Check status of hooks for all agents."""
    installed = detect_agents()

    if not installed:
        return CheckResult(
            "Hooks",
            HealthStatus.WARNING,
            "No AI coding assistants detected",
            "Install Claude Code, Cursor, or Gemini CLI to use Dippy hooks",
        )

    agent_names = ", ".join(sorted(info.name for info in installed.values()))
    return CheckResult(
        "Hooks",
        HealthStatus.OK,
        f"Found {len(installed)} agent(s): {agent_names}",
        None,
    )


def check_agent_hook_status(agent_id: str) -> CheckResult:
    """Check hook status for a specific agent."""
    agent = AGENTS.get(agent_id)
    if not agent:
        return CheckResult(
            "Hooks",
            HealthStatus.CRITICAL,
            f"Unknown agent: {agent_id}",
            f"Valid agents: {', '.join(AGENTS.keys())}",
        )

    if agent.is_installed():
        return CheckResult(
            "Hooks",
            HealthStatus.OK,
            f"{agent.name} is installed",
            f"Config: {agent.global_config}",
        )
    else:
        return CheckResult(
            "Hooks",
            HealthStatus.CRITICAL,
            f"{agent.name} not found",
            f"Expected config at: {agent.global_config}",
        )


def check_config_validation(cwd: Path) -> CheckResult:
    """Validate Dippy configuration files."""
    from dippy.core.config import ConfigError, load_config

    # Check global config
    global_errors = []
    try:
        load_config(Path.cwd(), config_path=None)
    except ConfigError as e:
        global_errors.append(str(e))

    # Check project config
    project_errors = []
    if (cwd / ".dippy").exists():
        try:
            load_config(cwd, config_path=None)
        except ConfigError as e:
            project_errors.append(str(e))

    if global_errors or project_errors:
        errors = []
        if global_errors:
            errors.append(f"Global: {'; '.join(global_errors)}")
        if project_errors:
            errors.append(f"Project: {'; '.join(project_errors)}")

        return CheckResult(
            "Configuration",
            HealthStatus.CRITICAL,
            "Config validation failed",
            "; ".join(errors),
        )

    return CheckResult(
        "Configuration",
        HealthStatus.OK,
        "Configuration is valid",
        None,
    )


def check_log_health() -> CheckResult:
    """Check health of Dippy log files."""
    log_paths = [
        Path.home() / ".claude" / "hook-approvals.log",
        Path.home() / ".dippy" / "audit.log",
    ]

    issues = []
    writable = []

    for log_path in log_paths:
        # Check if parent directory exists and is writable
        if log_path.parent.exists():
            # Try to check if writable
            test_file = log_path.parent / ".dippy_write_test"
            try:
                test_file.touch()
                test_file.unlink()
                writable.append(str(log_path.parent))
            except PermissionError:
                issues.append(f"{log_path.parent}: not writable")
            except OSError:
                issues.append(f"{log_path.parent}: cannot write")

    # Check log file size (warn if > 10MB)
    for log_path in log_paths:
        if log_path.exists():
            size_mb = log_path.stat().st_size / (1024 * 1024)
            if size_mb > 10:
                issues.append(f"{log_path}: {size_mb:.1f}MB (consider rotation)")

    if issues:
        return CheckResult(
            "Logs",
            HealthStatus.WARNING,
            "Log issues detected",
            "; ".join(issues),
        )

    if writable:
        return CheckResult(
            "Logs",
            HealthStatus.OK,
            "Log directories are writable",
            f"Writable: {', '.join(writable)}",
        )

    return CheckResult(
        "Logs",
        HealthStatus.OK,
        "Log files not created yet",
        "This is normal for new installations",
    )


def check_agent_specific(agent_id: str, cwd: Path, verbose: bool) -> CheckResult:
    """Run agent-specific diagnostic checks."""
    agent = AGENTS.get(agent_id)
    if not agent:
        return CheckResult(
            agent.name.capitalize(),
            HealthStatus.CRITICAL,
            f"Unknown agent: {agent_id}",
            None,
        )

    issues = []
    details = []

    # Check config exists
    if agent.is_installed():
        details.append(f"Config found: {agent.global_config}")

        # Check if project config exists
        project_config = cwd / agent.project_config
        if project_config.exists():
            details.append(f"Project config: {project_config}")
        else:
            details.append(f"No project config at: {project_config}")
    else:
        issues.append(f"{agent.name} not installed")

    # Check hook format compatibility
    hook_format_note = {
        "claude": "Uses Claude Code hook format (PreToolUse/PostToolUse)",
        "cursor": "Uses Cursor hook format (beforeShellExecution)",
        "gemini": "Uses Gemini CLI hook format (BeforeTool/AfterTool)",
        "pi": "Uses pi-mono extension format",
        "none": "No hook system (notifications only)",
    }.get(agent.hook_format, "")

    if hook_format_note:
        details.append(f"Hook format: {hook_format_note}")

    # Config format
    details.append(f"Config format: {agent.config_format.upper()}")

    if issues:
        return CheckResult(
            agent.name.capitalize(),
            HealthStatus.WARNING if agent.is_installed() else HealthStatus.CRITICAL,
            f"Issues found: {'; '.join(issues)}",
            "\n".join(details) if verbose else None,
        )

    return CheckResult(
        agent.name.capitalize(),
        HealthStatus.OK,
        f"{agent.name} is configured correctly",
        "\n".join(details) if verbose else None,
    )


# Import subprocess for installation check
import subprocess
