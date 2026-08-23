---
name: analyzing-audit-log
description: "Analyze Dippy user audit logs and propose narrowly scoped rules that remove repetitive approval prompts. Use when the user asks to inspect `~/.dippy/audit*.log`, find annoying approvals, or recommend safe Dippy allow rules. Do not use for general application log debugging."
---

# Analyzing the Dippy Audit Log

Group `ask` entries by workflow, reason, working directory, and frequency. Command
frequency alone is not enough: identify what repeated task the user is trying to
complete, such as creating, populating, inspecting, and cleaning a project-local
scratch directory.

Prefer the narrowest rule that covers the workflow. Keep destructive commands,
arbitrary code execution, remote execution, secret-bearing reads, and external
side effects subject to approval unless the user explicitly accepts that scope.

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

Do not modify the user's configuration when they only ask for recommendations.
When they explicitly request the change, edit the live configuration surgically
and preserve unrelated content.

Validate representative commands. Use `scripts/debug/try-rules.sh` from the
Dippy repo, which runs the engine with an empty HOME so nothing leaks in:

```bash
./scripts/debug/try-rules.sh tmp/candidate-rules 'mkdir -p tmp' 'rm -f tmp/x'
```

Write paths out literally - a `$VAR` or `$PWD` in the command is a literal to
Dippy and matches no path rule, so every check turns into its own approval
prompt.

The intended workflow must return `allow`; the out-of-scope control must remain
`ask` or `deny`.
