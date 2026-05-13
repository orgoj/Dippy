"""
Dippy doctor command for installation and configuration diagnosis.

Provides comprehensive health checks for Dippy installation including
PATH configuration, hook status, config validation, log file health,
and agent-specific diagnostics.
"""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path

from dippy.cli.agents import AGENTS
from dippy.cli.hooks import (
    HOOK_COMMANDS,
    _codex_config_toml_path,
    _codex_feature_flag_enabled,
    _get_hook_command_for_agent,
    _has_dippy_hook,
)


class HealthStatus(Enum):
    """Health status levels with corresponding indicators and exit codes."""

    OK = ("ok", 0, "+", "OK")
    WARNING = ("warning", 1, "?", "WARNING")
    CRITICAL = ("critical", 2, "!", "CRITICAL")

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
    fix_command: str | None = None
    matchers: dict[str, list[str]] = field(default_factory=dict)

    def to_dict(self) -> dict:
        """Convert to dictionary for JSON output."""
        return {
            "name": self.name,
            "status": self.status.level,
            "message": self.message,
            "details": self.details,
            "fix_command": self.fix_command,
            "matchers": self.matchers,
        }

    def display(self, verbose: bool = False) -> None:
        """Display the check result to stdout."""
        print(f"[{self.status.symbol}] {self.name}: {self.message}")
        if self.details and verbose:
            for line in self.details.split("\n"):
                print(f"    {line}")
        if self.fix_command:
            print(f"    Fix: {self.fix_command}")
        if self.matchers and verbose:
            for hook_type, patterns in self.matchers.items():
                print(f"    {hook_type}:")
                for pattern in patterns:
                    print(f"      - {pattern}")


