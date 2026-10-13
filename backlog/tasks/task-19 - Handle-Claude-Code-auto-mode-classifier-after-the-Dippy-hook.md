---
id: TASK-19
title: Handle Claude Code auto mode classifier after the Dippy hook
status: To Do
assignee: []
created_date: '2026-10-13 05:06'
labels:
  - 'scope:local'
  - 'status:needs-decision'
dependencies: []
priority: low
type: task
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Since Claude Code v2.1.283 auto mode is the default; its classifier runs after the Dippy hook, so a Dippy allow no longer guarantees execution. On 2026-10-09 it denied an approved policy-owner edit of a .dippy file. The user is trialing auto mode and may disable it. Options: autoMode.environment/allow in user settings (weakens that protection; user decision), or a PermissionDenied hook that records classifier denials in the Dippy audit log.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 The user chose an option or decided to drop the task
- [ ] #2 If the hook option is chosen, classifier denials appear in dippy audit, with tests
<!-- AC:END -->
