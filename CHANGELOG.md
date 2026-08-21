# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.18] - 2026-08-21

### Added

- **Environment context flags** - New `set context-env VAR` directive (repeatable) exposes an environment variable as the context flag `[$VAR=value]`. Rules can now be scoped per agent (for example `[$HCOM_INSTANCE_NAME=bot1]`) inside a single config, and combined with wrapper and AST flags. Unset or empty variables produce no flag, so guarded `allow` rules stay fail-closed.

## [0.2.17] - 2026-05-14

### Fixed

- **Gemini CLI hook protocol compliance** - Fixed a compatibility issue where Gemini CLI would report a "Hook failed" error on zablokované (denied) příkazy. Dippy now correctly returns a JSON response with `decision: "deny"` instead of exiting with code 2, matching the newer Gemini CLI hook protocol.

## [0.2.16] - 2026-05-14

### Added

- **Gemini YOLO mode automation** - Added `dippy hooks setup-gemini-yolo` to configure Gemini CLI for "Pure Dippy Control". This sets `approvalMode: "yolo"` in Gemini's `settings.json`, allowing Dippy to act as the primary authority for command approvals without Gemini's redundant secondary prompts.
- **Enhanced Gemini diagnostics** - `dippy doctor` now checks Gemini's `approvalMode` and recommends YOLO mode for the best Dippy experience.

### Documentation

- Added comprehensive guide for "Pure Dippy Control" in `docs/hook-systems/gemini-cli-hooks.md`.
- Updated `README.md` with Gemini YOLO mode instructions.

## [0.2.15] - 2026-05-13

### Fixed

- **Codex command auto-approval on current Codex** - Codex shell command approvals now use the `PermissionRequest` hook event with `decision.behavior = "allow"`, matching current Codex behavior where `PreToolUse` no longer grants execution approval.
- **Codex hook installation** - `dippy hooks install codex` now installs `PreToolUse`, `PermissionRequest`, and `PostToolUse` Bash hooks.
- **Codex feature flag compatibility** - The installer now writes the current `[features] hooks = true` flag while still detecting legacy `codex_hooks = true` configs.

### Documentation

- Updated Codex hook docs for current `PermissionRequest` approval flow and `hooks` feature flag.
- Documented the Gemini CLI 0.42 limitation where `BeforeTool` `allow` continues to Gemini's normal policy instead of auto-approving shell commands.

## [0.2.14] - 2026-04-22

### Added

- **Python `-c` inline code AST analysis** - `python -c 'code'` is now statically analyzed for safety instead of always requiring confirmation. Safe code (no I/O, no dangerous imports) is auto-approved. (Design from nickdaview/python-c-analysis)
- **Configurable Python module lists** - New `python-allow-module` and `python-deny-module` config directives let users customize which Python modules are considered safe or dangerous during static analysis.
  - Example: `python-allow-module numpy` whitelists numpy imports in inline code.
  - Example: `python-deny-module requests` blocks requests even though it's not in the hardcoded dangerous list.
- **Bash expansion detection in `-c` arguments** - When the `-c` code argument contains shell expansions (`$VAR`, `$(cmd)`), Dippy falls back to `ask` since the code can't be statically analyzed.

### Fixed

- **`__getattribute__` reflection bypass** - Added `__getattribute__` to REFLECTION_ATTRS to prevent bypassing the AST analyzer via `obj.__getattribute__('__globals__')` (reported by Codex GPT-5.4 review).
- **`--help` after `-c` bypass** - `python -c 'malicious' --help` no longer gets auto-approved via the help-flag shortcut. The analyzer now checks for `-c`/`-m` before matching version/help patterns.

## [0.2.13] - 2026-04-12

### Fixed

- **Codex hook config format** - `dippy hooks install codex` now writes the Codex-native nested hook format that current Codex actually parses
  - Uses `matcher` plus nested `hooks` with `type: "command"` and `command: "dippy --codex"`
  - Removes legacy flat Codex hook entries during reinstall/uninstall
  - Prevents false "installed" status when stale flat entries exist but Codex has no runnable handlers
- **Codex entrypoint isolation** - Hook subprocess tests now isolate `HOME` and `DIPPY_CONFIG`, so Codex-mode deny behavior is tested without leaking real user config or audit-log state

### Documentation

- Corrected project docs to describe the real Codex hook format and matcher model
- Added README guidance for manual Codex hook config and session restart after hook changes

## [0.2.12] - 2026-04-11

### Fixed

- **Bulk hook install CLI** - `dippy hooks install --all` now installs the full Dippy hook set for every supported agent instead of failing on the positional `agent` argument
  - `hooks install` now accepts an omitted agent only when `--all` is provided
  - Bulk install traverses Claude, Gemini, Cursor, Windsurf, and Codex
  - `dippy hooks install <agent> --all` keeps its existing meaning of installing the full hook set for a single agent
  - Added regression tests for parser behavior, missing-agent validation, and bulk project-local installs

## [0.2.11] - 2026-04-11

### Fixed

- **Codex doctor coverage** - `dippy doctor` now diagnoses OpenAI Codex CLI alongside Claude, Gemini, Cursor, and Windsurf
  - Detects Codex hook presence from `.codex/hooks.json`
  - Validates `codex_hooks = true` in `.codex/config.toml`
  - Warns when hooks are installed but the Codex feature flag is missing
  - Includes Codex hook approval log health in diagnostics
- **Codex hook status reporting** - `dippy hooks list` now reports Codex feature-flag state in both text and JSON output, so status is accurate when `hooks.json` and `config.toml` diverge

### Documentation

- Added Codex to the supported agents list in README hooks management docs
- Documented Codex feature-flag validation in doctor checks
- Corrected Codex configuration references in hook system comparison docs to distinguish `hooks.json` from `config.toml`

## [0.2.9] - 2026-04-11

### Added

- **Codex CLI Hooks** - Full hook support for OpenAI Codex CLI
  - PreToolUse/PostToolUse hooks for Bash command interception
  - Stop hook for notifier continuation
  - Dual-config install: `hooks.json` (hook definitions) + `config.toml` (feature flag)
  - `dippy hooks install codex --global` / `dippy hooks install codex`
  - Codex-specific response format following official wire protocol
  - `codex_hooks = true` feature flag automatically enabled on install

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

- **Gemini fail-open security** - Error paths in Gemini mode now return `ask` instead of `allow`, closing an inadvertent security bypass where hook failures would silently approve commands
- **Config merge** - `_merge_configs()` now correctly merges `aliases`, `log_rotate_max_days`, and `log_hook_approvals` from project config over global config
- **HandlerContext cwd** - Analysis now passes working directory through `HandlerContext.cwd`, enabling Python handler to resolve relative script paths correctly
- **Lazy handler loading** - `_discover_handlers()` now uses AST scan instead of importing every handler module at startup, reducing hook process cold-start time
- **MCP statusline shell injection** - Removed `shell=True` from MCP cache refresh subprocess; cache now written via Python file APIs with atomic rename

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
