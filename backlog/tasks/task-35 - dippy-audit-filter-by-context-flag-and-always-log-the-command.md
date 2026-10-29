---
id: TASK-35
title: 'dippy audit: filter by context flag and always log the command'
status: Done
assignee:
  - '@dippy_dev'
created_date: '2026-10-24 14:06'
updated_date: '2026-10-29 12:11'
labels: []
dependencies: []
priority: medium
type: enhancement
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Goal: identify which agent ran which exact command from `dippy audit` alone, without agent transcripts.

Findings: (1) `command` and `suggestion` are written only with `set log-full` (rust/dippy-rs/src/logging.rs, Logs::decision). A config-only policy (e.g. ~/.hcom/dippy/mikrotik.dippy) never sets it: the log path falls back to ~/.dippy/audit.log via default_log_path, but log-full stays false, so the denied sha256sum of mikrotik (2026-10-29 10:53 UTC) has no command. (2) `--grep` searches only command, cmd, message, file_path, tool, suggestion (audit.rs GREP_FIELDS), not context_flags; one agent is found only via --group-by context_flags.

Decisions (user, 2026-10-29): one complete log: remove the log-full mode and always write `command` and `suggestion`; `set log-full` stays accepted as a silent no-op. Add a new `--context-flag TEXT` filter (substring of any context flag, AND with the other filters).

In scope: logging.rs (Audit.full, Logs::close fallback), config.rs log_full setting, audit.rs filter, docs/config.md, README fork section, CHANGELOG.
Out of scope: inheriting log settings from the user config in config-only mode; duplicate reasons in `cmd` (separate task); changing log path defaults.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 `dippy audit --context-flag TEXT` keeps only entries with a context flag containing TEXT and combines with --grep by AND, covered by a Rust test
- [x] #2 Every audit entry records `command` and `suggestion` when present, with or without `set log-full` (hook, run and config-only paths), covered by a Rust test
- [x] #3 `set log-full` is still accepted silently (no error, no warning) and has no effect, covered by a Rust test
- [x] #4 docs/config.md marks `set log-full` obsolete and documents --context-flag; README fork section and CHANGELOG describe both changes
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Patch bump (just bump-version) + CHANGELOG section.
2. TDD: failing tests in logging.rs (command and suggestion without log-full), config.rs (set log-full accepted silently) and audit.rs (--context-flag, AND with --grep).
3. Remove the full flag from logging; keep parsing `log-full` as a no-op; add context_flag to audit query/CLI.
4. Update docs/config.md, README fork section; just check, just test, commit, just install-rs, dippy --version.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
More evidence 2026-10-29: a denied sha256sum of the mikrotik agent (config-only policy, Claude, 10:53 UTC) was logged without command, so the denied file could not be identified.

User decision 2026-10-29 (revised): one complete log; remove the log-full mode, always write command and suggestion; keep `set log-full` accepted as a silent no-op. Duplicate reasons in `cmd` are a separate task.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Removed the log-full mode: Logs::decision always writes command and suggestion (logging.rs test decision_entry_fields_and_order, hook audit tests); set log-full parses as a silent no-op (config.rs log_settings_parse_and_merge asserts no warnings). Added dippy audit --context-flag (audit.rs test, AND with --grep). Fixed a pre-existing race: hook test helper logged() now uses one temp dir per call. Docs, README fork section, CHANGELOG 0.6.19. Verified with just check.
<!-- SECTION:FINAL_SUMMARY:END -->
