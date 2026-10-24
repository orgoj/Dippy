---
id: TASK-36
title: Replay an agent's audit commands against a candidate policy
status: Idea
assignee: []
created_date: '2026-10-24 14:06'
labels: []
dependencies: []
priority: medium
type: feature
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Verifying a policy change means extracting the agent's real commands from the audit log (yq over a saved jsonl) and feeding them to scripts/debug/try-rules.sh by hand, repeated for every agent. Suggested direction: try-rules --from-audit or dippy audit --replay CONFIG with the same filters as dippy audit. Needs analysis before implementation.
<!-- SECTION:DESCRIPTION:END -->
