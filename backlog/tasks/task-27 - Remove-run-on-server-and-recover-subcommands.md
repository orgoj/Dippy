---
id: TASK-27
title: Remove run-on-server and recover subcommands
status: Done
assignee:
  - '@dippy_dev'
created_date: '2026-10-15 10:48'
updated_date: '2026-10-15 11:04'
labels: []
dependencies: []
priority: medium
type: chore
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
User decision 2026-10-15: remote execution moves to CCA (cca run). Dippy keeps only local 'dippy run' (classify, askpass/dashboard approval, bash -c in cwd). Remove run-on-server, recover and everything that exists only for them.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 dippy run-on-server and dippy recover are no longer CLI subcommands
- [x] #2 Remote transport, run state, server directive and run-on-server-* settings are removed; such config lines only warn and are skipped
- [x] #3 Analyzer, approval payload, askpass, dashboard API and docs no longer reference run-on-server or server
- [x] #4 dippy run still classifies, approves and executes locally (tests pass)
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
Bump 0.6.0; tests first (CLI rejects run-on-server/recover, server/run-on-server-* config lines warn+skip); remove code in main.rs, run.rs, config.rs, analyzer.rs, approval.rs, askpass.rs, admin.rs; update docs; just check; commit; install-rs
<!-- SECTION:PLAN:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Removed run-on-server, recover, config server, server directive, run-on-server-* settings, SSH/tmux/herdr transports and run state; server/flags/remote dropped from approval and askpass payloads; old config lines warn and skip. Verified: cargo test (473 Rust tests) under empty HOME, clippy, cargo fmt, just check, just test.
<!-- SECTION:FINAL_SUMMARY:END -->