def run(
    agent: str | None = None,
    verbose: bool = False,
    cwd: str | None = None,
    json_output: bool = False,
    quiet: bool = False,
    fix: bool = False,
) -> int:
    """Run diagnostic checks and return overall exit code.

    Args:
        agent: Optional agent ID to filter checks for a specific agent
        verbose: Show detailed diagnostic information
        cwd: Current working directory
        json_output: Output as structured JSON
        quiet: Minimal output, exit code only
        fix: Auto-repair common issues

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
    hook_checks = check_hook_status(cwd_path, verbose)
    checks.extend(hook_checks)

    # 3. Config validation
    checks.append(check_config_validation(cwd_path))

    # 4. Log health check
    checks.append(check_log_health(verbose))

    # 5. Agent-specific check (if --agent specified)
    if agent:
        checks.append(check_agent_specific(agent, cwd_path, verbose))

    # Auto-fix if requested
    if fix:
        fixed = apply_auto_fixes(checks, cwd_path)
        if fixed:
            # Re-run checks after fixing
            checks = []
            checks.append(check_installation())
            checks.extend(check_hook_status(cwd_path, verbose))
            checks.append(check_config_validation(cwd_path))
            checks.append(check_log_health(verbose))
            if agent:
                checks.append(check_agent_specific(agent, cwd_path, verbose))

    # Calculate summary
    summary = {
        "ok": sum(1 for c in checks if c.status == HealthStatus.OK),
        "warnings": sum(1 for c in checks if c.status == HealthStatus.WARNING),
        "critical": sum(1 for c in checks if c.status == HealthStatus.CRITICAL),
    }

    # Output based on format
    if quiet:
        return max((c.status.exit_code for c in checks), default=0)

    if json_output:
        print(_format_json_output(checks, summary))
    else:
        _format_text_output(checks, verbose, summary)

    # Return highest severity exit code
    max_status = max((check.status for check in checks), key=lambda s: s.exit_code)
    return max_status.exit_code


def _format_text_output(
    checks: list[CheckResult], verbose: bool, summary: dict
) -> None:
    """Format checks as text output.

    Args:
        checks: List of check results
        verbose: Show detailed information
        summary: Summary counts
    """
    print("Dippy Installation Check")
    print("=" * 40)
    print()

    for check in checks:
        check.display(verbose)
        print()

    # Print summary
    parts = []
    if summary["ok"]:
        parts.append(f"{summary['ok']} OK")
    if summary["warnings"]:
        parts.append(f"{summary['warnings']} warnings")
    if summary["critical"]:
        parts.append(f"{summary['critical']} critical")
    print(f"Summary: {', '.join(parts) if parts else 'No checks'}")


def _format_json_output(checks: list[CheckResult], summary: dict) -> str:
    """Format checks as JSON output.

    Args:
        checks: List of check results
        summary: Summary counts

    Returns:
        JSON string
    """
    output = {
        "summary": summary,
        "checks": [c.to_dict() for c in checks],
        "overall_status": "ok"
        if summary["critical"] == 0 and summary["warnings"] == 0
        else ("warning" if summary["critical"] == 0 else "critical"),
    }
    return json.dumps(output, indent=2)


def apply_auto_fixes(checks: list[CheckResult], cwd_path: Path) -> bool:
    """Apply automatic fixes to common issues.

    Args:
        checks: List of check results
        cwd_path: Current working directory

    Returns:
        True if any fixes were applied
    """
    from dippy.cli.hooks import install as hooks_install

    fixed = False

    for check in checks:
        if check.fix_command and check.status == HealthStatus.WARNING:
            # Parse the fix command to extract agent and scope
            if "dippy hooks install" in check.fix_command:
                parts = check.fix_command.split()
                try:
                    agent_idx = parts.index("install") + 1
                    if agent_idx < len(parts):
                        agent = parts[agent_idx]
                        global_flag = "--global" in parts

                        print(f"Auto-fixing: {check.name}")
                        result = hooks_install(
                            agent=agent,
                            global_config=global_flag,
                            cwd=str(cwd_path),
                            force=True,
                            dry_run=False,
                        )
                        if result == 0:
                            fixed = True
                            print(f"  Fixed: {check.name}")
                        else:
                            print(f"  Failed to fix: {check.name}", file=sys.stderr)
                except (ValueError, IndexError):
                    pass

    return fixed


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
            fix_command="uv tool install dippy",
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
    home_dir = Path.home()

    # Check each agent with hook support
    for agent_id in ("claude", "gemini", "cursor", "windsurf", "codex"):
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

        # Check project config (only if not in home directory)
        project_path = None
        project_config = None
        check_project = cwd_path != home_dir
        if check_project:
            project_path = cwd_path / hook_config["project_config"]
            if project_path.exists():
                try:
                    with open(project_path) as f:
                        project_config = json.load(f)
                except (json.JSONDecodeError, IOError):
                    pass

        global_feature_flag = None
        project_feature_flag = None
        if agent_id == "codex":
            global_feature_flag = _codex_feature_flag_enabled(
                _codex_config_toml_path(global_config=True)
            )
            if check_project:
                project_feature_flag = _codex_feature_flag_enabled(
                    _codex_config_toml_path(global_config=False, cwd=str(cwd_path))
                )

        # Determine agent status
        agent_exists = global_config is not None or project_config is not None
        if agent_id == "codex":
            agent_exists = (
                agent_exists or global_feature_flag or bool(project_feature_flag)
            )

        if not agent_exists:
            # Agent not installed - show relevant paths only
            paths_list = [f"  {global_path}"]
            if check_project and project_path:
                paths_list.append(f"  {project_path}")
            results.append(
                CheckResult(
                    f"Hook: {agent_info.name}",
                    HealthStatus.WARNING,
                    "Not installed",
                    "Config not found at:\n" + "\n".join(paths_list),
                    fix_command=f"dippy hooks install {agent_id} --global",
                )
            )
            continue

        # Check for dippy hook in configs
        has_hook = False
        hook_type = None
        locations = []
        legacy_path = None
        matchers = {}

        # Check global config
        if global_config and _has_dippy_hook(global_config, agent_id):
            has_hook = True
            locations.append("global")
            config_str = json.dumps(global_config)
            if "dippy-hook" in config_str or "/dippy" in config_str:
                hook_type = "legacy (full path)"
                import re

                match = re.search(r'"command":\s*"([^"]*dippy[^"]*)"', config_str)
                if match:
                    legacy_path = match.group(1)

            # Extract matchers for verbose output
            if verbose:
                matchers = _extract_matchers_from_config(global_config, agent_id)

        # Check project config (only if not in home directory)
        if (
            check_project
            and project_config
            and _has_dippy_hook(project_config, agent_id)
        ):
            has_hook = True
            locations.append("project")
            if not legacy_path:
                config_str = json.dumps(project_config)
                if "dippy-hook" in config_str or "/dippy" in config_str:
                    hook_type = "legacy (full path)"
                    import re

                    match = re.search(r'"command":\s*"([^"]*dippy[^"]*)"', config_str)
                    if match:
                        legacy_path = match.group(1)

            # Extract matchers for verbose output
            if verbose and not matchers:
                matchers = _extract_matchers_from_config(project_config, agent_id)

        if not has_hook:
            results.append(
                CheckResult(
                    f"Hook: {agent_info.name}",
                    HealthStatus.WARNING,
                    "Agent present, hook not installed",
                    f"Install with: dippy hooks install {agent_id} --global",
                    fix_command=f"dippy hooks install {agent_id} --global",
                )
            )
        elif hook_type == "legacy (full path)":
            expected_command = _get_hook_command_for_agent(agent_id)
            details = (
                f"Legacy command: {legacy_path}\nExpected command: {expected_command}"
                if legacy_path
                else f"Update with: dippy hooks install {agent_id} --global"
            )
            results.append(
                CheckResult(
                    f"Hook: {agent_info.name}",
                    HealthStatus.WARNING,
                    f"Legacy hook ({', '.join(locations)})",
                    details,
                    fix_command=f"dippy hooks install {agent_id} --global --force",
                    matchers=matchers,
                )
            )
        elif agent_id == "codex" and (
            ("global" in locations and global_feature_flag is False)
            or ("project" in locations and project_feature_flag is False)
        ):
            missing_scopes = []
            if "global" in locations and global_feature_flag is False:
                missing_scopes.append("global")
            if "project" in locations and project_feature_flag is False:
                missing_scopes.append("project")
            results.append(
                CheckResult(
                    f"Hook: {agent_info.name}",
                    HealthStatus.WARNING,
                    f"Hook installed, Codex feature flag missing ({', '.join(missing_scopes)})",
                    "Run install again to enable hooks in config.toml",
                    fix_command=f"dippy hooks install {agent_id} --global --force",
                    matchers=matchers,
                )
            )
        else:
            details = None
            if verbose:
                details = f"Location: {', '.join(locations)}\nConfig: {global_path if 'global' in locations else project_path}"
            results.append(
                CheckResult(
                    f"Hook: {agent_info.name}",
                    HealthStatus.OK,
                    f"Installed ({', '.join(locations)})",
                    details,
                    matchers=matchers,
                )
            )

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

        results.append(
            CheckResult(
                "Hook: pi-mono",
                HealthStatus.OK,
                f"Extension installed ({file_type})",
                details if verbose else target_info,
            )
        )
    else:
        results.append(
            CheckResult(
                "Hook: pi-mono",
                HealthStatus.WARNING,
                "Extension not found",
                f"Expected: {pi_extension}",
            )
        )

    return results


def _extract_matchers_from_config(config: dict, agent_id: str) -> dict[str, list[str]]:
    """Extract matcher patterns from an agent's config.

    Args:
        config: Parsed configuration dict
        agent_id: Agent ID

    Returns:
        Dictionary mapping hook types to matcher lists
    """
    matchers = {}

    # Get hooks section
    hooks = config.get("hooks", {})

    # Different agents use different hook names
    if agent_id == "claude":
        hook_names = [("PreToolUse", "PreToolUse"), ("PostToolUse", "PostToolUse")]
    elif agent_id == "gemini":
        hook_names = [("BeforeTool", "BeforeTool"), ("AfterTool", "AfterTool")]
    elif agent_id == "codex":
        hook_names = [
            ("PreToolUse", "PreToolUse"),
            ("PermissionRequest", "PermissionRequest"),
            ("PostToolUse", "PostToolUse"),
            ("Stop", "Stop"),
        ]
    else:
        return matchers

    for config_key, display_name in hook_names:
        if config_key in hooks:
            for hook_entry in hooks[config_key]:
                if agent_id == "codex":
                    if "matchers" in hook_entry:
                        if display_name not in matchers:
                            matchers[display_name] = []
                        tool_name = hook_entry["matchers"].get("tool_name", "(all)")
                        matchers[display_name].append(tool_name)
                elif "matcher" in hook_entry:
                    if display_name not in matchers:
                        matchers[display_name] = []
                    matchers[display_name].append(hook_entry["matcher"])

    return matchers


def check_config_validation(cwd_path: Path) -> CheckResult:
    """Validate Dippy configuration files."""
    from dippy.core.config import ConfigError, load_config

    errors = []
    warnings = []
    configs_found = []

    # Check global config
    global_config = Path.home() / ".dippy" / "config"
    if global_config.exists():
        configs_found.append("global")
        try:
            load_config(cwd_path, config_path=str(global_config))
        except ConfigError as e:
            errors.append(f"Global config: {_format_config_error(e, global_config)}")
    else:
        warnings.append("Global config not found (~/.dippy/config)")

    # Check project config
    project_config = cwd_path / ".dippy"
    if project_config.exists():
        configs_found.append("project")
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

    if warnings:
        details = "\n".join(warnings)
        if configs_found:
            details += f"\nValid configs: {', '.join(configs_found)}"
        return CheckResult(
            "Configuration",
            HealthStatus.WARNING,
            f"{len(warnings)} warning(s)",
            details,
        )

    if not configs_found:
        return CheckResult(
            "Configuration",
            HealthStatus.WARNING,
            "No config found",
            "Create ~/.dippy/config or .dippy file to customize rules",
        )

    return CheckResult(
        "Configuration",
        HealthStatus.OK,
        f"Configuration valid ({', '.join(configs_found)})",
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
        (Path.home() / ".codex" / "hook-approvals.log", "OpenAI Codex CLI"),
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
                issues.append(
                    f"{name}: cannot write to log directory ({log_path.parent})"
                )

        # Check log file size
        if log_path.exists():
            size_mb = log_path.stat().st_size / (1024 * 1024)
            if size_mb > 10:
                issues.append(
                    f"{name}: log file is {size_mb:.1f}MB (consider rotation)"
                )
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
    matchers = {}

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
                    # Check if legacy by inspecting config
                    config_str = json.dumps(config)
                    if "dippy-hook" in config_str or "/dippy" in config_str:
                        details.append("Dippy hook: legacy (old 'dippy-hook')")
                        issues.append("Legacy hook detected - consider updating")
                    else:
                        details.append("Dippy hook: installed")
                        # Extract matchers for verbose output
                        if verbose:
                            matchers = _extract_matchers_from_config(config, agent_id)
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
        "codex": "PreToolUse/PermissionRequest/PostToolUse/Stop hooks + hooks feature flag",
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
            matchers=matchers if verbose else {},
        )

    return CheckResult(
        agent.name,
        HealthStatus.OK,
        f"{agent.name} is configured",
        "\n".join(details) if verbose else None,
        matchers=matchers if verbose else {},
    )
