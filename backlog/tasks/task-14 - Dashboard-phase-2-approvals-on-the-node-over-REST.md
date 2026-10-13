---
id: TASK-14
title: 'Dashboard phase 2: approvals on the node over REST'
status: To Do
assignee: []
created_date: '2026-10-13 05:06'
labels:
  - 'status:needs-discussion'
dependencies: []
priority: low
type: feature
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Dashboard phase 1 shipped a read-only node and hub (--hub NODES_FILE, pull model, allowlisted proxy, dashboard-token, docs/dashboard-api.json). User decisions: REST, hub proxies nodes, nodes added at runtime, networking out of scope, node UI works alone when the network fails. Next: askpass goes to the local node, verdict can come from the hub. Reviewer warning: the token file and NODES_FILE are readable by same-uid agents. Waiting for the user to discuss the proposal (local tmp/dashboard.md).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Askpass asks reach the local node and can be decided there or via the hub
- [ ] #2 Approving requires a key that same-uid agents cannot read
- [ ] #3 Node approvals work when the hub is unreachable
- [ ] #4 docs/dashboard-api.json documents the approval endpoints
<!-- AC:END -->
