---
id: TASK-MEDIUM.1
title: 'Add AST context flags: @bracegroup, @pipeline, @compound'
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
Extend context-aware rules with additional AST flags:

- `@bracegroup` - inside `{ ...; }`
- `@pipeline` - part of `cmd1 | cmd2`
- `@compound` - any compound context (subshell, bracegroup, pipeline, list)

Implementation:
- Extend analyzer to track these contexts during AST walk
- Add flags to context_flags set when entering respective nodes

Reference: docs/plans/2026-01-17-context-aware-rules-design.md
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 @bracegroup flag set when entering brace-group node
@pipeline flag set when entering pipeline node  
@compound flag set for any compound context (subshell, bracegroup, pipeline, list)
Tests verify each flag works independently
Tests verify @compound covers all compound contexts
<!-- AC:END -->
