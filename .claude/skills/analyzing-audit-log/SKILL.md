---
name: analyzing-audit-log
description: "Analyze Dippy user audit logs and propose narrowly scoped rules that remove repetitive approval prompts. Use when the user asks to inspect `~/.dippy/audit*.log`, find annoying approvals, or recommend safe Dippy allow rules. Do not use for general application log debugging."
---

# Analyzing the Dippy Audit Log

Load the `using-dippy` skill first; it governs how rules are written and
verified. This file only covers reading the log and choosing what to propose.

Group `ask` entries by workflow, reason, working directory, and frequency. Command
frequency alone is not enough: identify what repeated task the user is trying to
complete, such as creating, populating, inspecting, and cleaning a project-local
scratch directory.

Query the log only with `dippy audit`; it is auto-approved and reads the
rotations too. Never build `cat | grep | yq` pipelines over the log files: each
one is a manual approval prompt for the user.

```bash
dippy audit --since 2026-10-01 --not-allow --policy-cwd ~/wiki
dippy audit --agent agy --not-allow --group-by cwd
dippy audit --not-allow --policy-cwd ~/wiki --group-by tool --group-by file_path
```

The `agent` field records the CLI (`claude`, `codex`, `agy`, `pi`), not the
agent's name: identify one agent by its `policy_cwd` (the project whose policy
applied) or else its `cwd`. Web and file-tool entries carry `tool` instead of
`cmd`. Decision `pass` means Dippy did not decide and the agent CLI prompted.

Prefer the narrowest rule that covers the workflow. Keep destructive commands,
arbitrary code execution, remote execution, secret-bearing reads, and external
side effects subject to approval unless the user explicitly accepts that scope.

Choose the narrowest configuration file too, not only the narrowest pattern.
A project's own `.dippy` beats `~/.dippy/config`, which widens every other
workspace. A path outside the project is not a reason to move the rule up.

For a project-local `tmp/**` workflow, consider the complete set:

```dippy
allow-redirect tmp/**
allow-read tmp/**
allow-edit tmp/**
allow rm -f tmp/**

# Place these after a broader `ask mkdir *` rule because Dippy uses last match wins.
allow mkdir -p tmp
allow mkdir -p tmp/**
```

Never extend that bundle to executing a file from `tmp/`. The agent can write
there, so an allowed binary in `tmp/` is a planted-binary path.

Do not modify the user's configuration when they only ask for recommendations.
When they explicitly request the change, edit the live configuration surgically
and preserve unrelated content.

Validate representative commands. Use `scripts/debug/try-rules.sh` from the
Dippy repo, which runs the engine with an empty HOME so nothing leaks in, and
`scripts/debug/check-rule.py` for read, edit, redirect, web and MCP rules:

```bash
./scripts/debug/try-rules.sh tmp/candidate-rules 'mkdir -p tmp' 'rm -f tmp/x'
PYTHONPATH=src python3 ./scripts/debug/check-rule.py edit --config-only tmp/candidate-rules tmp/x tmp/../x
```

Write paths out literally - a `$VAR` or `$PWD` in the command is a literal to
Dippy and matches no path rule, so every check turns into its own approval
prompt.

The intended workflow must return `allow`; the out-of-scope control must remain
`ask` or `deny`.
