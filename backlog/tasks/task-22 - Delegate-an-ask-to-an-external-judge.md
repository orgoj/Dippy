---
id: TASK-22
title: Delegate an ask to an external judge
status: To Do
assignee: []
created_date: '2026-10-13 05:34'
labels:
  - 'status:idea'
dependencies: []
priority: low
type: feature
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Idea: instead of prompting the user, hand a command to an external judge (e.g. set judge-command "claude -m haiku -p ...") that answers allow or deny. Open questions: latency (a model round trip inside a PreToolUse hook), failure policy, prompt injection (the command text comes from the agent being judged), scope (probably an explicit per-rule opt-in such as judge <pattern>). Related: nickdavies handler-plugins branch (entry point group dippy.handlers) as plumbing for external classifiers; a plugin can approve commands, same trust question. Discuss with the user before building.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Judging is opt-in per rule; without such a rule behavior is unchanged
- [ ] #2 A judge timeout, error or unparsable answer falls back to ask, never allow
- [ ] #3 Judge latency is bounded by a configurable timeout
- [ ] #4 Tests cover allow, deny and every fallback path without a real model call
<!-- AC:END -->
