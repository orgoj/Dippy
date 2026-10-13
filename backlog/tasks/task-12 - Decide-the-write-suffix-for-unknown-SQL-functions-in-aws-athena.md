---
id: TASK-12
title: Decide the (write) suffix for unknown SQL functions in aws athena
status: To Do
assignee: []
created_date: '2026-10-13 05:06'
labels: []
dependencies: []
priority: low
type: task
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
hook_compare reports a reason-text divergence with the same decision: Python says `aws athena start-query-execution (write)` for a query with an unknown SQL function, Rust omits `(write)`.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 The intended reason text is decided
- [ ] #2 Rust either emits the suffix (with a test) or the divergence is recorded as intended in hook_compare
<!-- AC:END -->
