---
id: TASK-32
title: Enforce deny-read rules on recursive directory arguments of shell commands
status: To Do
assignee: []
created_date: '2026-10-18 17:54'
labels: []
dependencies:
  - TASK-29
priority: medium
type: feature
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Follow-up of TASK-29, split out by user decision: TASK-29 filters file arguments only. Recursive directory arguments (grep -r X ., rg X ., rg without a path, fd . dir, tar c dir) can read files matching a deny-read pattern under the directory. Open questions: how to decide whether a deny-read pattern can match under a directory, how to detect recursion without per-program lists, and the impact of broad patterns such as **/.env* (would block rg/grep -r in every project). Not scheduled now.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Recursive directory arguments are denied or asked when a deny-read pattern can match under that directory, with a message naming the alternative (rg --glob '!.env*' or explicit files)
- [ ] #2 The decision on handling broad patterns such as **/.env* is recorded and tested
<!-- AC:END -->
