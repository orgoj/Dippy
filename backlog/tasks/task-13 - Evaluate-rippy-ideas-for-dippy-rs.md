---
id: TASK-13
title: Evaluate rippy ideas for dippy-rs
status: To Do
assignee: []
created_date: '2026-10-13 05:06'
labels: []
dependencies: []
references:
  - 'https://github.com/mpecan/rippy'
priority: low
type: spike
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
mpecan/rippy (MIT) has features worth considering. Research: ~/wiki/2-Knowledge/Research/rippy-deep-dive.md. Each candidate must be verified against dippy-rs first, then implemented or rejected separately.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Redirects through a symlink (e.g. /tmp/evil -> /etc) checked; paths are normalized only lexically today
- [ ] #2 Secret redaction in audit log, reasons and dashboard decided (a dashboard token is stored verbatim in the audit log today)
- [ ] #3 Static check of script files before execution (uv run python ./x.py, bash x.sh, .sql) decided
- [ ] #4 Two kinds of ask (approval vs uncertain) decided for audit, dashboard and Claude auto mode
- [ ] #5 Parser recursion bound on deeply nested input verified
- [ ] #6 Hook crash behavior (fail-open vs fail-closed) decided for dippy-rs hooks
- [ ] #7 Generated catalog of allow reasons, mutation testing and fuzzing decided
<!-- AC:END -->
