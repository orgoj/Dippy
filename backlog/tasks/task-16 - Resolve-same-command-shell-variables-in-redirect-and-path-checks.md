---
id: TASK-16
title: Resolve same-command shell variables in redirect and path checks
status: To Do
assignee: []
created_date: '2026-10-13 05:06'
labels: []
dependencies: []
priority: medium
type: enhancement
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
An agent writing `D=tmp/run; bin/report > $D/busy.txt` or `| tee $D/out.csv` gets a prompt on every line, although the literal path is allowed by `allow-redirect tmp/**`: Dippy matches `$D/busy.txt` literally. Still open in dippy-rs 0.4.60 (five of 22 asks for one agent on 2026-09-20). Alternative is an agent-side rule to write literal paths; decide the owning layer first.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 A literal single-word assignment earlier in the same analysed command is substituted before path matching
- [ ] #2 Environment values and command substitutions are never used; undeterminable values keep the current literal behavior
- [ ] #3 D=tmp/x; echo a > $D/f is allowed under allow-redirect tmp/**, and a variable pointing outside the allowed tree still asks
- [ ] #4 Rust tests cover redirects, tee and the fail-closed cases
<!-- AC:END -->
