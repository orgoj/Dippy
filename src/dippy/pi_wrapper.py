#!/usr/bin/env python3
"""
pi-mono wrapper for dippy command and file access validation.

Calls dippy's analysis functions and outputs JSON result.
Input: JSON with {
    "type": "bash" | "read" | "edit",
    "command": "...", (for bash)
    "path": "...",    (for read/edit)
    "cwd": "..."
}
Output: JSON with {"action": "allow|ask|deny|pass", "reason": "...", "context_flags": [...]}
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
        result = {
            "action": decision.action,
            "reason": decision.reason,
            "context_flags": sorted(decision.context_flags)
            if getattr(decision, "context_flags", None)
            else [],
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
