---
id: TASK-LOW.1
title: Add negation syntax for context flags
status: Done
assignee: []
created_date: '2026-01-17 17:56'
updated_date: '2026-01-17 20:49'
labels: []
milestone: m-0
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
- [x] #1 _extract_context_flags parses ! prefix
- [x] #2 Rule stores negated_flags separately from required_flags
- [x] #3 Matching checks negated flags are NOT in context
- [x] #4 Tests for [!@subshell] denies when not in subshell
- [x] #5 Tests for mixed [!@subshell,ssh] - ssh required, subshell forbidden
<!-- AC:END -->
