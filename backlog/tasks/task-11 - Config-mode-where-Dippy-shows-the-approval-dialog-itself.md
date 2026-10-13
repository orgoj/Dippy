---
id: TASK-11
title: Config mode where Dippy shows the approval dialog itself
status: To Do
assignee: []
created_date: '2026-10-13 05:06'
labels:
  - 'status:idea'
dependencies: []
priority: low
type: feature
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
User idea: a .dippy mode in which Dippy shows the approval dialog on every ask for any agent and returns only allow/deny to the agent, later handled centrally through the web UI. Idea stage; discuss scope with the user first.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 A config setting makes ask decisions go through the Dippy approval flow and the hook returns only allow or deny
- [ ] #2 Without the setting, ask behavior is unchanged for every hook mode
- [ ] #3 Tests run headlessly with a fake askpass
<!-- AC:END -->
