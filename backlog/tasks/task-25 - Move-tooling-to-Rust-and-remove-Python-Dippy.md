---
id: TASK-25
title: Move tooling to Rust and remove Python Dippy
status: To Do
assignee: []
created_date: '2026-10-13 10:37'
updated_date: '2026-10-14 19:54'
labels: []
dependencies:
  - TASK-8
priority: medium
type: chore
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Python Dippy is unmaintained (AGENTS.md). Today just check/test and CI run only Python (pytest, ruff, uv lock, check_style, parable checksum) and cargo test runs nowhere in just. Every rust/parity tool imports Python dippy; sql_tables.py generated rust/dippy-rs/src/sql/functions.rs from Python dippy.core.sql. Skills add-command, check-coverage, debug, verify-counts, release, using-dippy, analyzing-audit-log and rust/PORTING.md teach Python. scripts/debug/check-rule.py evaluates Python dippy.core (port: TASK-8). Removal also drops the unported dippy-statusline, idle-notifier and moltbot (user decision). Do not start without the user's go-ahead.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 just check runs cargo fmt --check, cargo clippy -D warnings and cargo test; just test runs cargo test; CI runs just check with a Rust toolchain
- [ ] #2 Skills and docs describe only the Rust workflow; rust/PORTING.md is removed; Rust comments no longer cite parity tools and functions.rs is hand-maintained
- [ ] #3 src/dippy, tests/, rust/parity/, tools/, uv.lock and Python packaging are deleted; README installs only the Rust binary; just bump-version updates the remaining version files
- [ ] #4 The completion gate's version check (dippy --version vs. the version source) still works
<!-- AC:END -->
