---
id: TASK-45
title: Allow local project scripts only while their content matches an approved hash
status: Done
assignee:
  - '@dippy_dev'
created_date: '2026-10-28 17:50'
updated_date: '2026-10-28 18:31'
labels: []
dependencies: []
priority: medium
type: feature
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Problem: allowing a project script by path lets an agent edit the script and then run arbitrary content under that allow. Goal: a rule can pin the approved content of a local script by sha256; the allow applies only while the file matches, so a committed project .dippy works the same for colleagues. The user approves a changed script with dippy trust PATH, which rewrites the pinned hash in the rule. In scope: the [sha256:HEX] rule prefix (beside context flags and [opts:]); direct execution (./scripts/x.sh) and interpreter forms written in the rule (bash ./scripts/x.sh, python scripts/x.py) - the hash applies to the first path word of the pattern resolved against cwd, so no interpreter-specific code; dippy trust PATH; docs and changelog. Out of scope: a separate trust store (variant B, rejected by the user); files the script sources, imports or calls; hardening against deliberate bypasses (TOCTOU between check and execution). Risk: [sha256:...] must not be parsed as a context flag. dippy trust stays ask for agents through the existing built-in dippy handler.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 A rule allow [sha256:HEX] ./scripts/x.sh * allows ./scripts/x.sh (and the same file via an equivalent relative path) only while the file's sha256 equals HEX; symlinks resolve to the target file
- [x] #2 An interpreter rule such as allow [sha256:HEX] bash ./scripts/x.sh * pins the script file, not the interpreter
- [x] #3 Hash mismatch, missing or unreadable file, or remote=True turns the matched rule into ask; the reason never mentions dippy trust or any approval step
- [x] #4 dippy trust PATH rewrites HEX in every sha256-pinned rule of the nearest project .dippy that names PATH, keeps the rest of the file byte-identical, prints old and new hash, and exits nonzero without changing anything when no such rule exists
- [x] #5 README fork section, rule syntax docs and CHANGELOG describe the feature
- [x] #6 dippy trust is deny by default through the built-in dippy handler (message states only that it is not allowed), and a config rule (allow dippy trust *, optionally scoped by context flags) allows it, so the policy owner can grant it per agent on the user's order (tests for both)
- [x] #7 A malformed [sha256:...] block, one on ask/deny/delegate, or a pattern without a literal script path skips the rule with a warning like other invalid rule lines; [sha256:...] combined with context flags or [opts:] keeps both working
<!-- AC:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
User decisions: implement both variants. (A) explicit sha256 pin in a committed project .dippy is the expected common path (works for colleagues). dippy trust is the user's approval command; for variant A it updates the pinned hash in the .dippy rule. The ask/deny message on a hash mismatch must NOT mention dippy trust or any approval hint (agents read it).

User: dippy trust must be allowable by rule; dippy_dev grants it to agents on the user's order. Syntax [sha256:HEX] and dippy trust failing without an existing pinned rule were proposed and not objected to.

User confirmed: [sha256:HEX] follows the existing bracket-prefix syntax ([opts:]), nothing new. dippy trust is denied by default, allowable by rule.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Added [sha256:HEX] allow rules that pin a local script's content (ask on mismatch, missing file or remote), dippy trust PATH rewriting the pinned hash in the nearest project .dippy, and a built-in deny for dippy trust (new handler Deny action) that a rule can override. Verified with config/analyzer/handler unit tests, full cargo test under an empty HOME, clippy, rustfmt, just check, and an end-to-end dippy --cmd run (allow, then ask after changing the script).
<!-- SECTION:FINAL_SUMMARY:END -->
