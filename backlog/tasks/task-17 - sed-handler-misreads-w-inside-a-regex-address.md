---
id: TASK-17
title: sed handler misreads w inside a regex address
status: To Do
assignee: []
created_date: '2026-10-13 05:06'
labels: []
dependencies: []
priority: medium
type: bug
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
In dippy-rs 0.4.60 `sed -n '/w x/p' f.md` asks with reason `sed w` when `x/p` is not an allowed redirect target, while `sed -n '/A w/,/C/p' f.md` is allowed. A `w` plus space inside a regex address is taken for the `w FILE` command. Checking real sed writes (w FILE, -i) against allow-redirect is intended (user, 2026-10-12).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 The sed handler finishes parsing address delimiters before looking for commands
- [ ] #2 sed -n /w x/p f.md is allowed as a read
- [ ] #3 Real w FILE commands and -i are still checked against allow-redirect
- [ ] #4 Rust tests cover both address forms and custom delimiters
<!-- AC:END -->
