#!/usr/bin/env python3
"""
pi-mono wrapper for dippy command validation.

Calls dippy's analyze() function and outputs JSON result.
Input: JSON with {"command": "...", "cwd": "..."}
Output: JSON with {"action": "allow|ask|deny|pass", "reason": "...", "context_flags": [...]}
"""
import json
import sys
from pathlib import Path

# Add dippy src to path
sys.path.insert(0, str(Path(__file__).parent.parent))

from dippy.core.analyzer import analyze
from dippy.core.config import load_config


def main():
    """Read JSON from stdin, call analyze(), output JSON."""
    try:
        # Read input
        input_data = json.loads(sys.stdin.read())
        command = input_data.get('command', '')
        cwd_str = input_data.get('cwd', '.')

        if not command:
            result = {
                'action': 'ask',
                'reason': 'Empty command',
                'error': False
            }
            print(json.dumps(result))
            sys.exit(0)

        # Convert cwd to Path
        cwd = Path(cwd_str).resolve() if cwd_str else Path.cwd()

        # Load dippy config (will use ~/.dippy/config or .dippy from cwd)
        try:
            config = load_config(cwd)
        except Exception as e:
            # If config fails, conservatively ask for approval
            result = {
                'action': 'ask',
                'reason': f'Config error: {str(e)}',
                'error': True
            }
            print(json.dumps(result))
            sys.exit(0)

        # Analyze command using dippy's actual entry point
        decision = analyze(command, config, cwd)

        # Output JSON (map dippy's Decision to pi-mono format)
        result = {
            'action': decision.action,
            'reason': decision.reason,
            'context_flags': sorted(decision.context_flags) if decision.context_flags else [],
            'error': False
        }
        print(json.dumps(result))
        sys.exit(0)

    except json.JSONDecodeError as e:
        # Invalid JSON input
        error_result = {
            'action': 'ask',
            'reason': f'Invalid JSON input: {str(e)}',
            'error': True
        }
        print(json.dumps(error_result))
        sys.exit(1)

    except Exception as e:
        # On error, conservatively ask for approval
        error_result = {
            'action': 'ask',
            'reason': f'Dippy error: {str(e)}',
            'error': True
        }
        print(json.dumps(error_result))
        sys.exit(1)


if __name__ == '__main__':
    main()
