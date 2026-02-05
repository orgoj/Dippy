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
from dippy.core.analyzer import analyze


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


# Initial mode from flags/env
MODE = _detect_mode_from_flags() or "claude"

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


def setup_logging():
    """Configure logging to file. Fails silently if unable to write."""
    try:
        log_file = _get_log_file()
        log_file.parent.mkdir(parents=True, exist_ok=True)
        logging.basicConfig(
            filename=str(log_file),
            level=logging.INFO,
            format="%(asctime)s [%(levelname)s] %(message)s",
            datefmt="%Y-%m-%d %H:%M:%S",
        )
    except (OSError, PermissionError):
        pass  # Logging is optional - don't crash if we can't write


# === Response Helpers ===


def approve(reason: str = "all commands safe") -> dict:
    """Return approval response."""
    logging.info(f"APPROVED: {reason}")
    if MODE == "gemini":
        return {
            "decision": "allow",
            "reason": f"🐤 {reason}",
            "systemMessage": f"🐤 {reason}",
            "continue": True,
        }
    if MODE == "cursor":
        # Include both snake_case (v2.0+) and camelCase (v1.7.x) for compatibility
        msg = f"🐤 {reason}"
        return {
            "permission": "allow",
            "user_message": msg,
            "agent_message": msg,
            "userMessage": msg,
            "agentMessage": msg,
        }
    return {
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow",
            "permissionDecisionReason": f"🐤 {reason}",
        }
    }


def ask(reason: str = "needs approval") -> dict:
    """Return ask response to prompt user for confirmation."""
    logging.info(f"ASK: {reason}")
    if MODE == "gemini":
        return {
            "decision": "ask",
            "reason": f"🐤 {reason}",
            "systemMessage": f"🐤 {reason}",
            "continue": True,
        }
    if MODE == "cursor":
        # Include both snake_case (v2.0+) and camelCase (v1.7.x) for compatibility
        msg = f"🐤 {reason}"
        return {
            "permission": "ask",
            "user_message": msg,
            "agent_message": msg,
            "userMessage": msg,
            "agentMessage": msg,
        }
    return {
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "ask",
            "permissionDecisionReason": f"🐤 {reason}",
        }
    }


def deny(reason: str = "denied by config") -> dict:
    """Return deny response to block the command."""
    logging.info(f"DENY: {reason}")
    if MODE == "gemini":
        # Gemini CLI: Exit code 2 with stderr is the standard way to block a tool
        # and provide feedback to the agent without stopping the loop or
        # triggering a manual confirmation dialog (in v0.23+).
        print(f"🐤 {reason}", file=sys.stderr)
        sys.exit(2)
    if MODE == "cursor":
        # Include both snake_case (v2.0+) and camelCase (v1.7.x) for compatibility
        msg = f"🐤 {reason}"
        return {
            "permission": "deny",
            "user_message": msg,
            "agent_message": msg,
            "userMessage": msg,
            "agentMessage": msg,
        }
    return {
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": f"🐤 {reason}",
        }
    }


def pass_(reason: str = "passing through") -> dict:
    """Return empty response to let Claude handle permissions with its default behavior."""
    logging.info(f"PASS: {reason}")
    if MODE == "gemini":
        return {"decision": "allow", "reason": f"🐤 {reason}", "continue": True}
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


def check_command(command: str, config: Config, cwd: Path) -> dict:
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
    )

    if result.action == "allow":
        return approve(result.reason)
    elif result.action == "deny":
        return deny(result.reason)
    elif result.action == "pass":
        return pass_(result.reason)
    else:
        return ask(result.reason)


def post_tool_response(message: str) -> dict:
    """Return PostToolUse response with feedback for Claude."""
    return {
        "hookSpecificOutput": {
            "hookEventName": "PostToolUse",
            "additionalContext": f"🐤 {message}",
        }
    }


def handle_post_tool_use(command: str, config: Config, cwd: Path) -> None:
    """Handle PostToolUse hook - output feedback message if rule matches."""
    from dippy.core.config import match_after
    from dippy.core.parser import tokenize

    words = tokenize(command)
    message = match_after(words, config, cwd)
    if message:  # non-empty string
        print(json.dumps(post_tool_response(message)))
    # empty string or None = silent (no output)


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
        return approve(reason)
    elif match.decision == "deny":
        return deny(reason)
    else:
        return ask(reason)


def handle_mcp_post_tool_use(tool_name: str, config: Config) -> None:
    """Handle PostToolUse hook for MCP tools - output feedback if rule matches."""
    message = match_after_mcp(tool_name, config)
    if message:  # non-empty string
        print(json.dumps(post_tool_response(message)))
    # empty string or None = silent (no output)


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
        return approve(reason)
    elif match.decision == "deny":
        return deny(reason)
    else:
        return ask(reason)


