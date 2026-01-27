# Dippy Configuration

## Design Principles

- **Not adversarial** - protecting against AI mistakes, not malicious actors
- **Favor expressivity** - let users say what they mean easily
- **Favor simplicity** - no complex syntax to learn
- **Favor familiarity** - use patterns people already know

## Overview

Dippy's config system extends the built-in approval rules. Line-based, glob patterns, last-match-wins.

## CLI Mode

Dippy can validate commands from the command line without running as a hook. This is useful for scripting, testing rules, or integrating with other AI tools.

### Usage

```bash
# Validate a command
dippy --cmd 'rm -rf /'

# JSON output
dippy --cmd 'ls -la' --json

# Read from stdin
echo 'git status' | dippy --stdin

# Specify working directory
dippy --cmd 'make build' --cwd /path/to/project

# Use custom config file
dippy --cmd 'docker run nginx' --config ~/.dippy/strict.conf
```

### Options

| Option | Description |
|--------|-------------|
| `--cmd COMMAND` | Command to validate |
| `--stdin` | Read command from stdin (plain text, not JSON) |
| `--cwd PATH` | Working directory (default: current) |
| `--json` | Output as JSON instead of text |
| `--config PATH` | Config file override (highest priority) |

### Exit Codes

| Code | Decision | Meaning |
|------|----------|---------|
| `0` | allow | Command is safe |
| `1` | deny | Blocked by rule |
| `2` | ask | Needs user approval |

### Output Formats

**Text (default):**
```
allow: ls
deny: rm: dangerous operation
ask: docker run: needs approval
```

**JSON (`--json`):**
```json
{"decision": "allow", "reason": "ls"}
{"decision": "deny", "reason": "rm: dangerous operation"}
{"decision": "ask", "reason": "docker run: needs approval"}
```

### Examples

**Scripting:**
```bash
if dippy --cmd "$cmd"; then
    eval "$cmd"
else
    echo "Command blocked by Dippy"
fi
```

**Batch validation:**
```bash
while read cmd; do
    result=$(dippy --cmd "$cmd" --json)
    echo "$cmd → $(echo "$result" | jq -r .decision)"
done < commands.txt
```

**Testing config rules:**
```bash
# Test if a rule works as expected
dippy --cmd 'docker run --privileged nginx' --config .dippy
# → ask: docker run: needs approval
```

## File Locations

| Location          | Purpose          |
| ----------------- | ---------------- |
| `~/.dippy/config` | User global      |
| `.dippy`          | Project-specific |
| `$DIPPY_CONFIG`   | Env override     |
| `--config PATH`   | CLI mode only    |

**Load order** (last match wins):
1. `~/.dippy/config` - user defaults
2. `.dippy` - project overrides
3. `$DIPPY_CONFIG` - env override (highest precedence)

Project config is found by walking up from cwd to filesystem root, stopping at the first `.dippy` found (like `.git` discovery).

## Syntax

```
allow <glob>                   # auto-approve matching commands
ask <glob>                     # always prompt user for matching commands
ask <glob> "message"           # prompt with message shown to AI
deny <glob>                    # reject matching commands (no user prompt)
deny <glob> "message"          # reject with message shown to AI

allow-redirect <glob>          # allow output redirects to matching paths
ask-redirect <glob>            # prompt for output redirects to matching paths
ask-redirect <glob> "message"  # prompt with message shown to AI
deny-redirect <glob>           # reject output redirects to matching paths
deny-redirect <glob> "message" # reject with message shown to AI

allow-edit <glob>            # allow file edits to matching paths
ask-edit <glob>              # prompt for file edits
ask-edit <glob> "message"    # prompt with message shown to AI
deny-edit <glob>             # reject file edits
deny-edit <glob> "message"   # reject with message shown to AI

allow-read <glob>            # allow file reads to matching paths
ask-read <glob>              # prompt for file reads
ask-read <glob> "message"    # prompt with message shown to AI
deny-read <glob>             # reject file reads
deny-read <glob> "message"   # reject with message shown to AI

after <glob>                   # post-action feedback (silent)
after <glob> "message"         # post-action feedback with message to AI

include <path-or-pattern>      # include external config file(s)
wrapper <command_name>         # define custom wrapper command

set <key> [value]              # settings
```

### Include Directive

The `include` directive allows you to split configuration across multiple files for better organization and reusability.

```
include <path-or-pattern>
```

**Features:**

