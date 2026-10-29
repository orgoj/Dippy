---
id: TASK-48
title: Group duplicate reasons of compound commands by message
status: To Do
assignee: []
created_date: '2026-10-29 11:59'
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
- [ ] #1 A compound command whose parts share one message yields "sed, sha256sum: <msg>" (each message once, command names distinct, in first-seen order), covered by a Rust test
- [ ] #2 Parts with different messages keep one group per message, joined in first-seen order, covered by a Rust test
- [ ] #3 The hook reason to the agent and the audit `cmd` field carry the grouped text, covered by a Rust test
- [ ] #4 README fork section and CHANGELOG describe the change
<!-- AC:END -->