def handle_web_post_tool_use(query: str, config: Config) -> None:
    """Handle PostToolUse hook for WebSearch - output feedback if rule matches."""
    message = match_after_web(query, config)
    if message:  # non-empty string
        print(json.dumps(post_tool_response(message)))
    # empty string or None = silent (no output)


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
    if tool_name in ("Read", "read_file"):
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
        return approve(reason)
    elif match.decision == "deny":
        return deny(reason)
    else:
        return ask(reason)


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
""",
    )

    # CLI mode arguments
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
    parser.add_argument("--version", action="version", version="dippy 0.2.4")
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
    )

    # Map 'pass' to 'ask' in CLI mode (pass means "let the AI decide" which
    # doesn't make sense in CLI context)
    action = result.action if result.action != "pass" else "ask"

    # Output result
    if args.json_output:
        print(json.dumps({"decision": action, "reason": result.reason}))
    else:
        print(f"{action}: {result.reason}")

    # Return exit code
    if action == "allow":
        return EXIT_ALLOW
    elif action == "deny":
        return EXIT_DENY
    else:
        return EXIT_ASK


def main():
    """Main entry point for the hook."""
    global MODE

    # Parse arguments first to detect CLI mode
    args = parse_cli_args()

    # CLI mode: --cmd or --stdin
    if args.cmd or args.stdin:
        if args.agent:
            MODE = args.agent
        sys.exit(cli_mode(args))

    # Detect mode strictly from flags/env or default to claude
    MODE = _detect_mode_from_flags() or "claude"

    # Hook mode: continue with original behavior
    setup_logging()
    if MODE == "gemini":
        logging.info("Gemini mode enforced by flag/env.")
    else:
        logging.info(f"Mode set to: {MODE}")

    try:
        # Read hook input from stdin
        input_raw = sys.stdin.read()
        if not input_raw:
            return
        input_data = json.loads(input_raw)

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

        # Normalize Gemini events to Claude names for internal routing
        if hook_event == "BeforeTool":
            hook_event = "PreToolUse"
        elif hook_event == "AfterTool":
            hook_event = "PostToolUse"

        # Extract command based on mode
        # Cursor: {"command": "...", "cwd": "..."}
        # Claude/Gemini: {"tool_name": "...", "tool_input": {"command": "..."}}
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
                        log_decision("allow", permission_mode, agent=MODE)
                        print(json.dumps(approve(permission_mode)))
                        return
                # Handle MCP tool
                if hook_event == "PostToolUse":
                    logging.info(f"PostToolUse MCP: {tool_name}")
                    handle_mcp_post_tool_use(tool_name, config)
                else:
                    logging.info(f"Checking MCP: {tool_name}")
                    result = check_mcp_tool(tool_name, config)
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
                        log_decision("allow", permission_mode, agent=MODE)
                        print(json.dumps(approve(permission_mode)))
                        return
                # Handle WebSearch tool
                if hook_event == "PostToolUse":
                    logging.info(f"PostToolUse WebSearch: {query}")
                    handle_web_post_tool_use(query, config)
                else:
                    logging.info(f"Checking WebSearch: {query}")
                    result = check_web_tool(query, config)
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
                            tool=tool_name,
                            file_path=file_path,
                            cwd=cwd,
                            agent=MODE,
                        )
                        print(json.dumps(approve(permission_mode)))
                        return

                    logging.info(f"Checking file op: {tool_name} -> {file_path}")
                    try:
                        result = check_file_tool(tool_name, file_path, config, cwd)
                        if not result and MODE == "gemini":
                            result = approve("passing through (no match)")
                        print(json.dumps(result))
                    except Exception as e:
                        logging.error(f"Error checking file tool: {e}")
                        if MODE == "gemini":
                            print(json.dumps(approve(f"error recovery: {e}")))
                        else:
                            print(json.dumps({}))
                    return
                # No file_path or PostToolUse - fall through to default behavior
                if MODE == "gemini":
                    print(json.dumps(approve("no file path provided")))
                else:
                    print(json.dumps({}))
                return

            # Only handle shell/bash commands
            if tool_name not in SHELL_TOOL_NAMES:
                if MODE == "gemini":
                    print(json.dumps(approve(f"unsupported tool: {tool_name}")))
                else:
                    print(json.dumps({}))
                return

            command = tool_input.get("command") or tool_input.get("cmd") or ""

        # Check for bypass permissions mode (Claude Code PreToolUse only)
        if hook_event != "PostToolUse":
            permission_mode = input_data.get("permission_mode", "default")
            if permission_mode in ("bypassPermissions", "dontAsk"):
                logging.info(f"Bypass mode ({permission_mode}): {command}")
                log_decision("allow", permission_mode, command=command, agent=MODE)
                print(json.dumps(approve(permission_mode)))
                return

        # Route based on hook event type
        if hook_event == "PostToolUse":
            logging.info(f"PostToolUse: {command}")
            handle_post_tool_use(command, config, cwd)
        else:
            logging.info(f"Checking: {command}")
            result = check_command(command, config, cwd)
            print(json.dumps(result))

    except json.JSONDecodeError:
        logging.error("Invalid JSON input")
        if MODE == "gemini":
            print(json.dumps(approve("invalid json input")))
        else:
            print(json.dumps({}))
    except Exception as e:
        logging.error(f"Error: {e}")
        if MODE == "gemini":
            print(json.dumps(approve(f"error recovery: {e}")))
        else:
            print(json.dumps({}))


if __name__ == "__main__":
    main()
