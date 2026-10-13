---
id: TASK-8
title: Port scripts/debug/check-rule.py to Rust
status: To Do
assignee: []
created_date: '2026-10-13 05:06'
labels: []
dependencies: []
priority: medium
type: task
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Live `dippy` is dippy-rs, but `scripts/debug/check-rule.py` still evaluates the frozen Python `dippy.core`, so read/edit/redirect/web/mcp rule checks can disagree with live behavior. `dippy --cmd` covers only command rules. Ask the user before starting.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 A Rust CLI checks read, edit, redirect, web and mcp rules against a config with the same inputs check-rule.py accepts (--config-only, --flag)
- [ ] #2 Results match live dippy-rs decisions, covered by Rust tests
- [ ] #3 check-rule.py and the docs/skills that reference it point to the Rust check
<!-- AC:END -->
