---
id: TASK-40
title: curl handler does not check -sLo and -O output paths
status: Idea
assignee: []
created_date: '2026-10-24 14:06'
labels: []
dependencies: []
priority: high
type: bug
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
The curl handler checks -o FILE and --output FILE against redirect rules, but allows a bundled short option ending in o (curl -sLo /tmp/x.deb URL) and -O/--remote-name (writes to cwd) without any path check. Observed while writing the knowledge policy, which needs explicit denies as a workaround. Suggested direction: treat every output form as a write checked against redirect rules. Needs analysis before implementation.
<!-- SECTION:DESCRIPTION:END -->
