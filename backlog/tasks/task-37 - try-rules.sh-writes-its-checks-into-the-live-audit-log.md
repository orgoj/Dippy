---
id: TASK-37
title: try-rules.sh writes its checks into the live audit log
status: Idea
assignee: []
created_date: '2026-10-24 14:06'
labels: []
dependencies: []
priority: medium
type: bug
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
scripts/debug/try-rules.sh runs dippy --cmd, which logs every check to the user's live audit log as agent cli, polluting audit analysis. Suggested direction: the helper passes a temporary audit-log path or disables logging. Needs analysis before implementation.
<!-- SECTION:DESCRIPTION:END -->
