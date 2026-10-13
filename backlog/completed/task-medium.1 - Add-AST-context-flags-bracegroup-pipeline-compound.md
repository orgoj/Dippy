---
id: TASK-MEDIUM.1
title: 'Add AST context flags: @bracegroup, @pipeline, @compound'
status: Done
assignee:
  - '@myself'
created_date: '2026-01-17 17:56'
updated_date: '2026-01-17 19:36'
labels: []
milestone: m-0
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
- [x] #1 @bracegroup flag set when entering brace-group node
- [x] #2 @pipeline flag set when entering pipeline node  
- [x] #3 @compound flag set for any compound context (subshell, bracegroup, pipeline, list)
- [x] #4 Tests verify each flag works independently
- [x] #5 Tests verify @compound covers all compound contexts
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Add @bracegroup flag in analyzer.py when entering brace-group node (line 168)
2. Add @pipeline flag in analyzer.py when entering pipeline node (line 69)
3. Add @compound flag that gets set for all compound contexts (subshell, bracegroup, pipeline, list)
4. Create tests in test_analyzer_bugs.py for each new flag independently
5. Create tests to verify @compound covers all compound contexts
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Implementation complete:

- Added @bracegroup flag in brace-group handler (line 170-173)
- Added @pipeline flag in pipeline handler (line 69-71)
- Added @compound flag to all compound contexts:
  - subshell (line 163-165)
  - brace-group (line 170-173)
  - pipeline (line 69-71)
  - list (line 82-84)

Files modified:
- src/dippy/core/analyzer.py - Added context flags to compound constructs
- tests/test_analyzer_bugs.py - Added 3 new test classes:
  - TestBracegroupContext (5 tests)
  - TestPipelineContext (4 tests)
  - TestCompoundContext (7 tests)

All 9688 tests pass across Python 3.11-3.14.
<!-- SECTION:NOTES:END -->
