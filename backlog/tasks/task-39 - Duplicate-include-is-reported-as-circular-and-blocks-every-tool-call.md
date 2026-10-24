---
id: TASK-39
title: Duplicate include is reported as circular and blocks every tool call
status: Idea
assignee: []
created_date: '2026-10-24 14:06'
labels: []
dependencies: []
priority: high
type: bug
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Including the same file twice from one config (e.g. ~/.dippy/final including ~/.dippy/secrets twice after a partial edit) fails with 'circular include', and Dippy then blocks every tool call, including the edit that would fix it; the user had to repair it by hand. Suggested direction: detect cycles only along the current include chain. Needs analysis before implementation.
<!-- SECTION:DESCRIPTION:END -->
