---
id: TASK-48
title: Group duplicate reasons of compound commands by message
status: Done
assignee:
  - '@dippy_dev'
created_date: '2026-10-29 11:59'
updated_date: '2026-10-29 12:16'
labels: []
dependencies: []
priority: medium
type: enhancement
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
A compound command whose parts hit the same rule repeats the reason for every part (analyzer.rs combine joins Decision.reason, which already bakes in "name: message"), e.g. "sed: <msg>, sed: <msg>, sha256sum: <msg>". The same text is the deny/ask reason the agent sees and the audit `cmd` field, so it bloats both. User decision 2026-10-29: group by message, each message once with its distinct command names. Out of scope: wording of rule messages, single-command reasons.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 A compound command whose parts share one message yields "sed, sha256sum: <msg>" (each message once, command names distinct, in first-seen order), covered by a Rust test
- [x] #2 Parts with different messages keep one group per message, joined in first-seen order, covered by a Rust test
- [x] #3 The hook reason to the agent and the audit `cmd` field carry the grouped text, covered by a Rust test
- [x] #4 README fork section and CHANGELOG describe the change
<!-- AC:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
analyzer.rs combine groups reasons by message (group_reasons); Decision keeps the ungrouped parts so nested lists/pipelines group as one. Tests: analyzer compound_reasons_are_grouped_by_message (shared and distinct messages, exact duplicates without message), hook codex_allow_is_silent_until_permission_request (agent reason and audit cmd). README fork section, CHANGELOG 0.6.20. Verified with just check.
<!-- SECTION:FINAL_SUMMARY:END -->