- **Glob patterns**: `include .dippy-ok-*` includes all matching files
- **Home expansion**: `include ~/.dippy/shared-rules` expands `~`
- **Relative paths**: Resolved relative to the including file's directory
- **Recursive**: Included files can include other files
- **Circular detection**: Raises error on circular includes
- **Inline expansion**: Content inserted as if written at that location

**Examples:**

```
# Include shared project rules
include .dippy-local-*

# Include team defaults from home
include ~/.dippy/team-defaults

# Include all developer-specific overrides
include .dippy-dev-*

# Include with glob pattern
include conf.d/*.conf
```

**Precedence:**
- Included rules are inserted at the point of inclusion
- Last-match-wins applies across includes
- Later includes override earlier ones

**Error handling:**
- Missing files: Warning logged, config continues loading
- Circular includes: ConfigError raised immediately
- Empty pattern: Warning logged, skipped

### Context Flags

Rules can be restricted to specific execution contexts using `[flags]` syntax:

```
allow [flags] <glob>           # only match when conditions are met
deny [flags] <glob>
ask [flags] <glob> "message"
```

**Available flags:**

| Flag | Context | Example |
|------|---------|---------|
| `@subshell` | Inside `(...)` | `(cd /tmp && make)` |
| `@bracegroup` | Inside `{ ...; }` | `{ cd /tmp; make; }` |
| `@pipeline` | Part of `cmd1 \| cmd2` | `cat file | grep x` |
| `@compound` | Any compound context (subshell, bracegroup, pipeline, list) | All of above |
| `ssh` | Inside `ssh "command"` | `ssh host "rm /tmp/*"` |
| `sudo` | Inside `sudo`, `doas`, `pkexec` | `sudo rm /etc/passwd` |
| `<custom>` | Inside user-defined wrappers | `wrap server1 free -h` |

### Custom Wrappers

Define your own wrapper commands with the `wrapper` directive:

```
wrapper <command_name>
```

**Example:**

```
# Define custom wrapper
wrapper wrap

# Allow free on server1 via wrap
allow [wrap,server1] free *

# Deny rm on server1 (any wrapper)
deny [server1] rm *
```

**Usage:**

```bash
wrap server1 free -h          # wrapper=wrap, dest=server1, cmd=free -h
wrap server1 ls -la           # Allowed
wrap server1 rm /tmp/x        # Denied
```

**Wrapper syntax:**

```
<wrapper> [options] <destination> [inner_command]
```

- `wrapper`: Command name defined in config
- `options`: Optional flags (same as SSH: `-p`, `-i`, `-l`, etc.)
- `destination`: First non-option token (becomes context flag)
- `inner_command`: Everything after destination (can be empty for interactive)

**Context flags from wrappers:**

Custom wrappers automatically set TWO context flags:
1. Wrapper name: `["wrap"]`
2. Destination: `["server1"]`

Both flags are available for rule matching:

```
allow [wrap,server1] free *    # Both wrapper AND destination
allow [server1] free *          # Destination only (any wrapper)
allow [wrap] free *             # Wrapper only (any destination)
deny [!wrap] rm *              # Negation: rm WITHOUT wrapper
```

**Multiple wrappers:**

```
wrapper wrap
wrapper tmux-cli
wrapper ssh-gateway
```

Wrappers merge via set union across config scopes (user + project).

**Built-in wrappers vs custom:**

Built-in wrappers (`ssh`, `sudo`) work the same way but are predefined. Custom wrappers let you define project-specific tools with the same context-aware control.

**Flag syntax:**
- AST context flags use `@` prefix: `@subshell`, `@compound`
- Wrapper flags have no prefix: `ssh`, `sudo`
- Multiple flags use AND logic: `[@subshell,ssh]` requires BOTH
- **Negation:** `!` prefix means flag must NOT be present: `[!@subshell]`
- Rules without flags match any context (backward compatible)

**Example: Deny `cd` outside subshells**

```
deny [!@subshell] cd *          # deny cd when NOT in subshell
```

This is equivalent to:
```
deny cd *                      # block standalone cd
allow [@subshell] cd *         # but allow in subshells
```

**Example: Allow `rm` via SSH only in `/tmp`**

```
deny [ssh] rm *                # deny rm via SSH anywhere
allow [ssh] rm /tmp/**         # but allow in /tmp
```

**Example: Mixed required and negated flags**

```
allow [ssh,!@subshell] rm *    # ssh required, but NOT in subshell
```

This matches `ssh host "rm /tmp/x"` but NOT `ssh host "(rm /tmp/x)"`.

**How it works:**

