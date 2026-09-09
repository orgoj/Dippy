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

### Direct execution

```bash
dippy run 'CMD'
dippy run-on-server SERVER 'CMD'
dippy run <<'DIPPY'
CMD
DIPPY
dippy run-on-server SERVER <<'DIPPY'
CMD
DIPPY
```

These commands load the normal user and project configuration, classify the
unchanged command string, and execute only an `allow` or an `ask` approved by
the configured askpass provider. There is no terminal fallback: missing,
broken, timed-out, or unexpected askpass responses deny execution.

Omitting `CMD` reads the script from stdin. For automatic outer-command
validation, use exactly one directly attached, non-empty quoted heredoc as
shown above. Quoting prevents the calling shell from expanding parameters or
command substitutions before Dippy can classify the script. Unquoted, piped,
file-backed or mixed-input forms remain on approval.

Approval is synchronous: the caller remains blocked while the dialog is open.
The effective wait is therefore limited by both `askpass-timeout` and any
timeout imposed by the calling agent CLI. The 59-second default is intended to
let Dippy return its own fail-closed error before a common 60-second caller
limit. A longer configured value works only when the caller also permits the
longer wait; if the caller terminates first, Dippy cannot return a final error.
The calling Dippy process is the sole owner of this deadline: it terminates the
askpass process when the configured timeout expires. The built-in GUI has no
independent timer, so it cannot race the caller and waits indefinitely when
launched on its own.

While waiting, Dippy writes a status line to its own stderr stating that the
requested command has not started. A redirect inside the quoted command does
not hide this status; redirecting the outer `dippy run` stderr does. On approval,
local execution inherits the command's stdout and stderr and returns its exit
code unchanged. On approval timeout, the command is not executed. Retry the
same Dippy request when it can be reviewed, or choose another safe solution;
never bypass Dippy because approval timed out.

Every blocked execution states that the requested command was not executed. A
denial includes the original command, the user's optional note or the matching
rule reason, and guidance to revise the request or choose another safe
solution.

Runtime messages deliberately use neutral terms such as `Approval denied` and
`Execution denied`. They do not name Dippy, askpass, providers, config files or
internal enforcement details because `dippy run` may sit behind a compatibility
wrapper. The agent needs the outcome, original command, execution status,
reason and safe next step; naming the hidden enforcement mechanism or warning
the agent not to bypass it would expose unnecessary implementation details and
could itself suggest an evasion path.

The initial wait status always reports that approval is required, gives the
timeout, and states that execution has not started. Its final instruction is
configured with `approval-wait-message`. The safe default is:

```text
Stop work and wait for the user unless you can continue safely without this command.
```

This default prevents an agent from treating the pending request as a command
failure and starting unrelated recovery or bypass attempts. A project with a
supervising agent or another user communication channel can replace the
instruction in its `.dippy`, for example:

```text
set approval-wait-message "Contact the supervising agent through the project channel to request authorization, then wait."
```

Only the instruction is configurable; the neutral status, timeout, and
not-started facts remain fixed so a project cannot accidentally hide them. An
empty message is rejected.

Remote targets must be safe SSH-config aliases declared explicitly:

```text
server build1
server database-readonly

set run-on-server-backend ssh       # ssh, tmux, or herdr
set run-on-server-session dippy     # managed tmux session / Herdr session
set run-on-server-timeout 300
set run-on-server-poll-interval 0.1
set askpass dippy-askpass-gui
set askpass-timeout 59
set approval-wait-message "Contact the supervising agent and wait."

allow [run-on-server] frob status
ask [run-on-server,build1] frob deploy
```

`ssh` preserves separate stdout and stderr. The tmux and Herdr backends return
their merged terminal capture and do not support interactive stdin. Each
operation creates a fresh local Bash transport pane and sends the command only
through SSH stdin. It does not reuse an interactive remote shell. Panes remain
available for inspection and recovery; close completed panes when no longer
needed. Changes of remote working directory or shell variables do not persist
between operations: include them in the approved command when needed.

A timeout, interruption, missing remote marker, or uncertain SSH disconnect is
recorded as `INDETERMINATE`; Dippy will not run another command on that server
within the same project. A leftover `running` record after a crash also blocks
execution. Changing the SSH profile, backend or session does not bypass this
guard. Projects are identified by their nearest `.dippy` or Git root, falling
back to the supplied working directory. Use the same project with `--cwd` for
execution and recovery.
Use `dippy recover SERVER` to check a persistent backend's completion marker,
or inspect the target manually and run `dippy recover SERVER --clear`.
If a persistent backend's start marker has already scrolled out but its
completion marker remains, Dippy returns all command output still present in
the capture buffer. Output discarded by tmux or Herdr itself cannot be
recovered.

