# Dippy Extension for pi-mono

Integrates dippy's bash command approval system with pi-mono AI coding assistant.

## Overview

This extension intercepts bash tool calls in pi-mono and validates them through [dippy](../README.md), providing:
- **Auto-approval** for safe commands (ls, git status, cat, etc.)
- **User prompts** for potentially destructive operations (rm, pip install, etc.)
- **Hard blocks** for dangerous patterns (rm -rf /, etc.)
- **Context-aware** decisions (pipelines, subshells, redirects)

## Installation

```bash
# The extension is already symlinked to:
# ~/.pi/agent/extensions/dippy-extension.ts

# If you need to reinstall:
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

The extension uses your existing dippy configuration:
- **Global config**: `~/.dippy/config`
- **Project config**: `.dippy` file (searched upward from current directory)

See [dippy configuration docs](../docs/config.md) for details.

## How It Works

```
pi-mono bash tool call
    ↓
dippy-extension.ts intercepts
    ↓
spawns python3 src/dippy/pi_wrapper.py
    ↓
wrapper calls dippy.core.analyzer.analyze()
    ↓
returns decision: allow | ask | deny | pass
    ↓
extension handles decision:
  - allow: execute immediately
  - ask: show confirmation dialog
  - deny: block with message
  - pass: let pi-mono decide
```

## Testing

```bash
# Test Python wrapper directly
echo '{"command":"ls","cwd":"/home/michael"}' | python3 src/dippy/pi_wrapper.py
# Expected: {"action":"allow",...}

echo '{"command":"rm -rf /","cwd":"/home/michael"}' | python3 src/dippy/pi_wrapper.py
# Expected: {"action":"ask",...}

# Test with pi-mono
pi
# Then try commands:
# - "List files" → should execute immediately
# - "Delete node_modules" → should prompt for approval
```

## Files

- **`dippy-extension.ts`** - Main extension file (TypeScript, loaded by pi-mono via jiti)
- **`src/dippy/pi_wrapper.py`** - JSON wrapper script (Python, called by extension)

## Troubleshooting

**Extension not loading:**
```bash
# Check symlink exists
ls -la ~/.pi/agent/extensions/dippy-extension.ts

# Check pi-mono logs for errors
pi --debug
```

**Python wrapper failing:**
```bash
# Test wrapper directly
echo '{"command":"ls","cwd":"."}' | python3 src/dippy/pi_wrapper.py

# Check dippy is installed
python3 -c "from dippy.core.analyzer import analyze; print('OK')"
```

**Commands not being filtered:**
```bash
# Check dippy config is loaded
echo '{"command":"rm foo","cwd":"."}' | python3 src/dippy/pi_wrapper.py

# Test dippy directly
dippy --cmd 'rm foo'
```

## Development

When modifying the extension:
1. **Python wrapper**: No rebuild needed, just test
2. **TypeScript extension**: pi-mono uses jiti to load TypeScript directly

## License

Same as dippy project.
