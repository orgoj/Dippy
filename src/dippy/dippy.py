#!/usr/bin/env python3
"""
Dippy - Approval autopilot for Claude Code, Gemini CLI, and Cursor.

A PreToolUse/BeforeTool/beforeShellExecution hook that auto-approves safe
commands while prompting for anything destructive. Stay in the flow.

Usage:
    Hook mode (stdin JSON):
        Claude Code: Add to ~/.claude/settings.json hooks configuration.
        Gemini CLI:  Add to ~/.gemini/settings.json with --gemini flag.
        Cursor:      Add to .cursor/hooks.json with --cursor flag.

    CLI mode (command validation):
        dippy --cmd 'rm -rf /'              # validate a command
        dippy --cmd 'ls -la' --json         # JSON output
        dippy --cmd 'git status' --cwd /path
        echo 'ls -la' | dippy --stdin       # read command from stdin

    Exit codes (CLI mode):
        0 = allow (command is safe)
        1 = deny (blocked by rule)
        2 = ask (needs user approval)

    See README.md for details.
"""

from __future__ import annotations

import argparse
import json
import logging
import os
import sys
from pathlib import Path

from dippy.core.analyzer import analyze
from dippy.core.config import (
    Config,
    ConfigError,
    configure_logging,
    load_config,
    log_decision,
    match_after_mcp,
    match_after_web,
    match_edit,
    match_read,
    match_mcp,
    match_web,
)
from dippy import __version__
from dippy.core.notifier import run_notifier, should_run_notifier

# === Mode Detection ===


def _env_flag(name: str) -> bool:
    """Check if an environment variable is truthy."""
    return os.environ.get(name, "").lower() in ("1", "true", "yes")


def _detect_mode_from_flags() -> str | None:
    """Detect mode from command-line flags or env vars. Returns None if not set."""
    if "--claude" in sys.argv or _env_flag("DIPPY_CLAUDE"):
        return "claude"
    if "--gemini" in sys.argv or _env_flag("DIPPY_GEMINI"):
        return "gemini"
    if "--cursor" in sys.argv or _env_flag("DIPPY_CURSOR"):
        return "cursor"
    if "--pi" in sys.argv or _env_flag("DIPPY_PI"):
        return "pi"
    if "--moltbot" in sys.argv or _env_flag("DIPPY_MOLTBOT"):
        return "moltbot"
    if "--codex" in sys.argv or _env_flag("DIPPY_CODEX"):
        return "codex"
    if "--windsurf" in sys.argv or _env_flag("DIPPY_WINDSURF"):
        return "windsurf"
    if "--pearai" in sys.argv or _env_flag("DIPPY_PEARAI"):
        return "pearai"
    return None


def _detect_mode_from_input(input_data: dict) -> str:
    """Auto-detect mode from input JSON structure."""
    # Cursor: {"command": "...", "cwd": "..."}
    if "command" in input_data and "tool_name" not in input_data:
        return "cursor"

    # Claude/Gemini: {"tool_name": "...", "tool_input": {...}}
    tool_name = input_data.get("tool_name", "")

    # Gemini uses "shell", "run_shell_command", etc.
    if tool_name in ("shell", "run_shell", "run_shell_command", "execute_shell"):
        return "gemini"

    # Claude uses "Bash" and MCP tools use "mcp__*" prefix
    if tool_name and tool_name != "Bash" and not tool_name.startswith("mcp__"):
        logging.warning(f"Unknown tool_name '{tool_name}', defaulting to Claude mode")
    return "claude"


# Initial mode from flags/env (may be overridden by auto-detect)
_EXPLICIT_MODE = _detect_mode_from_flags()
MODE = _EXPLICIT_MODE or "claude"  # Default for logging setup


# === Logging Setup ===


def _get_log_file() -> Path:
    """Get log file path based on mode."""
    if MODE == "gemini":
        return Path.home() / ".gemini" / "hook-approvals.log"
    if MODE == "cursor":
        return Path.home() / ".cursor" / "hook-approvals.log"
    if MODE == "pi":
        return Path.home() / ".pi" / "hook-approvals.log"
    if MODE == "moltbot":
        return Path.home() / ".moltbot" / "hook-approvals.log"
    if MODE == "codex":
        return Path.home() / ".codex" / "hook-approvals.log"
    if MODE == "windsurf":
        return Path.home() / ".windsurf" / "hook-approvals.log"
    if MODE == "pearai":
        return Path.home() / ".pearai" / "hook-approvals.log"
    return Path.home() / ".claude" / "hook-approvals.log"


_LOG_FORMAT = "%(asctime)s [%(levelname)s] %(message)s"
_LOG_DATEFMT = "%Y-%m-%d %H:%M:%S"


def setup_logging():
    """Add a file handler for the detected mode's log directory.

    Called after mode is finalized so the log file goes to the correct
    directory (~/.claude/, ~/.cursor/, etc.). Fails silently.
    """
    try:
        log_file = _get_log_file()
        log_file.parent.mkdir(parents=True, exist_ok=True)
        handler = logging.FileHandler(str(log_file))
        handler.setLevel(logging.INFO)
        handler.setFormatter(logging.Formatter(_LOG_FORMAT, datefmt=_LOG_DATEFMT))
        logging.getLogger().addHandler(handler)
    except (OSError, PermissionError):
        pass  # Logging is optional - don't crash if we can't write


# === Response Helpers ===


