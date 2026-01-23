<p align="center">
  <img src="images/dippy.gif" width="200">
</p>

<h1 align="center">🐤 Dippy</h1>
<p align="center"><em>Because <code>ls</code> shouldn't need approval</em></p>

---

<!-- FORK ENHANCEMENTS START -->
### 🍴 Fork Enhancements (vs [upstream](https://github.com/ldayton/Dippy))

- **File Edit Approval** — `allow-edit`, `ask-edit`, `deny-edit` rules for Write/Edit/MultiEdit tools
- **Include directive** — `include <path-or-glob>` for composable config files
- **Context-aware rules** — `[flags]` syntax with `@subshell`, `@compound`, negation (`!`)
- **Custom wrappers** — `wrapper <name>` for project-specific tools (ssh, docker exec, etc.)
- **Option rules** — `allow-opt`, `ask-opt`, `deny-opt` for subcommand/flag control
- **WebSearch support** — auto-approval for WebSearch tool *(by tony)*
- **Structured JSON output** — for PostToolUse hooks *(by tony)*
- **Bash test constructs** — support for `[ ]` and `[[ ]]` conditions
- **Log rotation** — `set log-rotate-max-days N` for automatic cleanup
- **Standard logging control** — `set log-standard off` to disable hook-approvals.log
- **Hybrid mode** — `set default pass` to let Claude decide unmatched commands
- **Audit log** — `cwd` field added for better context
- **82 more safe commands** — expanded allowlist from man page review
- **CLI mode** — standalone command validation with `--cmd`, `--stdin`, `--json`
<!-- FORK ENHANCEMENTS END -->

---

> **Stop the permission fatigue.** Claude Code asks for approval on every `ls`, `git status`, and `cat` - destroying your flow state. You check Slack, come back, and your assistant's just sitting there waiting.

Dippy is a shell command hook that auto-approves safe commands while still prompting for anything destructive. When it blocks, your custom deny messages can steer Claude back on track—no wasted turns. Get up to **40% faster development** without disabling permissions entirely.