When Dippy analyzes `ssh host "(rm /tmp/x)"`:
1. Parser detects outer `ssh` wrapper → sets `ssh` flag
2. Parser detects inner `(...)` subshell → sets `@subshell` and `@compound` flags
3. Rule `allow [ssh] rm *` matches (ssh flag present)
4. Rule `deny [ssh,!@subshell] rm *` does NOT match (@subshell negated)
5. Rule `allow [@compound] rm *` matches (@compound present)

**Escaping in patterns:** Use `[*]`, `[?]`, `[[]` to match literal glob characters.

**Escaping in messages:** Use `\"` for literal quotes, `\\` for literal backslash.

**Tilde expansion:** `~` expands to home directory in both patterns and commands. Environment variables (`$HOME`) are not expanded.

**One rule per line.** No line continuation.

**Forgiving parsing.** Invalid lines (unknown directives, malformed rules) are logged and skipped. Valid rules in the same file still take effect. Check `~/.claude/hook-approvals.log` for warnings.

## Messages

When an `ask`, `deny`, or their `-redirect` variants match, an optional message can be shown to the AI explaining why approval is needed (or why the command was rejected) and what to do instead. This helps the AI learn and adjust.

```
ask git push --force * "Use --force-with-lease instead"
deny rm -rf /* "Too dangerous - be more specific about what to delete"
ask *prod* "Production commands require manual review"
deny-redirect .env* "Never write secrets to env files"
```

If no message is provided, a default is generated from the pattern (e.g., `"deny: rm -rf /*"`).

Only `ask` and `deny` support messages - approval messages don't reliably reach the AI across all platforms.

## Pattern Matching

Dippy uses two pattern styles depending on context:

### Command Patterns

For `allow`, `ask`, `deny` rules, patterns match the full command string:

- `*` matches any characters (including spaces and none)
- `?` matches exactly one character
- `[abc]` matches any of a, b, or c
- `[a-z]` matches any character in range
- `[!abc]` or `[^abc]` matches any character NOT in set

**Trailing `*` matches bare commands.** The pattern `python *` matches both `python foo` AND bare `python`. To match only commands with arguments, use `?*`:
```
allow python ?*   # matches 'python foo', NOT bare 'python'
allow python *    # matches both 'python foo' AND bare 'python'
```

### Path Patterns

For redirect and file rules (`*-redirect`, `*-edit`, `*-mcp`), patterns match paths:

- `*` matches any characters except `/`
- `**` matches any characters including `/` (recursive)
- `?`, `[abc]`, `[a-z]`, `[!abc]` work as above

```
src/*      # matches src/foo.go, NOT src/sub/foo.go
src/**     # matches src/foo.go AND src/sub/foo.go
**/test.*  # matches test.py, src/test.py, src/sub/test.py
```

**Last match wins.** Rules are evaluated top-to-bottom; the last matching rule determines the decision. This allows broad rules followed by specific exceptions.

**Strictest wins across types.** When a command has both command rules and redirect rules matching, the most restrictive decision wins: `deny` > `ask` > `allow`. This prevents accidentally allowing dangerous redirects just because the command itself was allowed.

**Config wins over built-ins.** If a config rule matches, it takes precedence over Dippy's built-in safety handlers. Config represents explicit user intent.

**Path normalization:**

Both commands and patterns are normalized before matching:
- `~` expands to home directory
- `./foo` and `../foo` resolve against cwd to absolute paths
- Relative paths without `./` (e.g., `bin/foo`) resolve against cwd to absolute paths

```
# Config (cwd: /home/user/project)
allow node bin/*

# Command: node bin/script.js
# Normalized: node /home/user/project/bin/script.js
# Pattern normalized: node /home/user/project/bin/*
# → matches!

# Command: node /home/user/project/bin/script.js
# → also matches (already absolute, pattern normalized)

# Command: node /other/path/bin/script.js
# → does NOT match (different absolute path)
```

If no rule matches, built-in handlers decide.

## Command Rules

```
# Trust tools
allow just *
allow uv run *
allow python3 *
allow ~/bin/*

# Trust specific git operations
allow git stash pop
allow git stash apply
allow git checkout -- *

# Prompt for review
ask *prod* "Production commands require manual review"

# Hard blocks (no user override)
deny rm -rf /* "Too dangerous"
deny git push --force * "Use --force-with-lease instead"
```

Inverse patterns via ordering (last match wins):

