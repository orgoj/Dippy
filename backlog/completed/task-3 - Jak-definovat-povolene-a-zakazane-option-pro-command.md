---
id: TASK-3
title: Jak definovat povolene a zakazane option pro command
status: Done
assignee: []
created_date: '2026-01-16 06:26'
updated_date: '2026-01-16 12:03'
labels: []
dependencies: []
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
je treba brainstorm skill

potrebuju mit zakazane a povolne option pro konkretni command bez ohledu na poradi
- jak definovat v config ? potrebuji mit listy allow/ask/deny na option
- jak vyhodnocovat ?
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 1. Add allow-opt/ask-opt/deny-opt syntax to config parser
2. Implement option presence matching (prefix + item anywhere)
3. Mix with existing allow/deny/ask rules (first match wins)
4. Add tests

- [ ] #2 Add allow-opt/ask-opt/deny-opt syntax to config parser
Implement option presence matching (prefix + item anywhere)
Mix with existing allow/deny/ask rules (first match wins)
Add tests
<!-- AC:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Option rules: allow-opt/ask-opt/deny-opt <prefix> <item1> <item2>...
Match logic: command starts with prefix AND contains any item
Items are subcommands OR flags, matched anywhere
Mixes with existing rules, first match wins
Minimal code change: reuse existing rule infrastructure
<!-- SECTION:NOTES:END -->
