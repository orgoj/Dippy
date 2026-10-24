---
id: TASK-44
title: Check input redirects against read rules
status: To Do
assignee: []
created_date: '2026-10-24 20:41'
labels: []
dependencies: []
priority: medium
type: bug
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
An input redirect (< file) is not checked against deny-read/allow-read rules. Found while allowing hcom handoff for local_models: with the policy ~/.hcom/dippy/local-models.dippy (deny-read /** plus a few allow-read trees), 'hcom handoff --name local_models < /etc/nonexistent-secret' returns allow from the command rule. This affects every allowed command, not only hcom: a command allowed by a command rule can read any file through stdin, bypassing deny-read and the ~/.dippy/secrets denies.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 A command with '< FILE' where FILE is denied by a read rule returns the deny-read decision and its message, even when the command itself is allowed
- [ ] #2 A command with '< FILE' where FILE is allowed by read rules keeps the decision of the command rule
- [ ] #3 Relative input-redirect paths resolve against cwd; with remote=True they stay literal
- [ ] #4 Here-strings and heredocs (<<<, <<) are not treated as file reads
- [ ] #5 Rust tests in rust/dippy-rs cover each case above
<!-- AC:END -->
