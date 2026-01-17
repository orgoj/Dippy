---
id: TASK-MEDIUM.2
title: Add wrapper command context flags
status: To Do
assignee: []
created_date: '2026-01-17 17:56'
updated_date: '2026-01-17 18:09'
labels: []
dependencies: []
parent_task_id: TASK-MEDIUM
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Add context flags for wrapper-like commands (ssh, sudo, pkexec, etc.)

**IMPORTANT DISCOVERY from v1 implementation:**

`WRAPPER_COMMANDS` (time, timeout, nice) are transparent - they don't change security context.
But `ssh`, `sudo`, `pkexec` are different - they have CLI handlers that delegate via `analyze()`.

Current flow for `ssh host "rm /tmp/x"`:
1. CLI handler for ssh extracts inner command "rm /tmp/x"
2. Returns `delegate` with `inner_command`
3. `_analyze_simple_command` calls `analyze(inner_command, config, cwd, context_flags)`

To add `ssh` flag, we need to:
1. Modify CLI handlers (ssh.py, sudo.py) to return a `wrapper_context` field
2. In `_analyze_simple_command`, when handling delegate, add wrapper_context to context_flags
3. Then call `analyze(inner_command, config, cwd, context_flags | {wrapper_context})`

Examples:
- `ssh host "rm /tmp/x"` → context has `ssh` flag
- `sudo rm /tmp/x` → context has `sudo` flag

Use cases:
- `deny [ssh] rm *` - deny rm via ssh
- `allow [ssh] rm /tmp/**` - but allow in /tmp
- `allow [sudo] apt install *` - allow apt only via sudo

Reference: docs/plans/2026-01-17-context-aware-rules-design.md
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 CLI handlers return wrapper_context for delegate actions
_analyze_simple_command passes wrapper_context as flag
ssh handler sets wrapper_context="ssh"
sudo handler sets wrapper_context="sudo"
Tests verify [ssh] and [sudo] flags work
<!-- AC:END -->
