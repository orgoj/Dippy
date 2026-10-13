---
id: TASK-20
title: Drop HCOM_INSTANCE_NAME agent-bundle rules after hcom 0.7.28
status: To Do
assignee: []
created_date: '2026-10-13 05:06'
updated_date: '2026-10-13 05:35'
labels:
  - 'scope:local'
dependencies: []
priority: low
type: chore
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
The user config scopes agent-bundle file rules with both ${HCOM_AGENT_NAME} and the older ${HCOM_INSTANCE_NAME}. The second form is needed only until every agent runs hcom 0.7.28+. Multi-path forms (rm a b, ~/... arguments) still ask under [opts:] rules; since 0.5.0 they can use legacy rules with **+.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Every running agent uses hcom 0.7.28 or newer
- [ ] #2 The HCOM_INSTANCE_NAME rule lines are removed from the user config with explicit user consent
- [ ] #3 Bundle reads/edits and file commands still allow for an agent in its own bundle and ask outside it
<!-- AC:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Archived 2026-10-13: machine-local (live user config); moved to the dippy_dev bundle TODO.md.
<!-- SECTION:NOTES:END -->
