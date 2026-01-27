# Dippy Extension for pi-mono

Integrates dippy's bash command and file access approval system with pi-mono AI coding assistant.

## Overview

This extension intercepts tool calls in pi-mono and validates them through [dippy](../README.md), providing:
- **Bash Commands**: Validates `bash` tool calls using Dippy's AST analyzer.
- **File Edits**: Validates `write` and `edit` tool calls using native `allow-edit` rules.
- **File Reads**: Validates `read` tool calls using native `allow-read` rules.

Benefits:
- **Auto-approval** for safe operations (ls, cat, editing src files).
- **User prompts** for sensitive or dangerous operations.
- **Unified Security Policy** across all agent actions.

## Installation

```bash
# The extension is typically symlinked to:
# ~/.pi/agent/extensions/dippy-extension.ts

# If you need to install/reinstall:
ln -s /home/michael/work/ai/CLAUDE/TOOLS/dippy-dev/pi-extension/dippy-extension.ts \
      ~/.pi/agent/extensions/dippy-extension.ts
```

## Requirements

- **Dippy installed** in system Python:
  ```bash
  pip install dippy  # or: pip install -e /path/to/dippy-dev
  ```
- **pi-mono** with extension support

## Configuration

The extension uses your existing dippy configuration (~/.dippy/config or .dippy).

### Command Rules
```bash
allow ls *
deny rm -rf / "Are you crazy?"
```

### File Edit Rules (for write/edit tools)
```bash
allow-edit src/**        # auto-approve editing source code
deny-edit .env           # block editing environment variables
# default is 'ask' for files without a rule
```

### File Read Rules (for read tool)
```bash
allow-read src/**        # auto-approve reading source code
deny-read .env           # block reading secrets
# default is 'ask' (or whatever global 'default' is set to)
```

## How It Works

```
pi-mono tool call (bash|read|write|edit)
    ↓
dippy-extension.ts intercepts
    ↓
spawns python3 src/dippy/pi_wrapper.py
    ↓
wrapper calls dippy matching logic
    ↓
returns decision: allow | ask | deny | pass
    ↓
extension handles decision:
  - allow: execute immediately
  - ask: show confirmation dialog
  - deny: block with message
```

## Testing

```bash
# Test bash command
echo '{"type":"bash","command":"ls","cwd":"."}' | python3 src/dippy/pi_wrapper.py

# Test file edit (uses allow-edit rules)
echo '{"type":"edit","path":"src/main.ts","cwd":"."}' | python3 src/dippy/pi_wrapper.py

# Test file read (uses allow-read rules)
echo '{"type":"read","path":"README.md","cwd":"."}' | python3 src/dippy/pi_wrapper.py
```

## License

Same as dippy project.
