# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Idle Prompt Notifications** - Notification support for Claude Code idle state
  - New configuration `set idle-notifier-command "CMD"` with template expansion
  - Template placeholders: `{title}`, `{message}`, `{cwd}`, `{notification_type}`
  - Shell-safe escaping for double-quoted context (backslashes, quotes, dollar signs, backticks)
  - Example: `set idle-notifier-command notify-send "{title}" "{message}"`
- **Universal Notifier (Sidekick)** - Support for external notification commands
  - New configuration `set notifier-command "CMD"` for calling external scripts (e.g. mail check).
  - Support for `set notifier-include "list"` to limit calls to specific tools or commands.
  - Support for **Idle mode** (`--idle`) in `Stop`/`AfterAgent` hooks for long-polling notifications.
  - Integration with all agents: Claude Code, Gemini CLI, Cursor, and pi-mono.
  - Automatic wrapping of messages in `<notification_note>` tag.
  - Agent continuation enforcement (block stop) when notification is delivered in idle mode.
- **Generic Wrapper Enhancements** - Enhanced `wrapper` directive with subcommand and target flag support
  - New syntax: `wrapper <name> [subcommand_trigger] [target_flag]`
  - Automatically extracts target/destination (e.g., server name) and provides it as a context flag
  - Automatically analyzes inner commands starting after the trigger word
  - Support for custom target flags (e.g., `-t`, `-h`) with automatic fallback to first non-option token
  - Enforces `remote=True` for inner commands, skipping local path checks for remote operations

## [0.2.7] - 2026-03-31

### Fixed

- **Env-stripped rule matching** - Commands with environment variable prefixes (e.g., `UV_PROJECT_ENVIRONMENT=.venv-3.12 uv run pytest`) now correctly match config rules like `allow uv run *`. Both raw and env-stripped forms are tried in a single pass, preserving last-match-wins semantics.
- **SSH remote flag** - SSH handler now sets `remote=True` on delegated inner commands, preventing incorrect local path expansion for remotely executed commands.

### Added

- **Audit log suggestion field** - Ask decisions in the audit log now include a `suggestion` field (gated behind `set log_full`) showing the env-stripped command pattern. Copy-paste ready for `allow` rules. Only set for command-matching asks (not redirect/substitution asks).

## [0.2.4] - 2026-02-07

### Added

- **Gemini CLI Support** - Full integration with Gemini CLI hooks
  - Proper response format with `systemMessage` and `continue` fields
  - Deny via stderr + exit code 2 (blocks tool without confirmation dialog)
  - Normalized event names (BeforeTool→PreToolUse, AfterTool→PostToolUse)
  - Support for `google_web_search`, `write_file`, `replace`, `read_file`, `read_many_files` tools
  - See [Gemini CLI Setup Guide](docs/hook-systems/gemini-cli-setup.md)
- **Multi-Agent Support** - Dedicated modes for Claude, Gemini, Cursor, pi-mono, Moltbot, Codex, Windsurf, PearAI
  - Each agent has its own log file (e.g., `~/.gemini/hook-approvals.log`)
  - CLI flags (`--gemini`, `--pi`, etc.) and env vars (`DIPPY_GEMINI=1`, etc.)
- **CLI Mode Enhancements** - New options: `--agent`, `--remote`, `--version`
- **Agent Identification** - Audit log now includes `agent` field (`claude`, `gemini`, `cursor`, `pi`, `cli`)
- **File Read Approval** - `allow-read`, `ask-read`, `deny-read` rules
  - Added full support for `Read` tool in Claude Code and pi-mono
  - Native config directives for read operations (replaces synthetic cat checks)
  - Consistent glob matching with file edit rules
- **pi-mono extension enhancement** - Full tool validation for [pi-mono](https://github.com/badlogic/pi-mono)
  - Added file access control for `read`, `write`, and `edit` tools
  - Integrated native `allow-edit` and `allow-read` rules
  - Updated `pi-extension/dippy-extension.ts` and `src/dippy/pi_wrapper.py` for multi-tool support
  - Updated [pi-extension/README.md](pi-extension/README.md) with file-specific configuration examples

### Changed

- **Mode detection** - Removed auto-detection from input JSON; mode is now strictly from flags/env or defaults to Claude

## [Previous Versions]

See upstream [ldayton/Dippy](https://github.com/ldayton/Dippy) for changes before this fork.

### Fork Features (not in upstream)

- **Notifier (Sidekick)** - `set notifier-command "CMD"` for external alerts (mail, status).
- File Edit/Read Approval - `allow-edit`, `allow-read` etc. rules
- Include directive - `include <path-or-glob>` for composable configs
- Context-aware rules - `[flags]` syntax with `@subshell`, `@compound`, negation
- Custom wrappers - `wrapper <name>` for project-specific tools
- Option rules - `allow-opt`, `ask-opt`, `deny-opt` for subcommand control
- WebSearch support - auto-approval for WebSearch tool
- Structured JSON output - for PostToolUse hooks
- SSH/sudo handlers - remote context support
- Log rotation - `set log-rotate-max-days N`
- Hook approvals log control - `set log-hook-approvals off`
- Hybrid mode - `set default pass`
- Audit log - `cwd` field added
- CLI mode - standalone command validation
