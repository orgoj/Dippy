---
id: TASK-47
title: dippy trust re-pins sha256 rules in a --config / config-only policy file
status: Idea
assignee: []
created_date: '2026-10-29 11:42'
labels: []
dependencies: []
priority: medium
type: feature
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
dippy trust edits only the nearest project .dippy. Roaming config-only policies (~/.hcom/dippy/mikrotik.dippy) pin bundle scripts with allow [sha256:H] rules, so every re-approval is a manual hash replace (10 lines for one script on 2026-10-29). Suggested direction: let dippy trust take an explicit config file (e.g. global --config FILE) and re-pin every rule naming the script there. Needs analysis before implementation.
<!-- SECTION:DESCRIPTION:END -->
