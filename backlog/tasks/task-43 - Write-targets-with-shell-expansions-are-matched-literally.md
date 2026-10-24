---
id: TASK-43
title: Write targets with shell expansions are matched literally
status: Idea
assignee: []
created_date: '2026-10-24 15:37'
labels: []
dependencies: []
priority: low
type: bug
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Write targets containing a shell expansion are checked against redirect rules as written, not as Bash expands them. With allow-redirect /tmp/**, echo hi > /tmp/$T, cp a /tmp/$T, curl -o /tmp/$T URL and wget -O /tmp/$T URL are all allowed, although $T may contain ../ and write elsewhere. Found in the review of TASK-42 (agent_review_dippy_865); on 2026-10-24 the user decided to keep literal matching for now (Dippy guides non-adversarial agents, not a security boundary). Suggested direction: an allow rule must not approve a write target containing an expansion (ask instead), consistently across shell redirects, file commands and CLI handlers. Needs analysis before implementation.
<!-- SECTION:DESCRIPTION:END -->
