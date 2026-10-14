---
id: TASK-26
title: Support Claude TaskStop tool in hook mode
status: To Do
assignee: []
created_date: '2026-10-14 09:25'
labels:
  - rust
  - hooks
dependencies: []
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Claude Code's TaskStop tool (stops the agent's own background task) is logged as 'pass: unsupported tool: TaskStop', so Claude prompts the user. It only stops a task the agent started; classify it in dippy-rs instead of passing.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 A Claude PreToolUse payload for TaskStop gets an explicit Dippy decision instead of pass (unsupported tool)
- [ ] #2 Behavior covered by a Rust test in rust/dippy-rs
- [ ] #3 CHANGELOG and hook docs mention TaskStop
<!-- AC:END -->
