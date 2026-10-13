---
id: TASK-10
title: Askpass web UI with a queue of pending asks
status: To Do
assignee: []
created_date: '2026-10-13 05:06'
labels: []
dependencies: []
priority: medium
type: feature
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
The askpass GUI dialog is a stopgap (user, 2026-10-10). Goal: an open web UI with a queue of pending asks, with the GUI switched off. Design notes: tmp/askpass-gui-design.md (local, gitignored).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 The web UI lists pending asks and decides each cause through the REST API
- [ ] #2 It reuses askpass.rs logic (check per cause, statuses, decide_cause, admin::add_rule) instead of duplicating it
- [ ] #3 Layout follows the agreed GUI sections
- [ ] #4 Askpass can run with the GUI disabled and asks still reach the queue
<!-- AC:END -->
