---
id: TASK-39
title: Duplicate include is reported as circular and blocks every tool call
status: Done
assignee:
  - '@dippy_dev'
created_date: '2026-10-24 14:06'
updated_date: '2026-10-24 14:27'
labels: []
dependencies: []
priority: high
type: bug
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Including the same file twice (e.g. ~/.dippy/final including ~/.dippy/secrets twice after a partial edit, or a diamond A->B, A->C, B->D, C->D) fails with 'circular include', because expand_includes in rust/dippy-rs/src/config.rs records every loaded file in one set for the whole expansion. Dippy then blocks every tool call, including the edit that would fix it.

Decision: an include is textual substitution, so a file included again outside the current chain is expanded again (same result as writing its rules twice; last match wins keeps the order the user wrote). Only a file already on the current include chain (A -> B -> A, or self-include) is a cycle.

In scope: cycle detection along the current chain; docs/config.md include section.
Out of scope: warnings for duplicate includes; doctor changes; the Python version.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 A file included twice from the same config loads without error and its rules apply at both positions
- [x] #2 A diamond include (two files each including the same third file) loads without error
- [x] #3 A real cycle (A includes B, B includes A) and a self-include still fail with 'circular include'
- [x] #4 docs/config.md describes cycle detection along the include chain and repeated includes
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Bump 0.6.9 + CHANGELOG. 2. Failing Rust tests in config.rs (duplicate, diamond, cycle, self-include) with tmp dirs. 3. Track the chain: remove the file from the set after expanding it. 4. Update docs/config.md. 5. just check, just test, commit.
<!-- SECTION:PLAN:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
expand_includes tracks only the current include chain (the file is removed from the set after its expansion), so a repeated or diamond include expands again while a self-include or A->B->A still fails with 'circular include'. Rust tests a_file_included_twice_applies_at_both_positions, a_diamond_include_loads, include_cycles_still_fail; docs/config.md updated. Verified: cargo test (empty HOME), clippy, rustfmt, just check, and the debug binary on a duplicate include (allow) and a real cycle (deny).
<!-- SECTION:FINAL_SUMMARY:END -->
