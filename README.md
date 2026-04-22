<p align="center">
  <img src="images/dippy.gif" width="200">
</p>

<h1 align="center">🐤 Dippy</h1>
<p align="center"><em>Because <code>ls</code> shouldn't need approval</em></p>

---

<!-- FORK ENHANCEMENTS START -->
### 🍴 Fork Enhancements (vs [upstream](https://github.com/ldayton/Dippy))

- **File Edit/Read Approval** — `allow-edit`/`read`, `ask-edit`/`read`, `deny-edit`/`read` rules
- **Include directive** — `include <path-or-glob>` for composable config files
- **Context-aware rules** — `[flags]` syntax with `@subshell`, `@compound`, negation (`!`)
- **Custom wrappers** — `wrapper <name>` for project-specific tools (ssh, docker exec, etc.), with `--cmd`, `--flag`, `--context`, `--context-first` flags for flexible configuration
- **Option rules** — `allow-opt`, `ask-opt`, `deny-opt` for subcommand/flag control
- **WebSearch support** — auto-approval for WebSearch tool *(by tony)*
- **Gemini CLI support** — integrated hook support for Gemini CLI tools
- **Codex CLI support** — native `hooks.json` integration for Codex `PreToolUse`/`PostToolUse` on `Bash`
- **Codex enforcement model** — `deny` hard-blocks via `exit 2`; `ask` is currently advisory and fails open
- **Structured JSON output** — for PostToolUse hooks *(by tony)*
- **SSH/sudo handlers** — remote context support for ssh and sudo commands
- **Log rotation** — `set log-rotate-max-days N` for automatic cleanup
- **Notifier (Sidekick)** — `set notifier-command "CMD"` for external notifications (mail check). Supports long-polling via `--idle` in stop hooks. Use `set notifier-include "tool1, tool2"` to limit when it triggers.
- **Idle Prompt Notifications** — `set idle-notifier-command "notify-send {title} {message}"` for notifications when Claude is waiting for input
- **Hook approvals log control** — `set log-hook-approvals off` to disable hook-approvals.log
- **Hybrid mode** — `set default pass` to let Claude decide unmatched commands
- **Audit log** — `cwd`, `agent`, and `suggestion` fields for better context
- **CLI mode** — standalone command validation with `--cmd`, `--stdin`, `--json`, `--remote`
- **Multi-Agent Support** — dedicated modes for Claude, Gemini, pi-mono, Moltbot?
- **pi-mono extension** — TypeScript extension for [pi-mono](https://github.com/badlogic/pi-mono) AI assistant
- **Python `-c` AST analysis** — `python -c 'code'` is statically analyzed for safety instead of always requiring confirmation. Safe code (no I/O, no dangerous imports) is auto-approved *(design by nickdaview)*
- **Configurable Python modules** — `python-allow-module` and `python-deny-module` directives to customize which modules are safe or dangerous during `-c` analysis
- **CLI management** — `dippy hooks install/uninstall/list` and `dippy doctor` diagnostics
<!-- FORK ENHANCEMENTS END -->

---

> **Stop the permission fatigue.** Claude Code asks for approval on every `ls`, `git status`, and `cat` - destroying your flow state. You check Slack, come back, and your assistant's just sitting there waiting.

Dippy is a shell command hook that auto-approves safe commands while still prompting for anything destructive. When it blocks, your custom deny messages can steer Claude back on track—no wasted turns. Get up to **40% faster development** without disabling permissions entirely.

Built on [Parable](https://github.com/ldayton/Parable), our own hand-written bash parser—no external dependencies, just pure Python. 14,000+ tests between the two.

***Example: rejecting unsafe operation in a chain***

![Screenshot](images/terraform-apply.png)

***Example: rejecting a command with advice, so Claude can keep going***

![Deny with message](images/deny-with-message.png)

## ✅ What gets approved

- **Complex pipelines**: `ps aux | grep python | awk '{print $2}' | head -10`
- **Chained reads**: `git status && git log --oneline -5 && git diff --stat`
- **Cloud inspection**: `aws ec2 describe-instances --filters "Name=tag:Environment,Values=prod"`
- **Container debugging**: `docker logs --tail 100 api-server 2>&1 | grep ERROR`
- **Safe redirects**: `grep -r "TODO" src/ 2>/dev/null`, `ls &>/dev/null`
- **Command substitution**: `ls $(pwd)`, `git diff foo-$(date).txt`

![Safe command substitution](images/safe-cmd-sub.png)

## 🚫 What gets blocked

- **Subshell injection**: `git $(echo rm) foo.txt`, `echo $(rm -rf /)`
- **Subtle file writes**: `curl https://example.com > script.sh`, `tee output.log`
- **Hidden mutations**: `git stash drop`, `npm unpublish`, `brew unlink`
- **Cloud danger**: `aws s3 rm s3://bucket --recursive`, `kubectl delete pod`
- **Destructive chains**: `rm -rf node_modules && npm install` (blocks the whole thing)

![Redirect blocked](images/redirect.png)

---

## ⚠️ Known Limitations

**Subagents ignore PreToolUse hook decisions** - Claude Code subagents (spawned via Task tool) do not respect `allow`/`deny` decisions from PreToolUse hooks. Even when Dippy returns `"permissionDecision": "allow"`, subagents will still prompt for approval.

- **Cause:** Known bug in Claude Code ([#4740](https://github.com/anthropics/claude-code/issues/4740), [#4669](https://github.com/anthropics/claude-code/issues/4669))
- **Status:** Closed as "not planned" by Anthropic (January 2026)
- **Impact:** Hooks work correctly in main sessions but are ignored in subagents
- **Workaround:** Use explicit config rules instead of relying on hook decisions

See [docs/subagent-hook-issues.md](docs/subagent-hook-issues.md) for detailed analysis.

---

## Installation

### Quick Install (from source)

```bash
# Clone the repository
git clone https://github.com/orgoj/Dippy.git
cd Dippy

# Install via uv tool (recommended)
uv tool install .

# Verify installation
dippy --version
```

**Note:** Make sure `~/.local/bin` is on your PATH.

### Development Mode

```bash
uv pip install -e .
```

---

## Quick Start

The easiest way to configure Dippy is using the CLI commands:

### 1. Install Hooks

```bash
# Install for Claude Code (global)
dippy hooks install claude --global

# Install for Gemini CLI (global)
dippy hooks install gemini --global

# Install for Cursor IDE (global)
dippy hooks install cursor --global

# Install for Windsurf (global)
dippy hooks install windsurf --global

# Install for Codex CLI (global, requires codex_hooks feature flag)
dippy hooks install codex --global
```

### 2. Verify Installation

```bash
# Check hook status
dippy hooks list

# Run diagnostics
dippy doctor
```

That's it! Dippy is now configured and will auto-approve safe commands.

### Manual Configuration (Advanced)

If you prefer manual configuration or need project-specific settings:

**Claude Code** - add to `~/.claude/settings.json`:
```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash|Write|Edit|MultiEdit|Read|LS|Glob|Grep|Search|WebSearch|mcp__.*",
        "hooks": [{ "type": "command", "command": "dippy" }]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "Bash|WebSearch|mcp__.*",
        "hooks": [{ "type": "command", "command": "dippy" }]
      }
    ]
  }
}
```

**Gemini CLI** - add to `~/.gemini/settings.json`:
```json
{
  "hooks": {
    "BeforeTool": [
      {
        "matcher": "run_shell_command|write_file|replace|read_file|google_web_search",
        "hooks": [{ "type": "command", "command": "dippy --gemini" }]
      }
    ],
    "AfterTool": [
      {
        "matcher": "run_shell_command|google_web_search",
        "hooks": [{ "type": "command", "command": "dippy --gemini" }]
      }
    ]
  }
}
```

**Codex CLI** - add to `~/.codex/hooks.json` and enable `codex_hooks = true` in `~/.codex/config.toml`:
```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "^Bash$",
        "hooks": [{ "type": "command", "command": "dippy --codex" }]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "^Bash$",
        "hooks": [{ "type": "command", "command": "dippy --codex" }]
      }
    ]
  }
}
```

**Current Codex limitation:** Dippy can hard-block Codex shell commands with `deny`, but `ask` is advisory only. Codex shows the `systemMessage` and still runs the command, so anything that must not execute needs a `deny` rule.

**Codex sandbox gotcha:** During Codex `workspace-write` tool execution on Linux, `.codex` can appear inside the sandbox as a synthetic read-only file-like path even when the host workspace does not contain a normal `.codex` file. That artifact comes from Codex sandbox path protection, not from Dippy.

**Hooks installed:**
- **PreToolUse**: Validates tools BEFORE execution (Bash, file ops, WebSearch, MCP)
- **PostToolUse**: Shows feedback messages AFTER execution (for `after` directive)

Changing hook config requires restarting the agent session. For Codex, both `hooks.json` and `config.toml` are loaded at session start.

---

## CLI Commands

### Hooks Management

```bash
dippy hooks list                    # Show hook status for all agents
dippy hooks install <agent>         # Install hooks (project-local)
dippy hooks install <agent> --global  # Install hooks (global)
dippy hooks uninstall <agent>       # Remove hooks (project-local)
dippy hooks uninstall <agent> --global  # Remove hooks (global)
```

**Supported agents:** `claude`, `gemini`, `cursor`, `windsurf`, `codex`

**Status indicators:**
- `+` = installed
- `?` = legacy (old `dippy-hook` detected)
- ` ` = not installed

**Scopes:**
- **Project-local** (default): `.claude/settings.json`, `.cursor/hooks.json`, `.gemini/settings.json`, `.codex/hooks.json`
- **Global** (`--global`): `~/.claude/settings.json`, `~/.cursor/hooks.json`, `~/.gemini/settings.json`, `~/.codex/hooks.json`

### Diagnostics

```bash
dippy doctor                        # Run all health checks
dippy doctor --agent claude         # Check specific agent
dippy doctor --verbose              # Show detailed diagnostics
```

**Health checks:**
- ✓ Installation (on PATH, version check)
- ✓ Hook status per agent (Claude, Gemini, Cursor, Windsurf, Codex, pi-mono)
- ✓ Codex `codex_hooks` feature flag validation
- ✓ Legacy hook detection with full path
- ✓ pi_wrapper check for pi-mono/moltbot
- ✓ Configuration validation (syntax errors)
- ✓ Log health (writable directories, file size warnings)

**Exit codes:** `0` (OK), `1` (warnings), `2` (critical issues)

### CLI Mode

Validate commands without running as a hook:

```bash
dippy --cmd 'rm -rf /'              # validate a command
dippy --cmd 'ls -la' --json         # JSON output
dippy --cmd 'git status' --cwd /path
echo 'ls -la' | dippy --stdin       # read command from stdin
```

**Options:**
- `--cmd COMMAND` — command to validate
- `--stdin` — read command from stdin
- `--cwd PATH` — working directory
- `--json` — output as JSON
- `--config PATH` — custom config file
- `--agent NAME` — force agent name in audit log
- `--remote` — skip local path checks
- `--version` — show version

---

## Supported Agents

Dippy adapts its output format and behavior based on the agent:

| Agent | Flag | Env Var | Hook Support |
|-------|------|---------|--------------|
| Claude Code | `--claude` | `DIPPY_CLAUDE=1` | ✅ |
| Gemini CLI | `--gemini` | `DIPPY_GEMINI=1` | ✅ |
| Cursor IDE | `--cursor` | `DIPPY_CURSOR=1` | ✅ |
| Windsurf | `--windsurf` | `DIPPY_WINDSURF=1` | ✅ |
| pi-mono | `--pi` | `DIPPY_PI=1` | extension |
| Moltbot | `--moltbot` | `DIPPY_MOLTBOT=1` | extension |
| OpenAI Codex | `--codex` | `DIPPY_CODEX=1` | ✅ |
| PearAI | `--pearai` | `DIPPY_PEARAI=1` | partial |

Each agent mode maintains its own approval log (e.g., `~/.claude/hook-approvals.log`).

---

## Configuration

Dippy reads config from `~/.dippy/config` (global) and `.dippy` (project).

### Basic Rules

```bash
# Allow safe commands
allow git status
allow ls *
allow cat *

# Block dangerous commands
deny rm -rf *
deny docker rm *

# Prompt with message
deny pip install "Use uv pip install instead"
deny python "Use uv run python"
```

### File Operations

Auto-approve file operations using the same config:

```bash
allow-read src/**
allow-edit src/**
deny-read **/.env*
deny-edit **/.env*
ask-edit **/config.*
```

### Advanced Features

```bash
# Include external config files
include ~/.dippy/shared-rules
include .dippy-local-*

# Log settings
set log ~/.dippy/audit.log
set log-full
set log-rotate-max-days 30
set log-hook-approvals off

# Default behavior
set default ask                    # prompt (default)
# set default pass                 # let Claude decide
# set default allow                # auto-approve

# Context-aware rules
deny [!@subshell] cd *              # deny cd outside subshell
allow [@subshell] cd *              # allow (cd x && y)

# Custom wrappers
wrapper docker-exec
allow [docker-exec,prod] read *

# Option rules
allow-opt git status fetch log diff
deny-opt "git push" --force "Use --force-with-lease"

# MCP tools
allow-mcp mcp__github__get_*
deny-mcp mcp__*__delete_*

# After hook
after git commit * "Check project-tasks.md"
```

**Full documentation:** [docs/config.md](docs/config.md)

---

## Troubleshooting

### Not working?

```bash
# Run diagnostics
dippy doctor

# Check hook status
dippy hooks list

# Reinstall hooks
dippy hooks uninstall claude --global
dippy hooks install claude --global
```

### Check logs

- Claude Code: `~/.claude/hook-approvals.log`
- Gemini CLI: `~/.gemini/hook-approvals.log`
- Dippy audit: `~/.dippy/audit.log`

### Common Issues

- **Hook not triggering** → Run `dippy doctor` to diagnose
- **JSON syntax errors** → Check config with `dippy doctor`
- **Legacy hook detected** → Run `dippy hooks install <agent> --global`
---

## Uninstall

```bash
# Remove hooks
dippy hooks uninstall claude --global
dippy hooks uninstall gemini --global

# Uninstall tool
uv tool uninstall dippy
```

---

## pi-mono Extension

Dippy includes a TypeScript extension for [pi-mono](https://github.com/badlogic/pi-mono):

```bash
# Link the extension
ln -s /path/to/dippy/pi-extension/dippy-extension.ts \
      ~/.pi/agent/extensions/dippy-extension.ts
```

Uses your existing `~/.dippy/config` and `.dippy` files.

---

## Development

```bash
# Install in development mode
uv pip install -e .

# Run tests
uv run python -m pytest

# Run specific test
uv run python -m pytest tests/test_agents.py -v
```

---

**Full documentation:** [docs/config.md](docs/config.md)
**Upstream:** [ldayton/Dippy](https://github.com/ldayton/Dippy)