Recovery checks the saved backend, session and SSH profile before capturing
the saved concrete pane. `--clear` acknowledges an uncertain result; it does
not cancel a remote command. Inspect the server first. A subsequent operation
creates a new pane, so it cannot send input into the old SSH process. Corrupt
state files are reported as errors, not discarded. Old alias-only pending
records require manual inspection and `recover SERVER --clear`; Dippy never
adopts an old interactive SSH pane. The legacy alias lock is retained for
compatibility, so calls to the same alias serialize even across projects.

### Project SSH profiles

Without an override, all backends use the user's normal SSH configuration,
agent and multiplexing settings. No SSH files, keys or agents are created or
changed by Dippy.

To select a project profile, put both settings in the project's `.dippy`:

```text
server production
set run-on-server-ssh-config .agent-ssh/config
set run-on-server-ssh-auth-sock /run/user/1000/project-a-agent.sock
```

The socket must belong to an already-running, separately provisioned SSH
agent. Dippy does not launch an agent or load keys. Alternatively, use
`set run-on-server-ssh-auth-sock none` to disable agent authentication and
select private key files in the SSH config. Missing config, missing/non-socket
agent path, validation errors and authentication failures stop execution;
there is no retry using the user's configuration, agent, default keys or
master connection. Batch mode disables password and passphrase prompts.
Malformed `run-on-server-ssh-*` directives are fatal configuration errors,
including unknown setting names; they cannot be silently skipped into user mode.

Both settings follow normal Dippy scope precedence, including `--config` and
`set final`. A partial profile is rejected at execution. Relative paths are
resolved against the Dippy file that declares them, including an `include`
file. Paths containing spaces can be quoted. Socket paths containing `%`, `$`,
double quotes, backslashes or line breaks are rejected because OpenSSH can
interpret those characters instead of selecting a literal socket.

An example `.agent-ssh/config`, with deployment-specific absolute paths:

```sshconfig
Host production
    HostName production.example.net
    User project-a-agent
    IdentityFile /secure/project-a/id_ed25519.pub

Host *
    IdentitiesOnly yes
    ForwardAgent no
    StrictHostKeyChecking yes
    UserKnownHostsFile /secure/project-a/known_hosts
    ControlMaster auto
```

The public `IdentityFile` selects its matching private key from the project
agent. With agent authentication disabled, point `IdentityFile` at a private
key usable without an interactive passphrase prompt. Provision the known-host
entry separately. Keep private keys out of version control.

For an explicit profile Dippy supplies `-F`, `IdentityFile=none` as a baseline,
the selected `IdentityAgent`, `IdentitiesOnly=yes`, public-key-only batch
authentication, `ForwardAgent=no`, `PKCS11Provider=none` and
`PermitLocalCommand=no`. It validates effective options with `ssh -G` for the
actual destination and requires at least one explicitly selected identity.
The matching `SSH_AUTH_SOCK` is set (or removed for `none`) in every backend,
including inside terminal panes; a multiplexer daemon's old agent environment
cannot override it.

The profile's `ControlMaster` mode is honored, but Dippy replaces `ControlPath`
with a fresh private socket for each operation and forces `ControlPersist=no`.
The directory is owned by the current user with mode `0700`. This prevents
reuse of either the user's master or authentication cached before a profile
or agent changed. Connection caching **between operations** is intentionally
unavailable in profile mode. The default user mode retains normal caching.

SSH configuration is trusted executable configuration: `Match exec`, includes
and `ProxyCommand` may run local programs, even during `ssh -G`. Dippy does not
sandbox these programs. Automatic `ProxyJump` is rejected in profile mode
because destination options do not isolate jump-host authentication. If a
trusted administrator uses `ProxyCommand`, that command must explicitly
isolate its own SSH configuration, identity, agent and control socket too.
Native SSH relative `Include` paths follow OpenSSH's rules, not the containing
file's directory; use absolute paths for SSH includes and identity files.

To restore user mode over an inherited profile, reset both settings:

```text
set run-on-server-ssh-config none
set run-on-server-ssh-auth-sock none
```

Removing settings with `dippy config unset --project KEY` exposes any inherited
values; it is not an explicit reset. The keys also work with `dippy config
get/set --project`. This feature isolates connections made through Dippy.
Preventing an agent running as the same OS user from independently accessing
other keys or sockets requires an OS sandbox or a separate OS identity.

Configuration can be changed without rewriting rules or comments:

```bash
dippy config get --user
dippy config get --user run-on-server-backend
dippy config set --user run-on-server-backend herdr
dippy config set --project approval-wait-message \
  "Contact the supervising agent through the project channel and wait."
dippy config unset --user askpass
dippy config server add --project build1
dippy config server remove --project build1
dippy config server list --project
```

