---
id: TASK-18
title: Trial run-on-server behind cca-tmux-cli
status: To Do
assignee: []
created_date: '2026-10-13 05:06'
labels:
  - 'scope:local'
dependencies: []
priority: low
type: task
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Trial rollout of `dippy run-on-server`: change only the internal execution path of the local cca-tmux-cli wrapper, keep its CLI unchanged so agents need no new commands or rules. Use Dippy only when it exists and reports version >= 0.3.0; otherwise keep the legacy path. Once a compatible Dippy is invoked, never fall back: deny, missing server declaration, approval failure, backend failure or indeterminate result stay blocked. Keep the wrapper server validation and declare the same aliases with `server SERVER` (effective allowlist = intersection). Afterwards decide whether agents call run-on-server directly.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Allow, ask, deny, quoting, pipelines, multiline commands, redirects, exit codes, timeouts and indeterminate recovery verified through the unchanged wrapper interface
- [ ] #2 The inner command is classified exactly once and never retried after an uncertain result
- [ ] #3 Missing Dippy, older version, malformed version output and compatible version each tested explicitly
- [ ] #4 Mismatched server lists in both directions cannot bypass either allowlist
- [ ] #5 Existing agents and hooks behave unchanged and rollback to the legacy path is simple
<!-- AC:END -->
