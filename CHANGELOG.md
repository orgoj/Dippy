# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.9] - 2026-09-10

### Added

- Exclusive configuration through `DIPPY_CONFIG_ONLY` and `--config-only`, allowing role-specific processes to skip user and project configuration entirely.

## [0.3.8] - 2026-09-09

### Added

- `dippy run` and `dippy run-on-server` accept a command from one directly attached quoted heredoc. Dippy validates the literal script before local or remote execution; unquoted, empty, indirect or mixed-input forms remain on approval.

## [0.3.7] - 2026-09-08

### Added

- Custom wrappers can declare one quoted-heredoc script mode. Dippy analyzes the complete literal script as a remote command while keeping unquoted, indirect, empty or mixed-input forms on approval.

## [0.3.6] - 2026-09-06

### Fixed

- A bare `allow-web` now approves every web query, as documented. It was rejected as `requires a pattern` and skipped, so only `allow-web *` worked. `ask-web` and `deny-web` still require an explicit pattern.

## [0.3.5] - 2026-09-06

### Added

- Optional project SSH configuration and authentication socket settings for all `run-on-server` backends. Explicit profiles disable fallback to user authentication and use private per-operation control sockets; ordinary user SSH remains the default.

### Fixed

- Remote execution state is scoped to the project and server, and pending operations remain blocked after profile, backend or session changes and hard process termination. Corrupt state no longer silently clears the guard.
- tmux and Herdr execute each operation through a fresh local transport pane and SSH stdin, avoiding reuse of disconnected remote shells or another project's selected terminal. Recovery uses the recorded pane and checks connection configuration.

## [0.3.4] - 2026-09-02

### Added

- **Scoped handler delegation** — `delegate` rules pass matched commands to Dippy's native handler instead of approving them outright, so wrapper configurations can expose narrowly scoped read-only SSH commands while retaining handler validation.
- **SSH target context** — delegated SSH commands receive both `[ssh]` and the exact target token as context flags, enabling per-host inner-command rules.

### Fixed

- **SSH wrapper bypasses require approval** — forwarding, credential delegation, proxy, control-socket and similar SSH options (including clustered short options) no longer reach the read-only inner-command classifier, and every file-writing remote redirect remains approval-gated even when its path is locally writable.
- **Complete Bash redirect protection** — alternate file-writing forms such as `>|`, `<>`, numbered descriptors and variable descriptors now follow redirect rules; descriptor duplication such as `2>&1` remains safe.

## [0.3.3] - 2026-08-28

### Fixed

- **DuckDB writes honor redirect rules** — writes to the main database are automatically approved when its path matches `allow-redirect`, including multi-statement analysis with read-only `ATTACH`. Writable attachments, external file operations, dynamic database paths, and non-database objects continue to require approval.

## [0.3.2] - 2026-08-26

### Added

- **Context flags on file and web rules** — `allow-read`, `ask-read`, `deny-read`, `allow-edit`, `ask-edit`, `deny-edit`, `allow-web`, `ask-web`, and `deny-web` now support `[flags]` syntax (e.g., `[$HCOM_INSTANCE_NAME=agent]`), enabling per-agent file and web rule scoping.
- **GUI dialog single-key shortcuts & multi-monitor centering** — `dippy-askpass-gui` supports instant keyboard operation (`y`/`Y`/`Enter` = allow, `n`/`N`/`Esc` = deny) and centers itself in the geometric center of the active monitor via `xrandr`.
- **Multi-workspace session resolution** — file operations in multi-workspace AGY sessions match target paths against `workspacePaths` to locate and load the correct project `.dippy` configuration.

## [0.3.1] - 2026-08-26

### Added

- **Antigravity CLI (AGY) support** — native lifecycle hook integration for Antigravity CLI / AGY with named-hook format in `~/.gemini/config/hooks.json` and `.agents/hooks.json`.
- **AGY hook event mapping** — full compatibility with AGY `toolCall` payload structure, supporting `run_command` (shell execution), `view_file` / `grep_search` / `find_by_name` / `list_dir` (read tools), `write_to_file` / `replace_file_content` (edit tools), `search_web` / `read_url_content` (web search), `call_mcp_tool` (MCP tools), `PostToolUse`, and `Stop` hooks.
- **AGY Pure Control enforcement model** — in `--dangerously-skip-permissions` (YOLO) mode, Dippy enforces binary `allow`/`deny` decisions: automatically allowing safe commands, hard-blocking denied operations, and resolving `ask` classifications interactively through the configured `dippy-askpass-gui` provider (failing closed to `deny` if no askpass is configured).
- **AGY CLI commands & diagnostics** — `--agy` and `--antigravity` flags, agent auto-detection via `DIPPY_AGY`, and management via `dippy hooks install/uninstall/list agy` and `dippy doctor --agent agy`.