def approve(
    reason: str = "all commands safe",
    config: Config | None = None,
    tool_name: str | None = None,
    command: str | None = None,
    hook_event: str | None = None,
) -> dict:
    """Return approval response."""
    logging.info(f"APPROVED: {reason}")
    note = (
        run_notifier(config)
        if config and should_run_notifier(config, tool_name=tool_name, command=command)
        else None
    )

    if MODE == "gemini":
        res = {
            "decision": "allow",
            "reason": f"🐤 {reason}",
            "systemMessage": f"🐤 {reason}",
            "continue": True,
        }
        if note:
            res["additionalContext"] = note
        return res
    if MODE == "codex":
        if hook_event == "PermissionRequest":
            res = {
                "hookSpecificOutput": {
                    "hookEventName": "PermissionRequest",
                    "decision": {"behavior": "allow"},
                }
            }
            if note:
                res["systemMessage"] = note
            return res
        # Codex PreToolUse allow does not approve execution; empty output just
        # lets the later PermissionRequest hook make the approval decision.
        if note:
            return {"systemMessage": note}
        return None
    if MODE == "cursor":
        # Include both snake_case (v2.0+) and camelCase (v1.7.x) for compatibility
        msg = f"🐤 {reason}"
        res = {
            "permission": "allow",
            "user_message": msg,
            "agent_message": msg,
            "userMessage": msg,
            "agentMessage": msg,
        }
        if note:
            res["agent_message"] = f"{msg}\n\n{note}"
            res["agentMessage"] = f"{msg}\n\n{note}"
        return res

    res = {
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow",
            "permissionDecisionReason": f"🐤 {reason}",
        }
    }
    if note:
        res["systemMessage"] = note
    return res


def ask(
    reason: str = "needs approval",
    config: Config | None = None,
    tool_name: str | None = None,
    command: str | None = None,
    hook_event: str | None = None,
) -> dict:
    """Return ask response to prompt user for confirmation."""
    logging.info(f"ASK: {reason}")
    note = (
        run_notifier(config)
        if config and should_run_notifier(config, tool_name=tool_name, command=command)
        else None
    )

    if MODE == "gemini":
        res = {
            "decision": "ask",
            "reason": f"🐤 {reason}",
            "systemMessage": f"🐤 {reason}",
            "continue": True,
        }
        if note:
            res["additionalContext"] = note
        return res
    if MODE == "codex":
        # Codex: permissionDecision "ask" fails open, just show systemMessage
        msg = f"🐤 {reason}"
        if note:
            return {"systemMessage": f"{msg}\n\n{note}"}
        return {"systemMessage": msg}
    if MODE == "cursor":
        # Include both snake_case (v2.0+) and camelCase (v1.7.x) for compatibility
        msg = f"🐤 {reason}"
        res = {
            "permission": "ask",
            "user_message": msg,
            "agent_message": msg,
            "userMessage": msg,
            "agentMessage": msg,
        }
        if note:
            res["agent_message"] = f"{msg}\n\n{note}"
            res["agentMessage"] = f"{msg}\n\n{note}"
        return res

    res = {
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "ask",
            "permissionDecisionReason": f"🐤 {reason}",
        }
    }
    if note:
        res["systemMessage"] = note
    return res


def deny(
    reason: str = "denied by config",
    config: Config | None = None,
    tool_name: str | None = None,
    command: str | None = None,
    hook_event: str | None = None,
) -> dict:
    """Return deny response to block the command."""
    logging.info(f"DENY: {reason}")
    note = (
        run_notifier(config)
        if config and should_run_notifier(config, tool_name=tool_name, command=command)
        else None
    )

    if MODE == "gemini":
        # Gemini CLI: decision "deny" is the standard way to block a tool.
        # It provides feedback to the agent without stopping the loop.
        res = {
            "decision": "deny",
            "reason": f"🐤 {reason}",
            "systemMessage": f"🐤 {reason}",
        }
        if note:
            res["additionalContext"] = note
        return res
    if MODE == "codex":
        # Codex: exit code 2 with stderr for blocking
        msg = f"🐤 {reason}"
        if note:
            msg = f"{msg}\n\n{note}"
        print(msg, file=sys.stderr)
        sys.exit(2)
    if MODE == "cursor":
        # Include both snake_case (v2.0+) and camelCase (v1.7.x) for compatibility
        msg = f"🐤 {reason}"
        res = {
            "permission": "deny",
            "user_message": msg,
            "agent_message": msg,
            "userMessage": msg,
            "agentMessage": msg,
        }
        if note:
            res["agent_message"] = f"{msg}\n\n{note}"
            res["agentMessage"] = f"{msg}\n\n{note}"
        return res

    res = {
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": f"🐤 {reason}",
        }
    }
    # For Claude deny, we can't easily inject context while denying in PreToolUse
    # but we can show systemMessage to user.
    if note:
        res["systemMessage"] = note
    return res


def pass_(
    reason: str = "passing through",
    config: Config | None = None,
    tool_name: str | None = None,
    command: str | None = None,
    hook_event: str | None = None,
) -> dict:
    """Return empty response to let Claude handle permissions with its default behavior."""
    logging.info(f"PASS: {reason}")
    note = (
        run_notifier(config)
        if config and should_run_notifier(config, tool_name=tool_name, command=command)
        else None
    )

    if MODE == "gemini":
        res = {"decision": "allow", "reason": f"🐤 {reason}", "continue": True}
        if note:
            res["additionalContext"] = note
        return res
    if MODE == "codex":
        # Codex: exit 0 with no output is treated as success
        if note:
            return {"systemMessage": note}
        return None

    if note:
        return {"systemMessage": note}
    return {}


# === Askpass Support ===


