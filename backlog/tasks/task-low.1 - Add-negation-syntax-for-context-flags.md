---
id: TASK-LOW.1
title: Add negation syntax for context flags
status: To Do
assignee: []
created_date: '2026-01-17 17:56'
updated_date: '2026-01-17 18:09'
labels: []
dependencies: []
parent_task_id: TASK-LOW
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Add negation support: `[!@subshell]` means "only when NOT in subshell"

Examples:
- `deny [!@subshell] cd *` - deny cd when not in subshell
- `allow [!ssh] rm *` - allow rm only when not via ssh

Implementation:
- Parse `!` prefix on flags
- Invert match logic for negated flags

Reference: docs/plans/2026-01-17-context-aware-rules-design.md
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 _extract_context_flags parses ! prefix
Rule stores negated_flags separately from required_flags
Matching checks negated flags are NOT in context
Tests for [!@subshell] denies when not in subshell
Tests for mixed [!@subshell,ssh] - ssh required, subshell forbidden
<!-- AC:END -->