```
# Allow all docker EXCEPT rm/rmi
allow docker *
ask docker rm *
ask docker rmi *

# Allow rm but never rm -rf /
allow rm *
deny rm -rf /*
```

## Redirect Rules

Redirect patterns match the target path, normalized:
- Trailing slashes are stripped (`/tmp/foo/` → `/tmp/foo`)
- `~` expands to home directory
- Relative paths are resolved against cwd

Supports `**` for recursive directory matching:

- `**` matches zero or more directories
- `/tmp/**` matches `/tmp/foo`, `/tmp/a/b/c`, etc.
- `**/foo` matches `/foo`, `/a/foo`, `/a/b/c/foo`
- `/tmp/**/file.txt` matches `/tmp/file.txt`, `/tmp/a/file.txt`, `/tmp/a/b/file.txt`

```
# Allow temp paths
allow-redirect /tmp/**
allow-redirect .cache/**
allow-redirect **/*.log

# Prompt for review
ask-redirect **/.*              # all hidden files

# Hard blocks (no user override)
deny-redirect **/.env* "Never write secrets to env files"
deny-redirect **/*credential* "Never write credential files"
deny-redirect /etc/** "System config is off-limits"
```

Note: `**` is only supported in redirect rules. Command rules use standard fnmatch globs.

## Option Rules

Option rules provide fine-grained control over specific subcommands or flags. Unlike normal pattern matching, option rules match if:

1. The command starts with the specified **prefix**
2. Any **item** from the list appears anywhere in the command

```
allow-opt <prefix> <item1> <item2>...
ask-opt <prefix> <item1> <item2>... ["message"]
deny-opt <prefix> <item1> <item2>... ["message"]
```

The prefix can be a single word or quoted (e.g., `"git commit"`). Items are subcommands or flags that trigger the rule.

**Examples:**

```
# Allow specific read-only git subcommands
allow-opt git status fetch log diff show ls-files ls-tree

# Block git commit with --no-verify (anywhere in command)
deny-opt "git commit" --no-verify "Don't skip pre-commit hooks"

# Prompt for force push
ask-opt "git push" --force "Use --force-with-lease instead"

# Allow docker inspection commands
allow-opt docker ps inspect logs top stats

# Block dangerous docker flags
deny-opt "docker run" --privileged
deny-opt "docker run" --volume /:/host
```

**Matching behavior:**

| Command | Rule | Match? |
|---------|------|--------|
| `git status --short` | `allow-opt git status` | ✅ |
| `git fetch origin` | `allow-opt git status fetch` | ✅ |
| `git log --oneline` | `allow-opt git status fetch log` | ✅ |
| `git commit -m x` | `allow-opt git status fetch` | ❌ |
| `git commit --no-verify -m x` | `deny-opt git commit --no-verify` | ✅ |
| `git push --force origin` | `ask-opt git push --force` | ✅ |
| `git push origin main` | `ask-opt git push --force` | ❌ |
| `git push --force-with-lease` | `ask-opt git push --force` | ❌ (exact match only) |

**Important:** Item matching uses **exact word-boundary** matching. `--force` does NOT match `--force-with-lease`. For prefix matching, use normal glob rules like `deny "git push *--force*"`.

**Mixing with normal rules:**

Option rules mix with normal `allow`/`ask`/`deny` rules. Last match wins:

```
# Allow all git commands
allow git *

# But block force push specifically
deny-opt "git push" --force

# But allow force-with-lease
allow-opt "git push" --force-with-lease
```

For prefix matching (e.g., block all `--force*` variants), use glob patterns:

```
# Block all --force* variants using glob
deny "git push *--force*"

# Explicitly allow the safer variant (last match wins)
allow "git push --force-with-lease"
```

**Multi-word prefixes:**

Use quotes for prefixes with multiple words:

```
deny-opt "git commit" --no-verify
ask-opt "git stash" drop pop
deny-opt "docker run" --privileged
```

**Use cases:**

- Whitelist safe subcommands (e.g., `git status`, `git log`)
- Blacklist dangerous flags (e.g., `--force`, `--no-verify`, `--privileged`)
- Enforce safer alternatives (block `--force`, suggest `--force-with-lease`)
- Project-specific constraints (block `--global` config changes, etc.)

## Bash Test Constructs

Allow bash `[ ]` and `[[ ]]` test commands:

```
# allow bash test (first [] is flag)
allow [] [[] *
```

## Settings

**Boolean flags** (no value):
```
set log-full             # log full commands (requires log path set)
```