def _run_askpass(
    config: Config,
    command: str,
    cwd: str,
    rule: str | None,
    message: str | None,
    tool: str | None,
    source: str | None = None,
) -> str:
    """Run external askpass program for GUI approval.

    Args:
        config: Loaded configuration with askpass settings.
        command: The command being approved.
        cwd: Current working directory.
        rule: The rule pattern that matched (if any).
        message: The rule message (if any).
        tool: The tool name (Bash, Write, etc.).
        source: The config file where the rule was defined.

    Returns:
        "allow" if exit 0, "deny" if exit 1, "ask" for any other case
        (timeout, error, exit 2+, or no askpass configured).
    """
    import subprocess

    # Check for askpass program (env var overrides config)
    askpass = os.environ.get("DIPPY_ASKPASS")
    if askpass:
        askpass_path = Path(askpass)
    elif config.askpass:
        askpass_path = config.askpass
    else:
        return "ask"  # No askpass configured

    # Set up environment variables
    env = os.environ.copy()
    env["DIPPY_COMMAND"] = command
    env["DIPPY_CWD"] = cwd
    if rule:
        env["DIPPY_RULE"] = rule
    if message:
        env["DIPPY_MESSAGE"] = message
    if tool:
        env["DIPPY_TOOL"] = tool

    # Prepare JSON input
    stdin_data = json.dumps(
        {
            "command": command,
            "cwd": cwd,
            "rule": rule,
            "message": message,
            "tool": tool,
            "source": source,
        }
    )

    try:
        result = subprocess.run(
            [str(askpass_path)],
            input=stdin_data,
            env=env,
            timeout=config.askpass_timeout,
            capture_output=True,
            text=True,
        )
        if result.returncode == 0:
            return "allow"
        elif result.returncode == 1:
            return "deny"
        else:
            return "ask"  # Exit 2+ = fallback to Claude dialog
    except subprocess.TimeoutExpired:
        logging.warning(f"Askpass timeout after {config.askpass_timeout}s")
        return "ask"
    except FileNotFoundError:
        logging.warning(f"Askpass program not found: {askpass_path}")
        return "ask"
    except PermissionError:
        logging.warning(f"Askpass program not executable: {askpass_path}")
        return "ask"
    except OSError as e:
        logging.warning(f"Askpass error: {e}")
        return "ask"


# === Main Logic ===


def check_command(
    command: str, config: Config, cwd: Path, hook_event: str | None = None
) -> dict:
    """
    Main entry point: check if a command should be approved.

    Uses a single recursive walk of the bash AST to analyze all constructs.
    Returns a hook response dict.
    """
    result = analyze(command, config, cwd)

    log_decision(
        "allow" if result.action == "allow" else result.action,
        result.reason,
        command=command,
        cwd=cwd,
        context_flags=result.context_flags,
        agent=MODE,
        suggestion=result.suggestion,
    )

    if result.action == "allow":
        return approve(
            result.reason, config=config, command=command, hook_event=hook_event
        )
    elif result.action == "deny":
        return deny(result.reason, config=config, command=command, hook_event=hook_event)
    elif result.action == "pass":
        return pass_(
            result.reason, config=config, command=command, hook_event=hook_event
        )
    else:
        return ask(result.reason, config=config, command=command, hook_event=hook_event)


def post_tool_response(
    message: str,
    config: Config | None = None,
    tool_name: str | None = None,
    command: str | None = None,
) -> dict:
    """Return PostToolUse response with feedback for Claude."""
    note = (
        run_notifier(config)
        if config and should_run_notifier(config, tool_name=tool_name, command=command)
        else None
    )

    context = f"🐤 {message}"
    if note:
        context = f"{context}\n\n{note}"

    if MODE == "gemini":
        return {
            "decision": "allow",
            "reason": f"🐤 {message}",
            "additionalContext": context,
            "continue": True,
        }

    return {
        "hookSpecificOutput": {
            "hookEventName": "PostToolUse",
            "additionalContext": context,
        }
    }


def handle_post_tool_use(command: str, config: Config, cwd: Path) -> None:
    """Handle PostToolUse hook - output feedback message if rule matches."""
    from dippy.core.config import match_after
    from dippy.core.parser import tokenize

    words = tokenize(command)
    message = match_after(words, config, cwd)
    if message or config.notifier_command:
        # If no message from rule, but notifier exists, still call it
        resp = post_tool_response(
            message or "Notification", config=config, command=command
        )
        # Only print if we actually have something to say (message or note)
        if message or (
            resp.get("hookSpecificOutput", {}).get("additionalContext")
            or resp.get("additionalContext")
        ):
            print(json.dumps(resp))


# === MCP Tool Handling ===


def is_mcp_tool(name: str) -> bool:
    """Check if a tool name is an MCP tool."""
    return name.startswith("mcp__")


def check_mcp_tool(tool_name: str, config: Config) -> dict:
    """Check if an MCP tool should be approved based on config rules.

    Args:
        tool_name: MCP tool name (e.g., "mcp__github__get_issue").
        config: Loaded configuration.

    Returns:
        Hook response dict, or empty dict if no rules match (defer to default).
    """
    match = match_mcp(tool_name, config)
    if match is None:
        return {}  # No rules match - defer to Claude's default behavior
    reason = match.message if match.message else f"[{match.pattern}]"
    log_decision(match.decision, reason, rule=match.pattern, agent=MODE)
    if match.decision == "allow":
        return approve(reason, config=config, tool_name=tool_name)
    elif match.decision == "deny":
        return deny(reason, config=config, tool_name=tool_name)
    else:
        return ask(reason, config=config, tool_name=tool_name)


def handle_mcp_post_tool_use(tool_name: str, config: Config) -> None:
    """Handle PostToolUse hook for MCP tools - output feedback if rule matches."""
    message = match_after_mcp(tool_name, config)
    if message or config.notifier_command:
        resp = post_tool_response(
            message or "Notification", config=config, tool_name=tool_name
        )
        print(json.dumps(resp))


# === WebSearch Tool Handling ===


def check_web_tool(query: str, config: Config) -> dict:
    """Check if a WebSearch tool should be approved based on config rules.

    Args:
        query: WebSearch query string.
        config: Loaded configuration.

    Returns:
        Hook response dict, or empty dict if no rules match (defer to default).
    """
    match = match_web(query, config)
    if match is None:
        return {}  # No rules match - defer to Claude's default behavior
    reason = match.message if match.message else f"[{match.pattern}]"
    log_decision(match.decision, reason, rule=match.pattern, agent=MODE)
    if match.decision == "allow":
        return approve(reason, config=config, tool_name="WebSearch")
    elif match.decision == "deny":
        return deny(reason, config=config, tool_name="WebSearch")
    else:
        return ask(reason, config=config, tool_name="WebSearch")


