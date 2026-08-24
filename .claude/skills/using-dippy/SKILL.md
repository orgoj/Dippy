---
name: using-dippy
description: "Configure and troubleshoot Dippy approval rules and coding-agent hook integrations. Use whenever changing Dippy config, diagnosing an unexpected allow/ask/deny, or wiring Dippy to Claude, Codex, Gemini, Cursor, Windsurf, or an external command wrapper. This skill is project-local to the Dippy repository; do not install it globally."
---

# Using Dippy

Treat an overly broad `allow` as a security bug. Read the active user, project,
environment, and final configs in load order because the last matching rule
wins. Change live configuration only when the user explicitly requests it, and
re-read a live config immediately before editing it.

Test candidate command rules with `dippy --cmd` or the repository's
`try-rules.sh` debug helper. Classify destructive bypass examples as strings;
never execute them. Use the `check-path.py` debug helper for path decisions.

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