The default scope is `--user`; `--project` edits `.dippy` in the current
directory. Setting administration writes canonical hyphenated keys and updates
an existing underscore spelling in place instead of creating a duplicate.
Global `--config` and `--cwd` options apply to `run`, `run-on-server`, and
`recover` as well as validation mode.

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
| `--agent NAME` | Force agent name in audit log |
| `--remote` | Skip local path checks (for containers/SSH) |
| `--version` | Show Dippy version |

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
3. `$DIPPY_CONFIG` - env override
4. Final config via `set final <path>` (if configured, loaded last)

Project config is found by walking up from cwd to filesystem root, stopping at the first `.dippy` found (like `.git` discovery).

**Final config:** Use `set final ~/.dippy/emergency` in your user config for emergency overrides. The final file is only loaded when it exists - create it when needed (e.g., `deny *` during emergencies), delete when done.

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
include .dippy-user-*

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
| `$VAR=value` | Environment variable is set to that value | `[$HCOM_INSTANCE_NAME=bot1]` |

Note: context flags are supported by `allow`, `ask` and `deny` only. The
`-redirect`, `-edit`, `-read`, `-mcp`, `-web` and `-opt` variants ignore them.

### Environment Flags

Environment variables become context flags only after being declared with
`set context-env` (see [Settings](#settings)). Each declared variable that is
set and non-empty produces the flag `$NAME=value`:

```
set context-env HCOM_INSTANCE_NAME

allow [$HCOM_INSTANCE_NAME=bot1] deploy-tool *
deny  [$HCOM_INSTANCE_NAME=bot2] deploy-tool * "bot2 must not deploy"
```

This lets one config serve several agents that share a working directory,
without giving each of them a separate `$DIPPY_CONFIG` file.

Environment flags are added at the start of the analysis, so they combine with
every other flag - including wrapper flags for nested commands:

```
wrapper remote-run --cmd run --context -t

# only bot1, only on host1, only this command
allow [$HCOM_INSTANCE_NAME=bot1,remote-run,host1] systemctl status *
```

An unset or empty variable produces no flag at all, so an `allow` rule guarded
by it never matches (fail-closed).

### Custom Wrappers

Define your own wrapper commands with the `wrapper` directive:

```
wrapper <command_name> [--cmd <trigger>] [--flag <target_flag>] [--context <flag>] [--context-first] [--script-stdin <marker>]
```

| Option | Meaning |
|--------|---------|
| `--cmd <trigger>` | Subcommand after which the inner command starts (e.g. `run`, `exec`) |
| `--flag <target_flag>` | Flag whose value is the destination (e.g. `-t`) |
| `--context <flag>` | Flag whose value is added to the context flags |
| `--context-first` | First positional arg is the destination and becomes a context flag |
| `--script-stdin <marker>` | At the marker, analyze one directly attached quoted heredoc as the remote shell script |

**Example:**

```
# Everything after the wrapper name is the inner command
wrapper rtk

# Inner command starts after 'run'
wrapper tokf --cmd run

# 'docker -t NAME exec CMD' - target after -t, inner command after 'exec'
wrapper docker --cmd exec --flag -t

# 'cca-tmux-cli -t SESSION run CMD' - SESSION becomes a context flag
wrapper cca-tmux-cli --cmd run --context -t --script-stdin --script

# 'ssh SERVER CMD' - first positional arg becomes a context flag
wrapper ssh --context-first
```

The legacy positional form (`wrapper NAME trigger -flag`) is still parsed for
backwards compatibility and implies `--context-first`.

**Usage:**

```bash
# Basic
wrap server1 free -h          # wrapper=wrap, dest=server1, cmd=free -h

# With trigger
cca-tmux-cli l2 run "ls"      # wrapper=cca-tmux-cli, dest=l2, cmd=ls

# With trigger and target flag
cca-tmux-cli -t l2 run "ls"   # wrapper=cca-tmux-cli, dest=l2, cmd=ls

# Literal multiline script: no local expansion or indirect input
cca-tmux-cli -t l2 run --script <<'REMOTE'
free -h
uname -a
REMOTE
```

**Rules:**

```
# Allow free on server1 via wrap
allow [wrap,server1] free *

# Deny rm on server1 (any wrapper)
deny [server1] rm *

# Allow specific command via cca-tmux-cli
allow [cca-tmux-cli,l2] ls *
```

**How it works:**

1. **Trigger search**: If `subcommand_trigger` is defined, Dippy looks for this word and treats everything after it as the `inner_command`.
2. **Target extraction**: 
   - If `target_flag` is defined, Dippy looks for it *before* the trigger and takes the next token as the destination.
   - If no flag is defined or found, it takes the first non-option token *before* the trigger.
3. **Context flags**: Sets both the wrapper name (`cca-tmux-cli`) and destination (`l2`) as flags.
4. **Recursive analysis**: Analyzes the `inner_command` recursively.
5. **Remote mode**: Inner commands are automatically analyzed with `remote=True`, which skips local path checks (ideal for SSH/containers).

With `--script-stdin`, the marker must be the first and last word after the
wrapper trigger, and the command must have exactly one non-empty quoted heredoc
redirect. Dippy analyzes that literal body recursively in remote mode. Unquoted
heredocs, pipelines, files, variables, inline arguments and additional redirects
do not enter script mode and therefore remain on `ask`. This strict form keeps
local shell expansion and indirect input out of auto-approved multiline scripts.

**Wrapper flags only exist once the trigger is found.** `cca-tmux-cli -t host read`
(no `run`) is analyzed as a plain command, so `[cca-tmux-cli]` rules do not apply
to it - subcommands of the wrapper itself need ordinary positional rules.

To cut one agent off from a wrapper entirely, both halves of that split matter.
A `[wrapper]` rule reaches only what follows the trigger, so it leaves the
wrapper's own subcommands open. A positional rule on the wrapper name matches
the invocation itself and therefore covers every subcommand, the trigger
included:

```
deny [$AGENT=mail,cca-tmux-cli] *   # only `run "..."` - list and read stay open
deny [$AGENT=mail] cca-tmux-cli *   # the whole tool, run included
```

**A wrapper is recognised by its bare name.** `/usr/local/bin/wrap host free -h`
is not unwrapped, so neither the wrapper flags nor a `wrap *` rule apply to it -
it falls through to whatever matches the full path, usually `ask`.

**sudo inside a wrapper must be spelled out.** With a reset rule like
`ask [wrap] *`, a rule `allow [wrap,sudo] journalctl *` never fires - the reset
matches the `sudo` token before delegation reaches the inner command. Write
`allow [wrap] sudo journalctl *` instead.

Custom wrappers merge via set union across config scopes (user + project).
Later definitions of the same wrapper name override earlier ones.

**Built-in wrappers vs custom:**

Built-in wrappers (`ssh`, `sudo`) work the same way but are predefined. SSH
adds both `ssh` and the exact target token as context flags, so
`allow [ssh,ferda7] tail *` does not apply to another host. Custom wrappers let
you define project-specific tools with the same context-aware control.

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

**Patterns without globs match as a prefix.** `allow git status` also matches
`git status --short` - and anything else that follows. Convenient for simple
commands, dangerous when the extra arguments change what actually runs:

```
allow cca-tmux-cli list        # also allows: cca-tmux-cli list run "rm -rf /"
```

**Exact anchor `|`** disables prefix matching - the pattern must match the whole
command:

```
allow cca-tmux-cli list|       # only the bare subcommand
```

**A pattern token containing `/` is resolved against cwd - globs included.**
This is what makes `allow node bin/*` expand to `allow node /cwd/bin/*`, but it
also means a leading `*/` never matches an arbitrary absolute path: `*/tool`
becomes `/cwd/*/tool`. Without a `/` the token is left alone, and `*` then
crosses path separators normally:

```
deny *my-tool *     # matches /opt/x/my-tool arg
deny */my-tool *    # does NOT match it - reads as /cwd/*/my-tool
```

A rule that silently never matches is the dangerous half of this: check a new
`deny` against the command it is meant to stop before trusting it.

**`/**/tool` is the form that matches every invocation.** A leading `/` anchors
the pattern at the filesystem root instead of cwd, and `**` then covers both a
relative and an absolute path:

```
allow /**/hcom list *   # matches ./target/debug/hcom, /home/u/.cargo/bin/hcom
allow hcom list *       # matches the PATH binary - a separate rule
```

**`*` and `**` skip whole tokens, including subcommands.** Because `*` matches
spaces, a glob in the middle of a pattern is a bypass surface: a rule meant to
allow one read-only subcommand also allows a destructive one that happens to be
followed by the right words.

```
allow hcom * agent show *    # also allows: hcom kill boom agent show x
```

Spell out the intervening tokens instead - one rule per option form is verbose
but cannot be walked through.

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

Some of those handlers make rules unnecessary. `--help` and `-h` are approved
whenever they are the last token of a command of at most four tokens, so
`tool sub sub2 --help` needs no rule but the longer `tool --name X sub --help`
does. `help`, `version` and `--version` are approved only as the single
argument. `-c` or `-m` anywhere in the command disables all of this - what
follows them is script input, not a flag.

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

`delegate` is an opt-in escape from an outer command rule into a built-in CLI
handler. It is useful when a global reset keeps a wrapper on `ask`, while one
project or agent should classify the wrapped command instead:

```
ask ssh *
delegate [$HCOM_INSTANCE_NAME=log_reader] ssh *
allow [ssh] tail *
```

Only the matching invocation is delegated. A later `deny` still wins, and the
inner command is analyzed normally, including every part of a compound command.
Use `delegate` only for commands with a built-in handler; an unknown delegated
command still falls through to `ask`.

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

### What redirect rules cover

All Bash redirects that can open a file for writing are covered, including
`>`, `>>`, `>|`, `<>`, numbered descriptors such as `3>`, and variable
descriptors such as `{fd}>`. Descriptor duplication such as `2>&1` remains
safe. Redirect rules also govern commands that write files through their
arguments: `tee`, `find -fprint`, `sort -o`, and `cp`/`mv`.

For `cp` and `mv` the destination is checked. `mv` also removes its sources, so
those paths are checked too — a `deny-redirect **/.dippy` stops
`mv .dippy /tmp/saved`, not just a write into `.dippy`.

A destination directory is expanded to the paths actually written
(`cp -t /tmp a b` → `/tmp/a`, `/tmp/b`). Dippy never stats the filesystem, so a
bare `cp a /tmp` is taken at face value and asks; write `cp a /tmp/` when you
mean the directory.

Without a matching rule these commands ask, which is what they did before.

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

**Option matching does not stop at `--`.** Every token after the prefix is
considered, including arguments following the option delimiter. For example:

```
allow-opt "hcom send" --reply-to
```

also matches:

```bash
hcom send @agent -- '--reply-to'
```

Do not use an option rule as strict proof that an item was parsed as a CLI
option unless this behavior is acceptable.

**Items match whole tokens, not words inside a quoted argument.** A rule meant to
catch a keyword in a query or a script body silently matches nothing:

```
deny-opt duckdb COPY    # does NOT match: duckdb -readonly db.db "COPY (SELECT 1) TO '/x'"
```

The query is a single token once the parser strips the quotes, and an item is
compared against whole tokens. To reach inside it, use a normal rule with `**`
and a glob; character classes are how you get case-insensitivity:

```
ask duckdb ** *[Cc][Oo][Pp][Yy]*
```

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

# Instruction appended while direct execution waits for approval
set approval-wait-message "Stop work and wait for the user."

# Final config (loaded after all other configs)
set final ~/.dippy/emergency  # emergency overrides (loaded last)

# GUI approval (SSH_ASKPASS style)
set askpass /path/to/askpass  # external approval program (e.g., dippy-askpass-gui)
set askpass-timeout 59        # seconds to wait (default: 59)

# Environment variables exposed as context flags (repeatable)
set context-env HCOM_INSTANCE_NAME
set context-env CI

# Deny message formatting (for AI agents)
set deny-format "Custom template: {command} -> {reason}"
set deny-format-pi "PI-specific format: {reason}"
set deny-format-claude "Claude-specific format: {reason}"
```

### GUI Approval Provider (`dippy-askpass-gui`)

Dippy includes a standalone graphical approval dialog (`dippy-askpass-gui`) built with Tkinter. It is designed for fast, frictionless decision-making:

- **Operation Classification**: Automatically distinguishes and highlights the operation type:
  - **Read File** (`view_file`, `Read`, `read_file`): displays exact target file path.
  - **Edit File** (`write_to_file`, `replace_file_content`, `Write`, `Edit`): displays exact target file path.
  - **Run Command** (`Bash`, `run_command`): displays formatted shell command.
  - **Web Request** & **MCP Tool Call**: displays target URL/query or MCP tool name.
- **Multi-Monitor Centering**: Queries `xrandr --listmonitors` to detect active monitors, determines which monitor contains the mouse pointer, and positions the dialog in the exact geometric center of that active monitor.
- **Fast Keyboard Navigation**:
  - `y` / `Y` / `Enter` / `Return` — **Allow**
  - `n` / `N` / `Esc` / `Escape` — **Deny**
  - Focus defaults to the Allow button for instant one-touch operation.

### Antigravity CLI (AGY) Integration & Pure Control

Antigravity CLI (`agy`) integrates via lifecycle hooks defined in `~/.gemini/config/hooks.json` or `.agents/hooks.json`.

#### Pure Control Model
In YOLO / bypass mode (`--dangerously-skip-permissions`), AGY automatically ignores and skips tool `ask` prompts. To maintain security:
- Dippy enforces a **Pure Control** model for AGY, returning strictly binary `{"decision": "allow"}` or `{"decision": "deny"}`.
- When an operation matches an `ask` rule or falls through unclassified (`set default ask`), Dippy resolves the decision interactively through `dippy-askpass-gui`.
- If approved, Dippy returns `allow`. If denied, dismissed, or timed out, Dippy returns `deny` to hard-block execution in AGY.

#### Multi-Workspace Session Resolution
In multi-workspace sessions (`workspacePaths`), AGY provides multiple root directories. Dippy matches target file paths against `workspacePaths` to locate the containing workspace and load the appropriate project `.dippy` configuration rules.

#### Recommended Global Rules for Antigravity
Antigravity stores scratchpads, task plans, and conversation transcripts in `~/.gemini/antigravity-cli/brain/**`. To allow AGY to manage its workspace without approval prompts, configure global file rules in `~/.dippy/config`:

```
# Antigravity (AGY) artifact and brain directory
allow-read ~/.gemini/antigravity-cli/brain/**
allow-edit ~/.gemini/antigravity-cli/brain/**
```

Settings use kebab-case or snake_case interchangeably.

### Python Module Directives

When `python -c 'code'` or a Python script is analyzed for safety, Dippy checks
imports against built-in safe and dangerous module lists. These directives let
you customize those lists.

They are **standalone directives, not settings** — `set python-allow-module` is
an unknown setting and gets skipped with a warning.

```
python-allow-module <module>    # consider module safe
python-deny-module <module>     # consider module dangerous
python-allow-symbol <module>.<symbol>   # allow one name from a module
```

- One module per directive; repeat to add multiple modules.
- Dotted names are supported: `python-allow-module numpy.linalg`
- `python-allow-module` takes precedence over `python-deny-module` if both match.
- Allow overrides the built-in dangerous list; deny overrides the built-in safe list.
- Lists accumulate across configs: a project `.dippy` extends the global list
  instead of replacing it.

**Example — allow data science tools:**
```
python-allow-module numpy
python-allow-module pandas
python-allow-module scipy
```

**Example — block specific modules:**
```
python-deny-module requests
python-deny-module http.client
```

#### `python-allow-symbol`

Allows exactly one name from a module that would otherwise be rejected — useful
when the module as a whole is too broad to trust:

```
python-allow-symbol sys.stdin
```

That approves `from sys import stdin` (aliases included:
`from sys import stdin as s`), while everything else about `sys` stays as it
was. Specifically, these still need approval:

- `import sys` — a plain module import, not a symbol import
- `from sys import exit` — a name that was not allowed
- `from sys import stdin, argv` — every name in the import must be allowed
- `from sys import *` — wildcards are never allowed
- `from .sys import stdin` — a relative import reads a local file, not the stdlib

A `python-deny-module` you wrote yourself beats the symbol allowance, even
across merged configs; the built-in dangerous list does not.

There is no `python-deny-symbol`: you can already deny a whole module, and for
a mixed module you allow the few symbols you need and the rest falls through to
`ask`.

> This is a trust decision, not a sandbox. Importing a symbol runs the module's
> top-level code, and what you do with the imported object may have side
> effects static analysis cannot see. The other AST safety checks still apply
> after the import is accepted.

### Safe Data Processing (JSON and YAML)

Coding agents frequently default to inline Python (`python3 -c "import json..."`) or writing ad-hoc temporary scripts (`tmp/*.py` or agent scratchpads) to filter, inspect, or join structured data like JSON and YAML.

#### Why Blanket Python Allow Rules Are Problematic

Attempting to resolve agent approval fatigue by allowing arbitrary Python:
- `allow python -c *` opens an arbitrary code execution hole (an agent can run `os.system`, delete files, or exfiltrate secrets).
- `allow python tmp/**.py` or allowing scratchpad directories is similarly unsafe: an agent can write arbitrary code to that path and execute it unprompted.
- Dippy's Python AST analyzer intentionally blocks file I/O (`open()`, `pathlib`) because static analysis cannot safely prove that file reads won't leak sensitive files or be paired with side effects.

#### The Recommended Solution: `yq`

Instead of relaxing Python security rules or relying on complex, non-portable OS sandboxing, the recommended solution is directing agents to use **`yq`** ([mikefarah/yq](https://github.com/mikefarah/yq)):

1. **Native Dippy auto-approval:** Dippy includes a built-in handler (`cli/yq.py`) that automatically classifies `yq` as `allow`. Only in-place file mutation flags (`-i`, `--inplace`) require confirmation.
2. **Multiplatform single binary:** `yq` is a standalone Go executable with zero runtime dependencies, working identically across Linux, macOS, and Windows.
3. **Multi-format support:** Natively processes both **JSON and YAML** (as well as XML, CSV, and TOML) and seamlessly converts between them (`-o=json`, `-o=yaml`).
4. **No code execution risk:** Unlike Python interpreters, `yq` is a dedicated query and transformation tool with no shell escape hatches.

#### Guiding Agents in Instructions

To prevent agents from attempting Python one-liners in the first place, add a strict rule to your agent instructions (`CLAUDE.md`, `AGENTS.md`, or system prompt):

```markdown
JSON/YAML: strictly `yq` (never `jq`, never `python3 -c "import json"`, never python scripts for data inspection/conversion). Keys: `yq 'keys'`, nested: `yq '.env | keys'`, extract: `yq '.foo.bar'`, filter: `yq '.items[] | select(.active)'`.
```

### Deny Format

When a command is denied, Dippy can format the rejection message to be clearer for AI agents. This helps agents understand they should follow the instruction rather than trying alternative commands.

**Placeholders:**
- `{command}` - The original blocked command
- `{reason}` - The deny message from the matching rule
- `{pattern}` - The pattern that matched (extracted from reason if not available)

**Default formats:**
- General: `⚠️ DENIED by security policy.\n\nCommand: {command}\n\n{reason}`
- `pi`: Includes explicit instruction to not try alternatives
- `claude`: Similar to pi format

**Example configuration:**
```
# Clear format for pi agent
set deny-format-pi "⛔ DENIED: {command}\n\n📋 INSTRUCTION: {reason}\n\nDo NOT try alternatives. Follow the instruction exactly."

# Simpler format for Claude
set deny-format-claude "🚫 Blocked: {command}\n\n→ {reason}"

# Fallback for other agents
set deny-format "Command denied: {reason}"
```

This is especially useful for rules that suggest alternatives:
```
deny find * "For file and string recursive search use only `rg` cli command."
```

With the default pi format, the agent sees:
```
⛔ DENIED: find . -name test

📋 INSTRUCTION: find: For file and string recursive search use only `rg` cli command.

Do NOT try alternatives. Follow the instruction exactly.
```

### Default Behavior

The `set default` directive controls what happens when a command doesn't match any explicit rule:

| Value | Behavior | Use Case |
|-------|----------|----------|
| `ask` | Prompt user for approval | Safest - explicit approval for unknown commands |
| `pass` | Return empty response; Claude handles it | Hybrid - Dippy only handles explicitly configured rules |
| `allow` | Auto-approve | YOLO mode - trust everything not explicitly blocked |

## Askpass (GUI Approval)

When Dippy runs in headless environments (tmux, screen, background processes), the built-in terminal prompts won't be visible. The askpass feature lets you delegate approval to an external GUI program.

### Configuration

```
# In ~/.dippy/config
set askpass /usr/bin/zenity-dippy    # path to external program
set askpass-timeout 59               # seconds to wait (default: 59)
set approval-wait-message "Stop work and wait for the user."
```

Or via environment variable (overrides config):
```bash
export DIPPY_ASKPASS=/usr/bin/zenity-dippy
```

### How It Works

When a rule returns `ask` and askpass is configured:

1. Dippy calls the askpass program
2. GUI shown to user (via zenity, kdialog, rofi, etc.)
3. User approves or denies
4. Dippy returns `allow` or `deny` to Claude Code

Without askpass, `ask` would show Claude Code's built-in terminal dialog (invisible in headless environments).

### Askpass Program Contract

The askpass program receives context via environment variables:

| Variable | Content |
|----------|---------|
| `DIPPY_COMMAND` | The command being approved |
| `DIPPY_CWD` | Current working directory |
| `DIPPY_RULE` | The rule pattern that matched (if any) |
| `DIPPY_MESSAGE` | The rule message (if any) |
| `DIPPY_TOOL` | Tool name (Bash, Write, Edit, etc.) |

Additionally, JSON with full details is passed via stdin:
```json
{
  "command": "git push origin main",
  "cwd": "/home/user/project",
  "rule": "ask git push *",
  "message": "Pushing to remote",
  "tool": "Bash",
  "source": "/home/user/.dippy/config"
}
```

**Exit codes:**
- `0` = approve → Dippy returns `allow`
- `1` = deny → Dippy returns `deny`
- `2+` = fallback → Dippy returns `ask` (falls back to Claude dialog)

Timeout or execution errors also fall back to `ask`.

### Example Askpass Scripts

**zenity (GTK):**
```bash
#!/bin/bash
zenity --question \
  --title="Dippy: Command Approval" \
  --text="Approve command?\n\n$DIPPY_COMMAND\n\nIn: $DIPPY_CWD" \
  --ok-label="Allow" \
  --cancel-label="Deny"
```

**kdialog (KDE):**
```bash
#!/bin/bash
kdialog --yesno "Approve command?\n\n$DIPPY_COMMAND" \
  --title "Dippy" --yes-label "Allow" --no-label "Deny"
```

**rofi (tiling WM):**
```bash
#!/bin/bash
echo -e "Allow\nDeny" | rofi -dmenu -p "Dippy: $DIPPY_COMMAND" | grep -q "Allow"
```

**Python (cross-platform):**
```python
#!/usr/bin/env python3
import os
import tkinter as tk
from tkinter import messagebox

root = tk.Tk()
root.withdraw()

cmd = os.environ.get("DIPPY_COMMAND", "unknown command")
cwd = os.environ.get("DIPPY_CWD", "")

result = messagebox.askyesno(
    "Dippy Approval",
    f"Allow command?\n\n{cmd}\n\nIn: {cwd}"
)

exit(0 if result else 1)
```

### Security Considerations

- **Trust your askpass program** - it can approve any command
- **Use absolute paths** - `set askpass /usr/local/bin/my-askpass`
- **Timeout protection** - long-running or stuck programs fall back to `ask`

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

## Idle Prompt Notifications

When Claude Code is waiting for input (idle state), Dippy can trigger notifications to alert you. This is useful for:
- Long-running tasks that complete while you're away from your desk
- Background processes that need your attention
- Remote monitoring when working from another machine

### Configuration

```bash
# Set the notification command template
# Placeholders are expanded from hook data and executed via shell
set idle-notifier-command notify-send "{title}" "{message}"
```

### Template Placeholders

| Placeholder | Description | Example |
|------------|-------------|---------|
| `{title}` | Notification title from hook | "Claude Code" |
| `{message}` | Message from hook | "Claude is waiting for your input" |
| `{cwd}` | Current directory | "/home/user/project" |
| `{notification_type}` | Notification type | "idle_prompt" |

### Examples

**Simple desktop notification:**
```bash
set idle-notifier-command notify-send "Claude" "Waiting for input"
```

**With placeholders:**
```bash
set idle-notifier-command notify-send "{title}" "{message}"
```

**Custom script with smart routing (desktop vs remote):**
```bash
set idle-notifier-command ~/.dippy/notify-idle.sh "{title}" "{message}" "{cwd}"
```

With `~/.dippy/notify-idle.sh`:
```bash
#!/bin/bash
TITLE="$1"
MESSAGE="$2"
CWD="$3"

# Desktop notification (always shown)
if command -v notify-send &> /dev/null; then
    notify-send "$TITLE" "$MESSAGE" -u normal -i terminal &
fi

# Remote notification: only if NOT at desktop
# (create ~/.atdesktop when at your computer, remove when away)
if [ ! -f "$HOME/.atdesktop" ]; then
    if command -v ntf &> /dev/null; then
        ntf send "$TITLE: $MESSAGE (in $CWD)" &
    fi
fi
```

### Opting In

To enable idle prompt notifications, add `Notification` to your hook matcher in `settings.json`:

```json
"matcher": "Bash|Notification"
```

## Implementation Notes

**Hook caching:** Claude Code caches hooks at session start. Changes to dippy code or config require restarting the session to take effect.

**Two logging systems:** Dippy has two separate logs:
- `~/.claude/hook-approvals.log` - written by Python's `logging` module (can be disabled with `set log-hook-approvals off`)
- Audit log (configurable path) - written by `log_decision()`, requires `set log <path>`

**Log path:** The `~/.dippy/` directory may have write issues when running as a Claude Code hook. Using `~/.claude/dippy-audit.log` is more reliable.

**Log rotation:** Dippy automatically rotates audit logs daily. The current log is renamed to `audit-YYYY-MM-DD.log` (yesterday's date) on the first run after midnight. Old logs are automatically deleted after `log-rotate-max-days` days (default: 30). Set to `0` to disable rotation.

**Debugging config rules:** Check `~/.claude/hook-approvals.log` to see which rules matched. Entries show the pattern in parentheses when a config rule matches: `APPROVED: rm (rm /tmp/test-*)` vs just `APPROVED: rm` for built-in approval.

File, MCP and web rules name their origin in the decision reason - `[.dippy* @ /home/u/proj/.dippy]` - which tells you whether a user rule or a project rule won. The named file is the one that was loaded, so a rule reached through `include` is reported under the file that includes it.

**Suggestion field:** When `set log_full` is enabled, ask decisions include a `suggestion` field in the audit log. This shows the env-stripped command (without `VAR=val` prefixes) that you can copy directly as an `allow` rule. Only command-matching asks have suggestions; redirect and substitution asks do not.

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
