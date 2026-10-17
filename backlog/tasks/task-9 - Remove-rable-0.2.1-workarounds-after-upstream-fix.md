---
id: TASK-9
title: Remove rable 0.2.1 workarounds after upstream fix
status: To Do
assignee: []
created_date: '2026-10-13 05:06'
updated_date: '2026-10-17 08:22'
labels:
  - 'blocked:external'
dependencies: []
references:
  - 'https://github.com/mpecan/rable/pull/80'
priority: medium
type: chore
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
dippy-rs works around rable 0.2.1 bugs we reported: mpecan/rable#75 (`$'` in double quotes eats the rest), #76 (backtick in double quotes not a cmdsub part), #77 (invalid input without ParseError), #78 (`(( ))` span), #79 (List ending in `&` spans the next newline and here-document body; `effective_end`). Upstream PR mpecan/rable#80 fixes all five and was merged on 2026-10-13. Blocked until a rable release contains it. Note: #77 makes `; echo hi` and `ls && && x` parse errors.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 rust/dippy-rs/Cargo.toml pins the rable release that contains the #75-#79 fixes
- [ ] #2 Workarounds for #75-#79 in rust/dippy-rs/src/ast.rs and scan.rs are removed, with Rust tests still covering each case
- [ ] #3 Parity tools and an audit replay show no new unsafe allow
<!-- AC:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
2026-10-13: mpecan/rable#80 merged into main as 61f3de25; issues #75-#79 closed as completed. Latest release is still 0.2.1 (GitHub and crates.io) and no release PR is open yet; waiting for the next rable release (likely 0.2.2).

2026-10-17: still blocked. crates.io latest is 0.2.1; no release PR open. Open PR mpecan/rable#81 (more bash-oracle fixes) may land before the next release.
<!-- SECTION:NOTES:END -->