**Value settings:**
```
# Default behavior when no rule matches (default: ask)
set default ask          # Prompt for approval - safest option
set default pass         # Don't intercept - let Claude's permission system decide
set default allow        # Auto-approve everything without explicit rule

# Logging
set log ~/.dippy/audit.log  # enable logging to path
set log-rotate-max-days 30  # keep rotated logs for N days (0 = disable)
set log-hook-approvals off  # disable hook-approvals.log
```

Settings use kebab-case or snake_case interchangeably.

### Default Behavior

The `set default` directive controls what happens when a command doesn't match any explicit rule:

| Value | Behavior | Use Case |
|-------|----------|----------|
| `ask` | Prompt user for approval | Safest - explicit approval for unknown commands |
| `pass` | Return empty response; Claude handles it | Hybrid - Dippy only handles explicitly configured rules |
| `allow` | Auto-approve | YOLO mode - trust everything not explicitly blocked |

## Logging

**Default: no logging.**

When enabled with `set log <path>`, logs are structured (JSON) with minimal info:

```json
{"ts": "2024-01-15T10:23:45Z", "decision": "allow", "cmd": "git", "rule": "allow git stash *"}
{"ts": "2024-01-15T10:23:52Z", "decision": "ask", "cmd": "git", "rule": "ask git push --force *", "message": "Use --force-with-lease"}
{"ts": "2024-01-15T10:24:01Z", "decision": "deny", "cmd": "rm", "rule": "deny rm -rf /*", "message": "Too dangerous"}
{"ts": "2024-01-15T10:24:15Z", "decision": "ask", "cmd": "rm"}
```

The `cmd` field is best-effort extraction of the base command - may be wrong if parsing fails.

With `set log-full`, a `command` field is added containing the full command string. **Warning: may contain secrets.**

## Example

```
# ~/.dippy/config

allow just *
allow uv run *
allow python3 *
allow ~/bin/*

allow-redirect /tmp/*
allow-redirect .cache/*
deny-redirect .env* "Never write secrets"

```

```
# .dippy (project)

allow ./tools/*
allow git stash *
allow git checkout -- *
deny git push --force * "Use --force-with-lease"

allow-redirect ./build/*
```

## MCP Tool Rules

MCP tools follow the pattern `mcp__<server>__<tool>`. Dippy controls which MCP operations are allowed per-project.

### Syntax

```
allow-mcp <pattern>
ask-mcp <pattern>
ask-mcp <pattern> "message"
deny-mcp <pattern>
deny-mcp <pattern> "message"

after-mcp <pattern>
after-mcp <pattern> "message"
```

Patterns use fnmatch globs against the full tool name.

### Example

```
# Allow read-only GitHub operations
allow-mcp mcp__github__get_*
allow-mcp mcp__github__list_*
allow-mcp mcp__github__search_*

# Prompt for writes
ask-mcp mcp__github__create_* "Creating GitHub resources"
ask-mcp mcp__github__update_* "Modifying GitHub resources"

# Block destructive operations
deny-mcp mcp__github__delete_* "No deletions without manual review"
deny-mcp mcp__github__merge_* "Merges need manual approval"

# Post-action feedback
after-mcp mcp__github__create_* "Check the GitHub UI to verify"
```

### Opting In

To enable MCP rules, update your hook matcher in `settings.json`:

```json
"matcher": "Bash|mcp__.*"
```

## After Rules

After rules provide feedback to the AI after a command completes. They use Claude Code's PostToolUse hook.

```
after <glob>           # silent - matches, no message
after <glob> ""        # silent - matches, no message
after <glob> "message" # matches, sends message to Claude
```

Like other rules, last match wins. A silent `after` (with no message or empty string) overrides earlier matches—useful for excluding specific commands from a broad rule.

```
# Remind Claude after git operations
after git push * "Re-read the prompt file for next steps"
after git commit * "Update your todo list"

# Broad rule with specific override
after npm * "Check for any errors in the output"
after npm install * ""   # no message needed for install

# Project-specific workflows
after ./deploy.sh * "Verify deployment in staging before continuing"
after make test * "Review test output and fix any failures"
```

When an `after` rule matches:
1. Dippy outputs the message to stdout
2. Claude receives it as post-action feedback
3. Claude can adjust its behavior based on the message

Unlike PreToolUse rules (which control permission), after rules are purely informational—they cannot block or modify.

To enable after rules, register Dippy for PostToolUse in `settings.json` (see Installation in README).

## WebSearch Rules

WebSearch rules control approval for Claude's web search tool. Patterns match against the search query string.