def handle_web_post_tool_use(query: str, config: Config) -> None:
    """Handle PostToolUse hook for WebSearch - output feedback if rule matches."""
    message = match_after_web(query, config)
    if message or config.notifier_command:
        resp = post_tool_response(
            message or "Notification", config=config, tool_name="WebSearch"
        )
        print(json.dumps(resp))


# === Hook Entry Point ===

# Tool names that indicate shell/bash commands
SHELL_TOOL_NAMES = frozenset(
    {
        "Bash",  # Claude Code
        "bash",  # pi-mono
        "exec",  # moltbot
        "shell",  # Gemini CLI
        "run_shell",  # Gemini CLI alternate
        "run_shell_command",  # Gemini CLI official name
        "execute_shell",  # Gemini CLI alternate
    }
)

# Tool names that indicate file operations
FILE_TOOL_NAMES = frozenset(
    {
        "Write",
        "Edit",
        "MultiEdit",
        "Read",
        "LS",
        "Glob",
        "Grep",
        "Search",
        "write",  # moltbot / pi-mono
        "edit",  # moltbot / pi-mono
        "read",  # moltbot / pi-mono
        "write_file",
        "replace",
        "read_file",
        "read_many_files",
    }
)


def check_file_tool(tool_name: str, file_path: str, config: Config, cwd: Path) -> dict:
    """Check if a file operation should be approved based on edit rules.

    Args:
        tool_name: Tool name (Write, Edit, MultiEdit, Read, write_file, replace, read_file).
        file_path: Absolute path to the file being edited/read.
        config: Loaded configuration.
        cwd: Current working directory.

    Returns:
        Hook response dict, or empty dict if no rules match (defer to default).
    """
    if tool_name in ("Read", "read_file", "LS", "Glob", "Grep", "Search"):
        match = match_read(file_path, config, cwd)
    else:
        match = match_edit(file_path, config, cwd)

    if match is None:
        return {}  # No rules match - defer to Claude's default behavior

    reason = match.message if match.message else f"[{match.pattern}]"
    log_decision(
        match.decision,
        rule=match.pattern,
        tool=tool_name,
        file_path=file_path,
        cwd=cwd,
        agent=MODE,
    )

    if match.decision == "allow":
        return approve(reason, config=config, tool_name=tool_name)
    elif match.decision == "deny":
        return deny(reason, config=config, tool_name=tool_name)
    else:
        return ask(reason, config=config, tool_name=tool_name)


# === CLI Mode ===

# Exit codes for CLI mode
EXIT_ALLOW = 0
EXIT_DENY = 1
EXIT_ASK = 2


