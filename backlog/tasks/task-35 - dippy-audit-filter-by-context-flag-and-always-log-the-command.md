---
id: TASK-35
title: 'dippy audit: filter by context flag and always log the command'
status: To Do
assignee: []
created_date: '2026-10-24 14:06'
updated_date: '2026-10-29 11:52'
labels: []
dependencies: []
priority: medium
type: enhancement
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Goal: identify which agent ran which exact command from `dippy audit` alone, without agent transcripts.

Findings: (1) `command` is written only with `set log-full` (rust/dippy-rs/src/logging.rs, Logs::decision). A config-only policy (e.g. ~/.hcom/dippy/mikrotik.dippy) never sets it: the log path falls back to ~/.dippy/audit.log via default_log_path, but log-full stays false, so the denied sha256sum of mikrotik (2026-10-29 10:53 UTC) has no command. (2) `--grep` searches only command, cmd, message, file_path, tool, suggestion (audit.rs GREP_FIELDS), not context_flags; one agent is found only via --group-by context_flags.

Decisions (user, 2026-10-29): always log `command`; `set log-full` then governs only `suggestion`. Add a new `--context-flag TEXT` filter (substring of any context flag, AND with the other filters).

In scope: logging.rs command gating, audit.rs filter, docs/config.md (log-full description and audit filter docs), README fork section, CHANGELOG.
Out of scope: inheriting log settings from the user config in config-only mode; the long repeated per-subcommand messages in deny `cmd` fields; changing log path defaults.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Every audit entry that has a command records it in `command` without `set log-full` (hook, run and config-only paths), covered by a Rust test
- [ ] #2 `suggestion` is still written only with `set log-full`, covered by a Rust test
- [ ] #3 `dippy audit --context-flag TEXT` keeps only entries with a context flag containing TEXT and combines with --grep by AND, covered by a Rust test
- [ ] #4 docs/config.md, README fork section and CHANGELOG describe the new log-full meaning and --context-flag
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Patch bump (just bump-version) + CHANGELOG section.
2. TDD: failing tests in logging.rs (command without log-full, suggestion still gated) and audit.rs (--context-flag, AND with --grep).
3. Drop the a.full filter on command; add context_flag to audit query/CLI.
4. Update docs/config.md, README fork section; just check, just test, commit, just install-rs, dippy --version.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
More evidence 2026-10-29: a denied sha256sum of the mikrotik agent (config-only policy, Claude, 10:53 UTC) was logged without command, so the denied file could not be identified.
<!-- SECTION:NOTES:END -->