Built on [Parable](https://github.com/ldayton/Parable), our own hand-written bash parser—no external dependencies, just pure Python. 9,500+ tests.

![Screenshot](images/screenshot.png)

## ✅ What gets approved

- **Complex pipelines**: `ps aux | grep python | awk '{print $2}' | head -10`
- **Chained reads**: `git status && git log --oneline -5 && git diff --stat`
- **Cloud inspection**: `aws ec2 describe-instances --filters "Name=tag:Environment,Values=prod"`
- **Container debugging**: `docker logs --tail 100 api-server 2>&1 | grep ERROR`
- **Safe redirects**: `grep -r "TODO" src/ 2>/dev/null`, `ls &>/dev/null`
- **Command substitution**: `ls $(pwd)`, `git diff foo-$(date).txt`

## 🚫 What gets blocked

- **Subshell injection**: `git $(echo rm) foo.txt`, `echo $(rm -rf /)`
- **Subtle file writes**: `curl https://example.com > script.sh`, `tee output.log`
- **Hidden mutations**: `git stash drop`, `npm unpublish`, `brew unlink`
- **Cloud danger**: `aws s3 rm s3://bucket --recursive`, `kubectl delete pod`
- **Destructive chains**: `rm -rf node_modules && npm install` (blocks the whole thing)

---

## Installation

```bash
git clone https://github.com/ldayton/Dippy.git
```

Add to `~/.claude/settings.json` (or use `/hooks` interactively):

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash|mcp__.*",
        "hooks": [{ "type": "command", "command": "/path/to/Dippy/bin/dippy-hook" }]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "Bash|mcp__.*",
        "hooks": [{ "type": "command", "command": "/path/to/Dippy/bin/dippy-hook" }]
      }
    ]
  }
}
```

---

## CLI Mode

Validate commands without running as a hook:

```bash
dippy --cmd 'rm -rf /'              # validate a command
dippy --cmd 'ls -la' --json         # JSON output
dippy --cmd 'git status' --cwd /path
echo 'ls -la' | dippy --stdin       # read command from stdin
```

**Exit codes:**
- `0` = allow (command is safe)
- `1` = deny (blocked by rule)
- `2` = ask (needs user approval)

**Options:**
- `--cmd COMMAND` — command to validate
- `--stdin` — read command from stdin (plain text)
- `--cwd PATH` — working directory (default: current)
- `--json` — output as JSON
- `--config PATH` — custom config file

**Use cases:**
- Scripting: `if dippy --cmd "$cmd"; then eval "$cmd"; fi`
- Batch validation: `cat commands.txt | while read cmd; do dippy --cmd "$cmd"; done`
- Integration with other AI tools

---

## File Edit Approval

Dippy can also auto-approve file edits (`Write`, `Edit`, `MultiEdit` tools) using the same config system. To enable:

```json
"matcher": "Bash|Write|Edit|MultiEdit"
```

Then use `allow-edit`, `ask-edit`, `deny-edit` rules in your config:

```
allow-edit src/**        # auto-approve source edits
ask-edit **/config.*     # prompt for config changes
deny-edit **/.env*       # block env file edits
```

See [File Operation Rules](docs/config.md#file-operation-rules) for details.

---

## Configuration

⚠️ Configuration is still evolving; syntax and behaviors may change.

Dippy reads config from (lowest to highest priority):

- `~/.dippy/config` (user global)
- `.dippy` in the project tree (walks up from cwd)
- `$DIPPY_CONFIG` (env override)

Sample config:

```
# Include external config files
include ~/.dippy/defaults/*            # include shared rules from home
include .dippy-local-*                 # include project-specific overrides (glob pattern)

set log ~/.dippy/audit.log             # write audit log to this path
set log-full                           # include full command in audit log
set log-rotate-max-days 30             # keep rotated logs for N days (0 = disable)
set log-standard off                   # disable hook-approvals.log (standard logging)

# Default behavior for commands with no matching rule
set default ask                        # prompt for approval (default)
# set default pass                    # don't intercept - let Claude decide
# set default allow                   # auto-approve everything without explicit rule

deny docker *                          # block all docker by default
allow docker run nginx:*               # allow nginx runs
deny docker run *--privileged*         # still ban privileged mode, last matching rule wins

deny python "Use uv run python, which runs in project environment"  # remind Claude to use uv

allow-redirect /tmp/**                 # allow temp file writes
deny-redirect **/.env* "Never write secrets, ask me to do it"       # block env writes

# Context-aware rules (flags: @subshell, @compound, ssh, sudo, use ! to negate)
deny [!@subshell] cd *                 # deny cd when NOT in subshell (equivalent below)
deny cd *                              # block standalone cd
allow [@subshell] cd *                 # but allow (cd /tmp && make)

# Custom wrappers (define project-specific tools with context flags)
wrapper wrap                           # define custom wrapper
allow [wrap,server1] free *            # allow free on server1 via wrap
deny [server1] rm *                    # deny rm on server1 (any wrapper)

# Option-specific rules
allow-opt git status fetch log diff     # allow these git subcommands
deny-opt "git commit" --no-verify       # block commits skipping hooks
ask-opt "git push" --force "Use --force-with-lease instead"  # prompt for force push

# Bash test constructs ([ ] and [[ ]])
allow [] [[] *                          # allow test commands: [ -f file ], [[ condition1 && condition2 ]]

# MCP tool rules
allow-mcp mcp__github__get_*           # allow read-only GitHub MCP tools
allow-mcp mcp__github__list_*
deny-mcp mcp__*__delete_* "No deletions"  # block destructive MCP operations

after git commit * "Reread prompts/next-iteration.md"  # after hook keeps Claude on task
```

### Include Directive

Split configuration across multiple files:

```
include <path-or-pattern>
```

- **Glob patterns**: `include .dippy-ok-*` includes all matching files
- **Home expansion**: `include ~/.dippy/shared-rules` expands `~`
- **Relative paths**: Resolved relative to the including file
- **Recursive**: Included files can include other files
- **Last-match-wins**: Later includes override earlier ones

Configuration reference: [docs/config.md](docs/config.md)

---

## Uninstall

Remove the hook entry from `~/.claude/settings.json`.
