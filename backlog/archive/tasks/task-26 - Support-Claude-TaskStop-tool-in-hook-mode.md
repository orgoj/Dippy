---
id: TASK-26
title: Support Claude TaskStop tool in hook mode
status: To Do
assignee: []
created_date: '2026-10-14 09:25'
updated_date: '2026-10-14 19:34'
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

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Invalid premise. In Claude hook mode dippy-rs defers unsupported tools (hook.rs tool_input_payload: hook.defer, no decision), so Claude Code decides itself; TaskStop needs no permission and never prompted. The audit entry 'pass: unsupported tool: TaskStop' is informational, not an ask. Nothing to fix.
<!-- SECTION:NOTES:END -->