### Syntax

```
allow-web                      # auto-approve all web searches
allow-web <pattern>            # auto-approve searches matching pattern
ask-web <pattern>              # prompt for searches matching pattern
ask-web <pattern> "message"    # prompt with message shown to AI
deny-web <pattern>             # block searches matching pattern
deny-web <pattern> "message"   # block with message shown to AI

after-web <pattern>            # post-search feedback (silent)
after-web <pattern> "message"  # post-search feedback with message to AI
```

Patterns use fnmatch globs against the query string.

### Example

```
# Auto-approve all web searches
allow-web

# Or approve only specific topics
allow-web *framework*
allow-web *documentation*

# Prompt for sensitive searches
ask-web *password* "Review: searching for sensitive info"
ask-web *credential* "Credential-related search"

# Block suspicious searches
deny-web *exploit* "Blocked: suspicious search"

# Post-search feedback
after-web *api* "Verify the API version matches our project"
after-web *library* "Check if it's actively maintained"
```

### Opting In

To enable WebSearch rules, update your hook matcher in `settings.json`:

```json
"matcher": "Bash|WebSearch"
```

Or to enable both MCP and WebSearch:

```json
"matcher": "Bash|WebSearch|mcp__.*"
```

## Implementation Notes

**Hook caching:** Claude Code caches hooks at session start. Changes to dippy code or config require restarting the session to take effect.

**Two logging systems:** Dippy has two separate logs:
- `~/.claude/hook-approvals.log` - written by Python's `logging` module (can be disabled with `set log-hook-approvals off`)
- Audit log (configurable path) - written by `log_decision()`, requires `set log <path>`

**Log path:** The `~/.dippy/` directory may have write issues when running as a Claude Code hook. Using `~/.claude/dippy-audit.log` is more reliable.

**Log rotation:** Dippy automatically rotates audit logs daily. The current log is renamed to `audit-YYYY-MM-DD.log` (yesterday's date) on the first run after midnight. Old logs are automatically deleted after `log-rotate-max-days` days (default: 30). Set to `0` to disable rotation.

**Debugging config rules:** Check `~/.claude/hook-approvals.log` to see which rules matched. Entries show the pattern in parentheses when a config rule matches: `APPROVED: rm (rm /tmp/test-*)` vs just `APPROVED: rm` for built-in approval.

**System Python:** The hook runs with `#!/usr/bin/env python3` (system Python), not the uv virtualenv. System Python may be older and lack dependencies like `structlog`. Dippy must use only stdlib imports, or fail gracefully when optional dependencies are missing.

**VS Code syntax highlighting:** Install the extension from `editors/vscode/`:
```bash
cd editors/vscode
npx @vscode/vsce package
code --install-extension dippy-syntax-*.vsix
```
Highlights `.dippy` files and files named `config` (for `~/.dippy/config`).

## File Operation Rules

Claude Code hooks can match on `Read`, `Write`, `Edit`, and `MultiEdit` tools, not just `Bash`. This lets Dippy control file operations with per-project config.

### Syntax

```
allow-edit <glob>
ask-edit <glob>
ask-edit <glob> "message"
deny-edit <glob>
deny-edit <glob> "message"

allow-read <glob>
ask-read <glob>
ask-read <glob> "message"
deny-read <glob>
deny-read <glob> "message"
```

- `*-edit` applies to Write, Edit, and MultiEdit operations.
- `*-read` applies to Read operations.

Globs match file paths using `**` for recursive directory matching (same as redirect rules).

### Example

```
# Allow reading and editing source files
allow-read src/**
allow-edit src/**

# Prompt for config changes
ask-edit **/config.* "Config changes need review"

# Block sensitive files
deny-read **/.env* "Do not read secrets from env files"
deny-edit **/.env* "Use environment variables instead"
deny-edit **/secrets/** "Secrets are managed externally"
```

### Interaction with Built-in Permissions

Claude Code's `settings.json` has a `permissions` section with `allow`/`deny` rules. These systems layer:

1. **Built-in permissions** - global baseline, no per-project config, no messages
2. **Dippy** - per-project overrides with messages

If built-in permissions deny, Dippy never sees the request. If built-in allows, Dippy can still ask or deny.

### Opting In

To enable file operation rules, update your hook matcher in `settings.json`:

```json
"matcher": "Bash|Read|Write|Edit|MultiEdit"
```

**Trade-off:** This replaces Claude's "Allow reading/editing this session" UI. There's no way for hooks to defer to Claude's native session memory.
