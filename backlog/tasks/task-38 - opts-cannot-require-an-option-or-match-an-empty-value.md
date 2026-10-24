---
id: TASK-38
title: '[opts:] cannot require an option or match an empty value'
status: Idea
assignee: []
created_date: '2026-10-24 14:06'
labels: []
dependencies: []
priority: low
type: feature
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
A rule cannot allow claude -p only together with --tools "": [opts:] declarations are optional and a legacy pattern word cannot match an empty argument, so the safe form stays on ask. Suggested direction: a required-option marker in [opts:] (including an empty value). Needs analysis before implementation.
<!-- SECTION:DESCRIPTION:END -->
