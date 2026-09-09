---
name: using-dippy
description: "Configure and troubleshoot Dippy approval rules and coding-agent hook integrations. Use whenever changing Dippy config, diagnosing an unexpected allow/ask/deny, or wiring Dippy to Claude, Codex, Gemini, Cursor, Windsurf, or an external command wrapper. This skill is project-local to the Dippy repository; do not install it globally."
---

# Using Dippy

Treat an overly broad `allow` as a security bug. Read the active user, project,
environment, and final configs in load order because the last matching rule
wins. Change live configuration only when the user explicitly requests it, and
re-read a live config immediately before editing it.

Never hardcode user home paths (`/home/<user>/`) in `.dippy` rules; use `~` or
relative paths so configurations remain portable across machines. Keep
project-specific agent workflows (such as reading `.dippy` rules during rule
development) in that project's `.dippy` file rather than cluttering global
`~/.dippy/config` with per-agent environment conditions.

A pattern token containing `/` resolves against cwd, so `**/tool` never matches
an absolute path and `/**/tool` matches every location, a planted binary
included. Read Command Patterns in `docs/config.md` before writing a
path-qualified rule and name the real locations instead: the PATH name and the
install target (`~/go/bin/tool`). Never allow running a program from a directory
the agent may write, such as a project `tmp/`; a tool downloaded there is the
agent's mistake to fix, not a rule to add.

For an audit entry, use its exact command and `cwd`. Read that project's
instructions and inspect each config scope separately before choosing a code or
config fix; an explicit project rule can override a handler. Replay the command
against the current installation before editing and the candidate source before
installing.

Test candidate command rules with `dippy --cmd` or the repository's
`try-rules.sh` debug helper. `dippy --cmd` performs static rule evaluation
without executing the underlying command; keep `dippy --cmd *` and
`dippy --config * --cmd *` allowed in developer configurations to prevent
approval prompt storms during rule verification. Classify destructive bypass
examples as strings; never execute them. Use the `check-path.py` debug helper for
path decisions.
Pass `--config` a persistent file, not process substitution such as `/dev/fd/*`,
and use the project's documented Python runner for source checks.

For multiline approved execution, use one directly attached quoted heredoc:

```bash
dippy run <<'DIPPY'
CMD
DIPPY
dippy run-on-server SERVER <<'DIPPY'
CMD
DIPPY
```

Keep the delimiter quoted so the calling shell cannot expand parameters or
command substitutions before classification. Do not pipe or redirect a script
file into these subcommands. Keep `dippy run 'CMD'` for a single-line command.

## Codex `ask` Requires a Prompt Bridge

Codex `PreToolUse` cannot force an approval prompt. It parses an `ask` response
but does not enforce it, so the command continues unless Codex independently
requires approval. `PermissionRequest` only runs when Codex's own policy has
already decided to prompt.

For an external wrapper that can change state outside the local sandbox, add a
Codex execpolicy prompt rule:

```python
prefix_rule(
    pattern = ["remote-wrapper"],
    decision = "prompt",
    justification = "Let Dippy decide whether this external command requires user approval.",
)
```

Use `~/.codex/rules/*.rules` for every project or
`<project>/.codex/rules/*.rules` for one trusted project. Restart Codex after
changing rules. Verify the prefix without executing the wrapped command:

```bash
codex execpolicy check --pretty \
  --rules ~/.codex/rules/dippy.rules \
  -- remote-wrapper target run "command"
```

The prefix covers every later argument, subcommand, and target. Do not enumerate
servers. Codex requires a non-empty literal prefix and has no catch-all
execpolicy pattern, so add one prompt rule per external wrapper that needs
enforced Dippy `ask` semantics.

With the bridge, Codex emits `PermissionRequest`: Dippy auto-approves `allow`,
leaves `ask` undecided so Codex displays its native approval UI, and blocks
`deny`. An observational hook that exits successfully without returning a
decision cannot approve the request by itself.

For the full integration details, read
[`docs/hook-systems/codex-cli.md`](../../../docs/hook-systems/codex-cli.md).
