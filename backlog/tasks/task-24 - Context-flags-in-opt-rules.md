---
id: TASK-24
title: Context flags in -opt rules
status: To Do
assignee: []
created_date: '2026-10-13 10:31'
labels: []
dependencies: []
priority: medium
type: enhancement
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
allow-opt/ask-opt/deny-opt do not parse [flags] (docs/config.md, Context Flags). A global deny-opt such as the final config's "git commit" --no-verify therefore also applies inside cca-tmux-cli/ssh, where admin agents should only get ask. Parse [flags] and [!flags] on -opt rules like on command rules.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 deny-opt [!cca-tmux-cli,!ssh] "git commit" --no-verify denies locally and leaves a remote git commit --no-verify to the remote rules
- [ ] #2 allow-opt/ask-opt/deny-opt honor required and negated context flags, covered by Rust tests
- [ ] #3 docs/config.md lists -opt among the variants that support context flags
<!-- AC:END -->