## [0.3.0] - 2026-08-25

### Added

- **Approved command execution** — `dippy run 'CMD'` classifies the exact command string, resolves `ask` through the configured askpass provider, and executes an approved command with Bash while preserving its exit code.
- **Managed remote execution** — `dippy run-on-server SERVER 'CMD'` accepts only explicitly declared `server` aliases and supports configured SSH, tmux, and Herdr transports. Remote rules receive `[run-on-server]` and `[run-on-server,SERVER]` context.
- **Fail-closed recovery** — uncertain remote results block later commands for that server and are never retried. `dippy recover SERVER` checks persistent backend markers; `--clear` explicitly releases a target after manual inspection.
- **Configuration administration** — `dippy config get/set/unset` and `dippy config server add/remove/list` atomically update user or project configuration while preserving unrelated rules and comments.
- **Tk approval provider** — `dippy-askpass-gui` displays read-only command context, supports an optional audit note, and denies on close or GUI failure. The calling Dippy process owns the configurable timeout, avoiding competing timers.
- **Configurable approval wait instruction** — `set approval-wait-message "..."` customizes the instruction appended to the fixed neutral waiting status, with normal user/project/final precedence. The default tells the agent to stop and wait for the user unless it can continue safely without the pending command; projects can instead direct it to a supervising agent or another authorization channel.

### Fixed

- **Approval timeout diagnostics** — the Tk dialog now shows the working directory, and `dippy run` reports that execution has not started while waiting. Approval timeouts fail closed with the configured duration, the original requested command, and safe retry guidance without exposing the askpass invocation. The default is 59 seconds so Dippy can normally report the error before a one-minute caller timeout.
- **Direct execution askpass override** — `dippy run` and `run-on-server` now honor `DIPPY_ASKPASS` when selecting the approval provider, matching the documented precedence and the existing hook path.
- **Unambiguous execution denials** — approval denials, rule denials, unavailable approval services, and timeouts now show the original requested command and explicitly state that it was not executed. User notes and rule reasons are preserved with safe next-step guidance. Runtime messages remain implementation-neutral so compatibility wrappers do not expose their enforcement mechanism or suggest an evasion path.
- **Execution config precedence** — explicit default-valued overrides such as project-level `set askpass-timeout 59` now replace a different user value, and `run`, `run-on-server`, and `recover` honor global `--config` and `--cwd` options.
- **Canonical config administration** — `dippy config set/unset` treats underscore and hyphen spellings as the same setting and rewrites the result with canonical hyphens instead of appending duplicates.
- **Persistent output fallback** — when a tmux or Herdr start marker has scrolled out but the completion marker remains, Dippy returns all output still retained in the capture buffer instead of returning none.

## [0.2.23] - 2026-08-23

### Added

- **`python-allow-symbol module.symbol`** - allows one name from a module too broad to trust as a whole: `python-allow-symbol sys.stdin` approves `from sys import stdin` (aliases included) while `import sys`, `from sys import exit`, multi-name imports, wildcards and relative imports keep asking. A `python-deny-module` the user wrote themselves beats the allowance; the built-in dangerous list does not. Design by nickdaview.
- **`cp` and `mv` destinations go through the redirect rules** - the same rules that already govern `>`, `tee` and `find -fprint`. `mv` also removes its sources, so those paths are checked too: `deny-redirect **/.dippy` now stops `mv .dippy /tmp/saved`, not just a write into it. A destination directory is expanded to the paths actually written (`cp -t /tmp a b` → `/tmp/a`, `/tmp/b`). Without a matching rule both still ask, exactly as before.
- **Cursor `preToolUse` hook** - `beforeShellExecution` ignores an `allow` answer and prompts anyway, which defeats the point of Dippy. `dippy hooks install cursor` now writes `preToolUse` with a `Shell` matcher instead. `beforeMCPExecution` keeps its old handling: its `tool_input` is a JSON string, not an object, so it still checks the top-level command. Reported by nickdaview.

### Fixed

- **Delegating handlers no longer lose shell quoting** - `uv run`, `env`, `arch`, `caffeinate` and `sudo` rebuilt the inner command with a bare `" ".join()`, so an argument containing metacharacters turned back into syntax: `uv run echo '(a)'` became a parse error, and `sudo echo 'a;zonk'` was analyzed as two commands. They now re-quote with `bash_join()`. `ssh` deliberately keeps the plain join - it hands its arguments to a remote *shell*, where the metacharacters really are syntax, and re-quoting would hide a remote compound command from analysis. Upstream #117, extended to the fork's own handlers.
- **Glob patterns in `after` rules match bare commands** - the trailing `' *'` fallback in `match_after` compared strings literally, so `after pyth?n *` matched `python foo` but not `python`. `match_command` was fixed for this in upstream #118; `match_after` was missed.
- **A relative import no longer inherits a module's safe status** - `from .json import loads` reads a local `json.py`, but the Python analyzer treated it as the stdlib module and approved it. Relative imports now always ask.