def parse_cli_args() -> argparse.Namespace:
    """Parse command-line arguments."""
    parser = argparse.ArgumentParser(
        prog="dippy",
        description="Validate shell commands against Dippy rules.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Exit codes (CLI mode):
  0 = allow (command is safe)
  1 = deny (blocked by rule)
  2 = ask (needs user approval)

Examples:
  dippy --cmd 'rm -rf /'
  dippy --cmd 'ls -la' --json
  echo 'git status' | dippy --stdin --cwd /repo

Subcommands:
  dippy hooks list     List installed hooks
  dippy doctor         Check installation and configuration
""",
    )

    # Add subparsers for subcommands
    subparsers = parser.add_subparsers(
        dest="subcommand",
        title="Subcommands",
        description="Available subcommands",
        metavar="<subcommand>",
    )

    # CLI mode arguments (for backward compatibility with --cmd/--stdin)
    input_group = parser.add_mutually_exclusive_group()
    input_group.add_argument("--cmd", metavar="COMMAND", help="Command to validate")
    input_group.add_argument(
        "--stdin",
        action="store_true",
        help="Read command from stdin (plain text, not JSON)",
    )

    parser.add_argument(
        "--cwd", metavar="PATH", help="Working directory (default: current)"
    )
    parser.add_argument(
        "--json", action="store_true", dest="json_output", help="Output as JSON"
    )
    parser.add_argument("--config", metavar="PATH", help="Config file path override")
    parser.add_argument("--agent", metavar="NAME", help="Agent name for audit log")
    parser.add_argument("--version", action="version", version=f"dippy {__version__}")
    parser.add_argument(
        "--remote", action="store_true", help="Remote context (skip local path checks)"
    )

    # Hook mode arguments (for backward compatibility)
    parser.add_argument("--claude", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--gemini", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--cursor", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--pi", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--moltbot", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--codex", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--windsurf", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--pearai", action="store_true", help=argparse.SUPPRESS)

    # === hooks subcommand ===
    hooks_parser = subparsers.add_parser(
        "hooks",
        help="Manage Dippy hooks for AI coding assistants",
        description="List, install, and manage Dippy hooks for Claude Code, Cursor, and other AI coding assistants.",
    )
    hooks_subparsers = hooks_parser.add_subparsers(
        dest="hooks_action",
        title="Hooks actions",
        description="Available hooks actions",
        metavar="<action>",
    )

    # hooks list
    list_parser = hooks_subparsers.add_parser(
        "list",
        help="List hook status (shows both global and project)",
        description="List Dippy hook status for all agents. Shows both global and project-local installation status.",
    )
    list_parser.add_argument(
        "--verbose",
        action="store_true",
        help="Show detailed information (matchers, commands, paths)",
    )
    list_parser.add_argument(
        "--json",
        action="store_true",
        help="Output as structured JSON",
    )
    list_parser.add_argument(
        "--quiet",
        action="store_true",
        help="Minimal output, exit code only",
    )

    # hooks install
    install_parser = hooks_subparsers.add_parser(
        "install",
        help="Install Dippy hooks for an agent",
        description="Install Dippy hooks for a specific AI coding assistant.",
    )
    install_parser.add_argument(
        "agent",
        nargs="?",
        choices=["claude", "gemini", "cursor", "windsurf", "codex"],
        help="Agent to install hooks for; omit with --all to install for all agents",
    )
    install_parser.add_argument(
        "--global",
        action="store_true",
        help="Install to global config instead of project-local",
    )
    install_parser.add_argument(
        "--force",
        action="store_true",
        help="Replace existing/legacy hooks",
    )
    install_parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Show what would be done without making changes",
    )
    install_parser.add_argument(
        "--no-backup",
        action="store_true",
        help="Skip config backup before install",
    )
    install_parser.add_argument(
        "--all",
        action="store_true",
        help="Install ALL supported hooks (PreToolUse, PostToolUse, Notification, Stop, etc.)",
    )

    # hooks uninstall
    uninstall_parser = hooks_subparsers.add_parser(
        "uninstall",
        help="Uninstall Dippy hooks for an agent",
        description="Remove Dippy hooks for a specific AI coding assistant.",
    )
    uninstall_parser.add_argument(
        "agent",
        choices=["claude", "gemini", "cursor", "windsurf", "codex"],
        help="Agent to uninstall hooks for",
    )
    uninstall_parser.add_argument(
        "--global",
        action="store_true",
        help="Uninstall from global config instead of project-local",
    )
    uninstall_parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Show what would be done without making changes",
    )

    # hooks setup-gemini-yolo
    yolo_parser = hooks_subparsers.add_parser(
        "setup-gemini-yolo",
        help="Enable Gemini YOLO mode for Pure Dippy Control",
        description="Configures Gemini's approvalMode to 'yolo' so that Dippy can take full control over command approvals without double-prompting.",
    )
    yolo_parser.add_argument(
        "--global",
        action="store_true",
        help="Update global settings.json instead of project-local",
    )
    yolo_parser.add_argument(
        "--disable",
        action="store_true",
        help="Disable YOLO mode (set back to 'default')",
    )
    yolo_parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Show what would be done without making changes",
    )

    # === doctor subcommand ===
    doctor_parser = subparsers.add_parser(
        "doctor",
        help="Diagnose Dippy installation and configuration",
        description="Check Dippy installation status, hook configurations, and common issues.",
    )
    doctor_parser.add_argument(
        "--agent",
        metavar="AGENT",
        choices=[
            "claude",
            "gemini",
            "cursor",
            "windsurf",
            "pi",
            "moltbot",
            "codex",
            "pearai",
        ],
        help="Show diagnostics for a specific agent",
    )
    doctor_parser.add_argument(
        "--verbose",
        action="store_true",
        help="Show detailed diagnostic information",
    )
    doctor_parser.add_argument(
        "--json",
        action="store_true",
        help="Output as structured JSON",
    )
    doctor_parser.add_argument(
        "--quiet",
        action="store_true",
        help="Minimal output, exit code only",
    )
    doctor_parser.add_argument(
        "--fix",
        action="store_true",
        help="Auto-repair common issues",
    )

    return parser.parse_args()


def cli_mode(args: argparse.Namespace) -> int:
    """
    CLI mode: validate a single command and exit with appropriate code.

    Returns exit code: 0=allow, 1=deny, 2=ask
    """
    # Get command from --cmd or --stdin
    if args.cmd:
        command = args.cmd
    elif args.stdin:
        command = sys.stdin.read().strip()
    else:
        # Should not happen due to argparse, but just in case
        print("Error: --cmd or --stdin required", file=sys.stderr)
        return EXIT_ASK  # Input error, not a deny

    if not command:
        print("Error: empty command", file=sys.stderr)
        return EXIT_ASK  # Input error, not a deny

    # Determine working directory
    cwd = Path(args.cwd).resolve() if args.cwd else Path.cwd()

    # Load config
    try:
        config = load_config(cwd, config_path=args.config)
    except ConfigError as e:
        if args.json_output:
            print(json.dumps({"decision": "ask", "reason": f"config error: {e}"}))
        else:
            print(f"ask: config error: {e}")
        return EXIT_ASK

    # Run notifier if configured
    note = (
        run_notifier(config)
        if config.notifier_command and should_run_notifier(config, command=command)
        else None
    )

    # Analyze command
    result = analyze(command, config, cwd, remote=args.remote)

    # Log decision to audit log if configured
    log_decision(
        result.action,
        message=result.reason,
        command=command,
        cwd=cwd,
        context_flags=result.context_flags,
        agent=MODE,
        suggestion=result.suggestion,
    )

    # Map 'pass' to 'ask' in CLI mode (pass means "let the AI decide" which
    # doesn't make sense in CLI context)
    action = result.action if result.action != "pass" else "ask"

    # Output result
    if args.json_output:
        res = {"decision": action, "reason": result.reason}
        if note:
            res["note"] = note
        print(json.dumps(res))
    else:
        print(f"{action}: {result.reason}")
        if note:
            print(f"\n{note}")

    # Return exit code
    if action == "allow":
        return EXIT_ALLOW
    elif action == "deny":
        return EXIT_DENY
    else:
        return EXIT_ASK


def handle_subcommand(args: argparse.Namespace) -> int:
    """Handle dippy subcommands.

    Args:
        args: Parsed command-line arguments

    Returns:
        Exit code: 0 for success, 1 for errors
    """
    if args.subcommand == "hooks":
        return handle_hooks_subcommand(args)
    elif args.subcommand == "doctor":
        return handle_doctor_subcommand(args)
    else:
        print(f"Unknown subcommand: {args.subcommand}", file=sys.stderr)
        return 1


def handle_hooks_subcommand(args: argparse.Namespace) -> int:
    """Handle the 'hooks' subcommand.

    Args:
        args: Parsed command-line arguments

    Returns:
        Exit code: 0 for success, 1 for errors
    """
    from dippy.cli.hooks import (
        install as hooks_install,
        list_hooks,
        uninstall as hooks_uninstall,
    )

    if args.hooks_action == "list":
        return list_hooks(
            cwd=getattr(args, "cwd", None),
            verbose=getattr(args, "verbose", False),
            json_output=getattr(args, "json", False),
            quiet=getattr(args, "quiet", False),
        )
    elif args.hooks_action == "install":
        return hooks_install(
            agent=args.agent,
            global_config=getattr(args, "global", False),
            cwd=getattr(args, "cwd", None),
            force=getattr(args, "force", False),
            dry_run=getattr(args, "dry_run", False),
            no_backup=getattr(args, "no_backup", False),
            all_hooks=getattr(args, "all", False),
        )
    elif args.hooks_action == "uninstall":
        return hooks_uninstall(
            agent=args.agent,
            global_config=getattr(args, "global", False),
            cwd=getattr(args, "cwd", None),
            dry_run=getattr(args, "dry_run", False),
        )
    elif args.hooks_action == "setup-gemini-yolo":
        from dippy.cli.hooks import setup_gemini_yolo
        return setup_gemini_yolo(
            global_config=getattr(args, "global", False),
            cwd=getattr(args, "cwd", None),
            disable=getattr(args, "disable", False),
            dry_run=getattr(args, "dry_run", False),
        )
    else:
        # No action specified, show help
        print(
            "Error: Please specify an action (list, install, uninstall)",
            file=sys.stderr,
        )
        return 1


def handle_doctor_subcommand(args: argparse.Namespace) -> int:
    """Handle the 'doctor' subcommand.

    Args:
        args: Parsed command-line arguments

    Returns:
        Exit code: 0 for success, 1 for errors
    """
    from dippy.cli.doctor import run as doctor_run

    return doctor_run(
        agent=getattr(args, "agent", None),
        verbose=getattr(args, "verbose", False),
        cwd=getattr(args, "cwd", None),
        json_output=getattr(args, "json", False),
        quiet=getattr(args, "quiet", False),
        fix=getattr(args, "fix", False),
    )


def main():
    """Main entry point for the hook."""
    global MODE

    # Parse arguments first to detect CLI mode
    args = parse_cli_args()

    # Handle subcommands
    if hasattr(args, "subcommand") and args.subcommand:
        sys.exit(handle_subcommand(args))

    # CLI mode: --cmd or --stdin (backward compatibility)
    if args.cmd or args.stdin:
        if args.agent:
            MODE = args.agent
        sys.exit(cli_mode(args))

    # Detect mode strictly from flags/env or default to claude
    MODE = _detect_mode_from_flags() or "claude"

    # Root passes INFO through to the file handler (added once the mode, and
    # thus the log directory, is known). The stderr fallback only surfaces
    # warnings/errors, so normal runs stay quiet there while early failures
    # (bad JSON, unknown tool) are still visible before the file handler exists.
    root = logging.getLogger()
    root.setLevel(logging.INFO)
    stderr_handler = logging.StreamHandler()
    stderr_handler.setLevel(logging.WARNING)
    stderr_handler.setFormatter(logging.Formatter(_LOG_FORMAT, datefmt=_LOG_DATEFMT))
    root.addHandler(stderr_handler)

    try:
        # Read hook input from stdin
        input_data = json.load(sys.stdin)

        # Auto-detect mode from input if no explicit flag/env was set
        if _EXPLICIT_MODE is None:
            MODE = _detect_mode_from_input(input_data)

        # Add file handler now that mode (and thus log directory) is known
        setup_logging()

        if _EXPLICIT_MODE is None:
            logging.info(f"Auto-detected mode: {MODE}")

        # Extract cwd from input
        # Cursor: top-level "cwd"
        # Claude Code: may be in tool_input or top-level
        cwd_str = input_data.get("cwd")
        if not cwd_str:
            tool_input = input_data.get("tool_input", {})
            cwd_str = tool_input.get("cwd")
        if cwd_str:
            cwd = Path(cwd_str).resolve()
        else:
            cwd = Path.cwd()

        # Load config (fails hard on errors)
        try:
            config = load_config(cwd)
            configure_logging(config)
        except ConfigError as e:
            logging.error(f"Config error: {e}")
            if MODE == "gemini":
                print(json.dumps({"decision": "allow", "reason": f"config error: {e}"}))
            else:
                print(json.dumps(ask(f"config error: {e}")))
            return

        # Detect hook event type (Claude Code / Gemini CLI)
        hook_event = input_data.get("hook_event_name", "PreToolUse")

        # Notification hook handling (idle_prompt, etc.)
        if hook_event == "Notification":
            import subprocess

            notification_type = input_data.get("notification_type", "")

            if notification_type == "idle_prompt" and config.idle_notifier_command:
                # Get values from hook
                title = input_data.get("title", "Claude Code")
                message = input_data.get("message", "Claude is waiting for your input")
                cwd = input_data.get("cwd", "")

                # Expand the command template with all placeholders
                expanded_command = expand_template(
                    config.idle_notifier_command,
                    title=title,
                    message=message,
                    cwd=cwd,
                    notification_type=notification_type,
                )

                # Run expanded command via shell
                try:
                    subprocess.run(
                        expanded_command,
                        shell=True,
                        capture_output=True,
                        text=True,
                        timeout=30,
                    )
                    logging.info(f"Idle notification: {title}")
                except subprocess.TimeoutExpired as e:
                    stderr = e.stderr if e.stderr else ""
                    logging.warning(
                        f"Idle notifier timed out: {expanded_command[:100]}"
                        + (f" | stderr: {stderr[:100]}" if stderr else "")
                    )
                except Exception as e:
                    logging.warning(f"Idle notifier failed: {e}")

            # Return empty response for Notification hooks
            print(json.dumps({}))
            return

        # Stop hook handling (Idle mode)
        if hook_event in ("Stop", "SubagentStop", "AfterAgent"):
            logging.info(f"Stop hook: {hook_event}")
            note = run_notifier(config, idle=True)
            if note:
                if MODE == "gemini":
                    print(
                        json.dumps(
                            {
                                "decision": "allow",
                                "reason": "New notification received",
                                "additionalContext": note,
                                "continue": True,
                            }
                        )
                    )
                elif MODE == "codex":
                    # Codex Stop: decision "block" means continue
                    print(json.dumps({"decision": "block", "reason": note}))
                else:
                    # Claude Code: return decision "block" to force continuation
                    print(
                        json.dumps(
                            {"decision": "block", "reason": note, "continue": True}
                        )
                    )
            else:
                # No notification, allow stop
                if MODE == "gemini":
                    print(json.dumps({"decision": "allow", "continue": False}))
                elif MODE == "codex":
                    # Codex: continue=false allows normal stop
                    print(json.dumps({"continue": False}))
                else:
                    print(json.dumps({"decision": "approve", "continue": True}))
            return

        # Normalize Gemini events to Claude names for internal routing
        if hook_event == "BeforeTool":
            hook_event = "PreToolUse"
        elif hook_event == "AfterTool":
            hook_event = "PostToolUse"

        # Extract command based on mode
        # Cursor: {"command": "...", "cwd": "..."}
        # Claude/Gemini: {"tool_name": "...", "tool_input": {"command": "..."}}

        # Initialize context variables for safe exception handling
        command = ""
        tool_name = None
        file_path = ""
        query = ""

        if MODE == "cursor":
            # Cursor sends command directly (beforeShellExecution hook)
            command = input_data.get("command", "")
            tool_name = None
        else:
            # Claude Code and Gemini CLI use tool_name/tool_input format
            tool_name = input_data.get("tool_name", "")
            tool_input = input_data.get("tool_input", {})

            # Check if this is an MCP tool
            if is_mcp_tool(tool_name):
                # Check for bypass permissions mode first
                if hook_event != "PostToolUse":
                    permission_mode = input_data.get("permission_mode", "default")
                    if permission_mode in ("bypassPermissions", "dontAsk"):
                        logging.info(f"Bypass mode ({permission_mode}): {tool_name}")
                        log_decision(
                            "allow", message=permission_mode, tool=tool_name, agent=MODE
                        )
                        _emit(approve(permission_mode, hook_event=hook_event))
                        return
                # Handle MCP tool
                if hook_event == "PostToolUse":
                    logging.info(f"PostToolUse MCP: {tool_name}")
                    handle_mcp_post_tool_use(tool_name, config)
                else:
                    logging.info(f"Checking MCP: {tool_name}")
                    result = check_mcp_tool(tool_name, config)
                    if not result:
                        log_decision(
                            "pass",
                            message="no matching rule",
                            tool=tool_name,
                            agent=MODE,
                        )
                    print(json.dumps(result))
                return

            # Check if this is a WebSearch tool
            if tool_name in ("WebSearch", "google_web_search"):
                query = tool_input.get("query") or tool_input.get("q") or ""
                # Check for bypass permissions mode first
                if hook_event != "PostToolUse":
                    permission_mode = input_data.get("permission_mode", "default")
                    if permission_mode in ("bypassPermissions", "dontAsk"):
                        logging.info(f"Bypass mode ({permission_mode}): {tool_name}")
                        log_decision(
                            "allow",
                            message=permission_mode,
                            tool=tool_name,
                            command=query,
                            agent=MODE,
                        )
                        _emit(approve(permission_mode, hook_event=hook_event))
                        return
                # Handle WebSearch tool
                if hook_event == "PostToolUse":
                    logging.info(f"PostToolUse WebSearch: {query}")
                    handle_web_post_tool_use(query, config)
                else:
                    logging.info(f"Checking WebSearch: {query}")
                    result = check_web_tool(query, config)
                    if not result:
                        log_decision(
                            "pass",
                            message="no matching rule",
                            tool="WebSearch",
                            command=query,
                            agent=MODE,
                        )
                    print(json.dumps(result))
                return

            # Check if this is a file operation tool
            if tool_name in FILE_TOOL_NAMES:
                file_path = (
                    tool_input.get("file_path")
                    or tool_input.get("path")
                    or tool_input.get("filepath")
                    or ""
                )
                # Multi-file support: extract paths array
                if not file_path:
                    paths = tool_input.get("paths") or []
                    if paths and hook_event != "PostToolUse":
                        # Evaluate each path, take strictest result
                        strictest = None
                        for p in paths:
                            m = (
                                match_read(p, config, cwd)
                                if tool_name
                                in (
                                    "Read",
                                    "read_file",
                                    "read",
                                    "read_many_files",
                                    "LS",
                                    "Glob",
                                    "Grep",
                                    "Search",
                                )
                                else match_edit(p, config, cwd)
                            )
                            if m is not None:
                                if strictest is None or (
                                    "deny",
                                    "ask",
                                    "allow",
                                    "pass",
                                ).index(m.decision) < (
                                    "deny",
                                    "ask",
                                    "allow",
                                    "pass",
                                ).index(strictest.decision):
                                    strictest = m
                        if strictest is not None:
                            reason = (
                                strictest.message
                                if strictest.message
                                else f"[{strictest.pattern}]"
                            )
                            log_decision(
                                strictest.decision,
                                rule=strictest.pattern,
                                tool=tool_name,
                                file_path=paths[0],
                                cwd=cwd,
                                message=f"multi-file ({len(paths)} paths): {reason}",
                                command=json.dumps(paths),
                                agent=MODE,
                            )
                            if strictest.decision == "allow":
                                print(
                                    json.dumps(
                                        approve(
                                            reason, config=config, tool_name=tool_name
                                        )
                                    )
                                )
                            elif strictest.decision == "deny":
                                print(
                                    json.dumps(
                                        deny(reason, config=config, tool_name=tool_name)
                                    )
                                )
                            else:
                                print(
                                    json.dumps(
                                        ask(reason, config=config, tool_name=tool_name)
                                    )
                                )
                        else:
                            # No rules matched any path — apply mode-specific fallback
                            if MODE == "gemini":
                                log_decision(
                                    "ask",
                                    message=f"multi-file ({len(paths)} paths): no matching rule",
                                    tool=tool_name,
                                    file_path=paths[0],
                                    command=json.dumps(paths),
                                    cwd=cwd,
                                    agent=MODE,
                                )
                                _emit(ask("no matching rule"))
                            else:
                                log_decision(
                                    "pass",
                                    message=f"multi-file ({len(paths)} paths): no matching rule",
                                    tool=tool_name,
                                    file_path=paths[0],
                                    command=json.dumps(paths),
                                    cwd=cwd,
                                    agent=MODE,
                                )
                                print(json.dumps({}))
                        return

                if file_path and hook_event != "PostToolUse":
                    # Check for bypass permissions mode first
                    permission_mode = input_data.get("permission_mode", "default")
                    if permission_mode in (
                        "bypassPermissions",
                        "dontAsk",
                        "acceptEdits",
                    ):
                        logging.info(f"Bypass mode ({permission_mode}): {tool_name}")
                        log_decision(
                            "allow",
                            message=permission_mode,
                            tool=tool_name,
                            file_path=file_path,
                            cwd=cwd,
                            agent=MODE,
                        )
                        _emit(approve(permission_mode))
                        return

                    logging.info(f"Checking file op: {tool_name} -> {file_path}")
                    try:
                        result = check_file_tool(tool_name, file_path, config, cwd)
                        if not result:
                            if MODE == "gemini":
                                log_decision(
                                    "ask",
                                    message="no matching rule",
                                    tool=tool_name,
                                    file_path=file_path,
                                    cwd=cwd,
                                    agent=MODE,
                                )
                                result = ask("no matching rule")
                            else:
                                log_decision(
                                    "pass",
                                    message="no matching rule",
                                    tool=tool_name,
                                    file_path=file_path,
                                    cwd=cwd,
                                    agent=MODE,
                                )
                        print(json.dumps(result))
                    except Exception as e:
                        logging.error(f"Error checking file tool: {e}")
                        if MODE == "gemini":
                            log_decision(
                                "ask",
                                message="file-check-error",
                                tool=tool_name,
                                file_path=file_path,
                                cwd=cwd,
                                agent=MODE,
                            )
                            _emit(ask(f"error: {e}"))
                        else:
                            log_decision(
                                "pass",
                                message="file-check-error",
                                tool=tool_name,
                                file_path=file_path,
                                cwd=cwd,
                                agent=MODE,
                            )
                            print(json.dumps({}))
                    return
                # No file_path or PostToolUse - fall through to default behavior
                if hook_event != "PostToolUse":
                    if MODE == "gemini":
                        log_decision(
                            "ask",
                            message=f"no file path for {tool_name}",
                            tool=tool_name,
                            agent=MODE,
                        )
                        _emit(ask("no file path provided"))
                    else:
                        log_decision(
                            "pass",
                            message=f"no file path for {tool_name}",
                            tool=tool_name,
                            agent=MODE,
                        )
                        print(json.dumps({}))
                else:
                    print(json.dumps({}))
                return

            # Only handle shell/bash commands
            if tool_name not in SHELL_TOOL_NAMES:
                if MODE == "gemini":
                    log_decision(
                        "ask",
                        message=f"unsupported tool: {tool_name}",
                        tool=tool_name,
                        agent=MODE,
                    )
                    _emit(ask(f"unsupported tool: {tool_name}"))
                else:
                    log_decision(
                        "pass",
                        message=f"unsupported tool: {tool_name}",
                        tool=tool_name,
                        agent=MODE,
                    )
                    print(json.dumps({}))
                return

            command = tool_input.get("command") or tool_input.get("cmd") or ""

        # Check for bypass permissions mode (Claude Code PreToolUse only)
        if hook_event != "PostToolUse":
            permission_mode = input_data.get("permission_mode", "default")
            if permission_mode in ("bypassPermissions", "dontAsk"):
                logging.info(f"Bypass mode ({permission_mode}): {command}")
                log_decision(
                    "allow",
                    message=permission_mode,
                    command=command,
                    cwd=cwd,
                    agent=MODE,
                )
                result = approve(permission_mode, hook_event=hook_event)
                if result is not None:
                    print(json.dumps(result))
                return

        # Route based on hook event type
        if hook_event == "PostToolUse":
            logging.info(f"PostToolUse: {command}")
            handle_post_tool_use(command, config, cwd)
        else:
            logging.info(f"Checking: {command}")
            result = check_command(command, config, cwd, hook_event=hook_event)
            # Codex: None sentinel means exit 0 with no output
            if result is not None:
                print(json.dumps(result))

    except json.JSONDecodeError:
        logging.error("Invalid JSON input")
        if MODE == "gemini":
            log_decision("ask", message="json-parse-error", agent=MODE)
            result = ask("invalid json input")
            if result is not None:
                print(json.dumps(result))
        else:
            log_decision("pass", message="json-parse-error", agent=MODE)
            print(json.dumps({}))
    except Exception as e:
        logging.error(f"Error: {e}")
        if MODE == "gemini":
            log_decision("ask", message="hook-error", agent=MODE)
            result = ask(f"error: {e}")
            if result is not None:
                print(json.dumps(result))
        else:
            log_decision("pass", message="hook-error", agent=MODE)
            print(json.dumps({}))


if __name__ == "__main__":
    main()
