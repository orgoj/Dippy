# Dippy Extension for Moltbot

Command and file operation validation for Moltbot's Pi agent.

## Features

- **Bash command validation**: Block dangerous commands (rm -rf, git push --force, etc.)
- **File read validation**: Control which files can be read
- **File write/edit validation**: Control which files can be modified
- **Shared config**: Uses same ~/.dippy/config and .dippy files as Claude Code

## Installation

### Option 1: Symlink (recommended for development)

```bash
ln -s /path/to/dippy/moltbot-extension ~/.moltbot/extensions/dippy
```

### Option 2: Config reference

Add to ~/.moltbot/config.yaml:
```yaml
plugins:
  - path: /path/to/dippy/moltbot-extension
    config:
      enabled: true
      askBehavior: block  # block | ask | allow
```

## Configuration

### Plugin Config Options

- `enabled`: Enable/disable validation (default: true)
- `askBehavior`: How to handle 'ask' decisions:
  - `block` (default): Block operation until explicit config rule added
  - `allow`: Allow without prompt (fast but less secure)
  - `ask`: Prompt user (when UI becomes available)

### Dippy Config (~/.dippy/config)

```bash
# Global default for unmatched commands/files
default ask

# Bash commands
allow git status
allow git log **
allow ls **
allow cat **
deny rm -rf **
deny git push --force **
ask sudo **

# File reads
allow-read ~/projects/**
allow-read src/**
deny-read ~/.ssh/**
deny-read .env

# File edits
allow-edit ~/projects/**
allow-edit src/**
deny-edit /etc/**
deny-edit ~/.ssh/**
```

### Project-level Config (.dippy)

Create a `.dippy` file in your project root to override global settings:

```bash
# Project-specific overrides
allow npm run build
allow pnpm test
deny rm -rf node_modules  # Use proper npm commands instead
```

## Tool Mapping

| Moltbot Tool | Dippy Type | Dippy Config Rules |
|--------------|------------|-------------------|
| `exec` | `bash` | `allow`, `ask`, `deny` |
| `bash` | `bash` | `allow`, `ask`, `deny` |
| `read` | `read` | `allow-read`, `ask-read`, `deny-read` |
| `write` | `edit` | `allow-edit`, `ask-edit`, `deny-edit` |
| `edit` | `edit` | `allow-edit`, `ask-edit`, `deny-edit` |

## How It Works

```
Moltbot tool call (exec|read|write|edit)
    ↓
dippy-extension.ts intercepts via before_tool_call hook
    ↓
spawns python3 src/dippy/pi_wrapper.py
    ↓
wrapper calls dippy matching logic
    ↓
returns decision: allow | ask | deny | pass
    ↓
extension handles decision:
  - allow/pass: execute immediately
  - ask: depends on askBehavior config
  - deny: block with message
```

## Testing

### Test the Python wrapper directly

```bash
# Test bash command
echo '{"type":"bash","command":"ls","cwd":"."}' | python3 src/dippy/pi_wrapper.py

# Test file edit (uses allow-edit rules)
echo '{"type":"edit","path":"src/main.ts","cwd":"."}' | python3 src/dippy/pi_wrapper.py

# Test file read (uses allow-read rules)
echo '{"type":"read","path":"README.md","cwd":"."}' | python3 src/dippy/pi_wrapper.py
```

### Test with moltbot

```bash
# Should be allowed (assuming default config)
moltbot agent --message "run ls -la"

# Should be blocked
moltbot agent --message "run rm -rf /"
```

## Requirements

- Python 3.8+
- dippy repo accessible at configured path
- Moltbot with plugin support

## Troubleshooting

### "Dippy wrapper not found"

Ensure the symlink points to the correct location and the dippy repo is complete:

```bash
ls -la ~/.moltbot/extensions/dippy
ls /path/to/dippy/src/dippy/pi_wrapper.py
```

### "Dippy validation error"

Check that Python 3 is available and dippy is importable:

```bash
python3 -c "from dippy.core.analyzer import analyze; print('OK')"
```

### Commands blocked unexpectedly

Check your dippy config and add appropriate rules:

```bash
# View current config
cat ~/.dippy/config

# Add rule for specific command
echo "allow your-command **" >> ~/.dippy/config
```

## License

Same as dippy project.
