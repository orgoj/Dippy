---
id: TASK-35
title: 'dippy audit: filter by context flag and always log the command'
status: Idea
assignee: []
created_date: '2026-10-24 14:06'
labels: []
dependencies: []
priority: medium
type: enhancement
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Finding a single agent in the audit log needs --group-by context_flags, because --grep does not search context flags; many entries also omit the full command when log-full is off, so policy design falls back to agent transcripts. Suggested direction: a --context-flag filter for dippy audit and logging the command on every entry. Needs analysis before implementation.
<!-- SECTION:DESCRIPTION:END -->
