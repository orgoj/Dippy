---
id: TASK-4
title: Pridat do logu adresar projektu
status: Done
assignee:
  - '@michael'
created_date: '2026-01-16 06:39'
updated_date: '2026-01-16 08:45'
labels: []
dependencies: []
priority: high
---

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 1. Log shows project directory\n2. Log format is clear\n3. Tested on real command
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Read current log format\n2. Add project dir to log output\n3. Test
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Added cwd parameter to log_decision() in config.py. Pass cwd from check_command() in dippy.py. Log JSON now includes 'cwd' field.\n\nFiles modified:\n- src/dippy/core/config.py:722 (added cwd param)\n- src/dippy/core/config.py:739-740 (add cwd to entry)\n- src/dippy/dippy.py:193 (pass cwd)
<!-- SECTION:NOTES:END -->
