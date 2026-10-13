---
id: TASK-21
title: Claude Code subagents ignore Dippy allow
status: To Do
assignee: []
created_date: '2026-10-13 05:34'
labels: []
dependencies: []
priority: medium
type: spike
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
A rule that allows a command in the main Claude Code session still prompts inside a subagent. Unclear whether this is fixable from the hook side.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Reproduced with a fictional allowed command in a main session and a subagent, or shown not to reproduce with current Claude Code and dippy-rs
- [ ] #2 Root cause identified: hook input, Dippy decision or Claude Code permission handling
- [ ] #3 Either fixed in dippy-rs with a test, or the limitation documented in docs/hook-systems
<!-- AC:END -->