### Documentation

- `python-allow-module` and `python-deny-module` are standalone directives, not settings. The reference wrote them as `set python-allow-module numpy`, which is an unknown setting: skipped with a warning, leaving the module list empty and every import asking.

## [0.2.22] - 2026-08-23

### Added

- **File, MCP and web rule decisions name the config file they came from** - the reason was a bare `[.dippy*]`, which reads exactly the same whether the rule sits in `~/.dippy/config` or in a project `.dippy` that overrides it. It now reads `[.dippy* @ /home/u/proj/.dippy]`. Found the hard way: an `ask-edit .dippy*` in the user config appeared to be denying, and locating the project `deny-edit` that actually won - reached through an `include` - took a manual grep through four files. A rule's own message is user-facing guidance and is left untouched.

### Documentation

- `/**/tool` is the pattern form that matches every invocation path, relative and absolute alike - the reference documented the `*/tool` trap but never the way out.
- `*` and `**` in a command pattern skip whole tokens, so a glob in the middle is a bypass surface: `allow hcom * agent show *` also allows `hcom kill boom agent show x`.
- The built-in help/version approvals, so nobody writes rules that were never needed: `--help` and `-h` up to four tokens, `help`/`version`/`--version` only as the single argument, and none of it when `-c` or `-m` is present.

## [0.2.21] - 2026-08-23

### Fixed

- **`return` no longer needs approval** - 0.2.20 added `:`, `break`, `continue`, `shift` and `exit` to the shell-builtin section of `SIMPLE_SAFE` but overlooked `return`, so any command containing a shell function that returns an exit status asked for the keyword alone: `f() { return 1; }; echo hi` prompted on `return 1`. Found in a live audit log, like the original. The loop body is still judged on its own - `f() { rm -rf /; return 1; }; f` stays unapproved.

## [0.2.20] - 2026-08-22

### Fixed

- **Flow-control builtins no longer need approval** - `:`, `break`, `continue`, `shift` and `exit` were missing from the shell-builtin section of `SIMPLE_SAFE`, so any loop using one asked for the keyword alone, however harmless the rest of the pipeline was. Found in a live audit log: `for f in bin/*; do [ -f "$f" ] || continue; ...; done` prompted on `continue`.
- **CLI-mode tests no longer read the developer's own config** - `run_dippy()` in `tests/test_cli_mode.py` spawned a subprocess with the ambient `HOME` and cwd, so three assertions about built-in behaviour were really asserting on whatever `~/.dippy/config` happened to contain. They now run in an empty HOME and cwd.

### Documentation

- Two ways a rule can silently match nothing: a pattern token containing `/` is resolved against cwd even when it holds a glob (so a leading `*/` never matches an absolute path), and a wrapper is recognised only by its bare name (so a path-qualified invocation is not unwrapped).

## [0.2.19] - 2026-08-21

### Fixed

- **`_emit()` restored** - The helper was deleted in 62d49af while eight call sites kept referencing it. Every one of those paths raised `NameError`, which the top-level handler turned into a generic `ask`, so the specific reason ("unsupported tool: X", "no file path provided") never reached the user. The idle-notifier path was equally broken by a missing `expand_template` import.
- **Explicit mode is re-read per call** - `main()` used the import-time `_EXPLICIT_MODE` constant, so an explicit `--gemini`/`DIPPY_GEMINI` could be overridden by input auto-detection. The flags are now read when the call runs.
- **Python module lists survive config merging** - `python-allow-module` and `python-deny-module` in a project config were silently discarded by `_merge_configs()`; a project-level `python-deny-module` had no effect at all. Both lists now accumulate like the other rule lists.

## [0.2.18] - 2026-08-21

### Added

- **Environment context flags** - New `set context-env VAR` directive (repeatable) exposes an environment variable as the context flag `[$VAR=value]`. Rules can now be scoped per agent (for example `[$HCOM_INSTANCE_NAME=bot1]`) inside a single config, and combined with wrapper and AST flags. Unset or empty variables produce no flag, so guarded `allow` rules stay fail-closed.

## [0.2.17] - 2026-05-14

### Fixed

- **Gemini CLI hook protocol compliance** - Fixed a compatibility issue where Gemini CLI would report a "Hook failed" error on denied commands. Dippy now correctly returns a JSON response with `decision: "deny"` instead of exiting with code 2, matching the newer Gemini CLI hook protocol.

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
