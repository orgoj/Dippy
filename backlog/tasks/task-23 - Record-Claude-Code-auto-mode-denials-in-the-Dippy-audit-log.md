---
id: TASK-23
title: Record Claude Code auto mode denials in the Dippy audit log
status: To Do
assignee: []
created_date: '2026-10-13 05:35'
labels: []
dependencies: []
priority: low
type: feature
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Since Claude Code v2.1.283 auto mode is the default and its classifier runs after the Dippy hook, so a Dippy allow no longer guarantees execution. Classifier denials are invisible in dippy audit today. A PermissionDenied hook could record them.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 A Dippy hook entry point records Claude Code auto mode classifier denials as audit entries with tool, command and reason
- [ ] #2 dippy audit can filter those entries
- [ ] #3 The hook never changes the permission outcome
- [ ] #4 Setup is documented in docs/hook-systems and covered by Rust tests with a temporary audit-log path
<!-- AC:END -->
