#!/usr/bin/env python3
"""
pi-mono wrapper for dippy command and file access validation.

Calls dippy's analysis functions and outputs JSON result.
Input: JSON with {
    "type": "bash" | "read" | "edit" | "idle",
    "command": "...", (for bash)
    "path": "...",    (for read/edit)
    "cwd": "..."
}
Output: JSON with {"action": "allow|ask|deny|pass", "reason": "...", "note": "...", "context_flags": [...]}
"""

import json
import sys
from pathlib import Path

# Add dippy src to path
sys.path.insert(0, str(Path(__file__).parent.parent))

from dippy.core.analyzer import analyze, Decision
from dippy.core.config import (
    load_config,
    match_edit,
    match_read,
    configure_logging,
    log_decision,
)
from dippy.core.notifier import run_notifier, should_run_notifier


# Default deny format templates
DEFAULT_DENY_FORMAT = "⚠️ DENIED by security policy.\n\nCommand: {command}\n\n{reason}"
DEFAULT_DENY_FORMATS = {
    "pi": "⚠️ COMMAND DENIED by security policy.\n\nOriginal: {command}\n\nINSTRUCTION: {reason}\n\nYou MUST follow the instruction above. Do NOT try alternative commands.",
    "claude": "⚠️ COMMAND DENIED by security policy.\n\nOriginal: {command}\n\nINSTRUCTION: {reason}\n\nYou MUST follow the instruction above. Do NOT try alternative commands.",
}


def format_deny_reason(
    reason: str,
    command: str | None,
    pattern: str | None,
    config,
    agent: str,
) -> str:
    """Format deny reason using configured template.

    Supports placeholders:
    - {command} - original command
    - {reason} - message from rule
    - {pattern} - pattern that matched

    Args:
        reason: The deny reason/message
        command: Original command (for bash)
        pattern: Pattern that matched (optional)
        config: Config object with deny_format settings
        agent: Agent ID (pi, claude, etc.)

    Returns:
        Formatted reason string
    """
    # Try agent-specific format first, then general format, then default
    template = None
    if config.deny_format_agents and agent in config.deny_format_agents:
        template = config.deny_format_agents[agent]
    elif config.deny_format:
        template = config.deny_format
    elif agent in DEFAULT_DENY_FORMATS:
        template = DEFAULT_DENY_FORMATS[agent]
    else:
        template = DEFAULT_DENY_FORMAT

    # Extract pattern from reason if available (format: "cmd: message" or "cmd (pattern)")
    extracted_pattern = pattern
    if not extracted_pattern and reason:
        # Try to extract from reason like "find: Use rg instead" -> pattern="find"
        if ": " in reason:
            extracted_pattern = reason.split(":")[0].strip()

    # Substitute placeholders
    result = template
    result = result.replace("{command}", command or "")
    result = result.replace("{reason}", reason or "")
    result = result.replace("{pattern}", extracted_pattern or "")

    return result


def main():
    """Read JSON from stdin, dispatch to appropriate validator, output JSON."""
    try:
        # Read input
        input_data = json.loads(sys.stdin.read())
        req_type = input_data.get("type", "bash")
        cwd_str = input_data.get("cwd", ".")
        cwd = Path(cwd_str).resolve() if cwd_str else Path.cwd()
        agent = input_data.get("agent", "pi")  # Agent ID for audit logging

        # Load dippy config
        try:
            config = load_config(cwd)
            configure_logging(config)
        except Exception as e:
            result = {
                "action": "ask",
                "reason": f"Config error: {str(e)}",
                "error": True,
            }
            print(json.dumps(result))
            sys.exit(0)

        decision = None

        if req_type == "bash":
            command = input_data.get("command", "")
            if not command:
                decision = Decision("ask", "Empty command")
            else:
                decision = analyze(command, config, cwd)

        elif req_type == "idle":
            # Idle mode: wait for notifications
            decision = Decision("allow", "idle")

        elif req_type == "edit":
            path = input_data.get("path", "")
            if not path:
                decision = Decision("ask", "Empty path for edit")
            else:
                # Use native Dippy match_edit rules
                match = match_edit(path, config, cwd)
                if match:
                    decision = Decision(
                        match.decision, f"edit {path}: {match.message or match.pattern}"
                    )
                else:
                    # Fallback to global default for edits
                    decision = Decision(config.default, f"edit {path} (default)")

        elif req_type == "read":
            path = input_data.get("path", "")
            if not path:
                decision = Decision("ask", "Empty path for read")
            else:
                # Use native Dippy match_read rules
                match = match_read(path, config, cwd)
                if match:
                    decision = Decision(
                        match.decision, f"read {path}: {match.message or match.pattern}"
                    )
                else:
                    # Fallback to global default for reads
                    decision = Decision(config.default, f"read {path} (default)")

        else:
            decision = Decision("ask", f"Unknown request type: {req_type}")

        # Log the decision
        if req_type == "bash":
            log_decision(
                decision.action,
                command=input_data.get("command", ""),
                cwd=cwd,
                message=decision.reason,
                context_flags=getattr(decision, "context_flags", None),
                agent=agent,
                suggestion=getattr(decision, "suggestion", None),
            )
        elif req_type == "edit":
            log_decision(
                decision.action,
                tool="Edit",
                file_path=input_data.get("path", ""),
                cwd=cwd,
                message=decision.reason,
                agent=agent,
            )
        elif req_type == "read":
            log_decision(
                decision.action,
                tool="Read",
                file_path=input_data.get("path", ""),
                cwd=cwd,
                message=decision.reason,
                agent=agent,
            )

        # Output JSON
        # Format deny reason if applicable
        output_reason = decision.reason
        if decision.action == "deny":
            command_str = input_data.get("command", "") if req_type == "bash" else None
            pattern_str = None
            # Try to extract pattern from reason
            if decision.reason and ": " in decision.reason:
                pattern_str = decision.reason.split(":")[0].strip()
            output_reason = format_deny_reason(
                decision.reason, command_str, pattern_str, config, agent
            )

        result = {
            "action": decision.action,
            "reason": output_reason,
            "context_flags": sorted(decision.context_flags)
            if getattr(decision, "context_flags", None)
            else [],
            "note": (
                run_notifier(config, idle=(req_type == "idle"))
                if (
                    req_type == "idle"
                    or should_run_notifier(
                        config,
                        tool_name=req_type if req_type in ("read", "edit") else None,
                        command=input_data.get("command"),
                    )
                )
                else None
            ),
            "error": False,
        }
        print(json.dumps(result))
        sys.exit(0)

    except json.JSONDecodeError as e:
        error_result = {
            "action": "ask",
            "reason": f"Invalid JSON input: {str(e)}",
            "error": True,
        }
        print(json.dumps(error_result))
        sys.exit(1)

    except Exception as e:
        error_result = {
            "action": "ask",
            "reason": f"Dippy error: {str(e)}",
            "error": True,
        }
        print(json.dumps(error_result))
        sys.exit(1)


if __name__ == "__main__":
    main()
