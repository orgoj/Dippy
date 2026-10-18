---
id: TASK-31
title: deny-opt misses attached and = option forms
status: To Do
assignee: []
created_date: '2026-10-18 16:50'
labels: []
dependencies: []
priority: high
type: bug
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
`deny-opt` matches an option only when its value is a separate word. Verified 2026-10-18 in ansible-contimex/.dippy: with `deny-opt mitmdump -s --scripts`, `mitmdump -s evil.py` and `mitmdump --scripts evil.py` deny, but `mitmdump --scripts=evil.py` and `mitmdump -sevil.py` fall through to `allow mitmdump *`. Every policy relying on deny-opt has the same bypass; contimex now carries explicit `deny mitmdump * --scripts=* *` and `deny mitmdump * -s?* *` workaround rules until this is fixed.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 A long option listed in deny-opt also denies its `--opt=value` form
- [ ] #2 A single-dash option listed in deny-opt also denies its attached-value form (`-svalue`) and its occurrence inside a short-option cluster where the program allows it, without denying unrelated options that merely share a prefix (`--set` for `-s`, `-cmdx` handling for multi-letter single-dash options decided explicitly)
- [ ] #3 Rust tests cover every deny-opt form (separate, `=`, attached, cluster) for single-letter, multi-letter single-dash (`duckdb -cmd`, `-init`) and long options, plus lookalike options that must stay allowed
- [ ] #4 Existing deny-opt rules (e.g. `git commit --no-verify` in ~/.dippy/final, duckdb, mitmdump) are verified with `dippy --cmd` after the fix; the contimex workaround rules are removed once deny-opt covers them; `just install-rs` completion gate passed
<!-- AC:END -->
