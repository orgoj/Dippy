---
id: TASK-6
title: log rotate
status: Done
assignee:
  - '@michael'
created_date: '2026-01-17 15:53'
updated_date: '2026-01-18 12:15'
labels:
  - log
dependencies: []
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
simple KISS
brainstorm skill use
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 1. Config has log_rotate_max_days field (default 30)
- [x] #2 2. Config parser reads "set log-rotate-max-days N"
- [x] #3 3. Rotation happens once per day (first run after midnight)
- [x] #4 4. Rotated files named audit-YYYY-MM-DD.log (yesterday date)
- [x] #5 5. Old logs deleted when older than log_rotate_max_days
- [x] #6 6. docs/config.md documents the setting
- [x] #7 7. README.md mentions log rotation
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Add log_rotate_max_days field to Config dataclass
2. Parse log-rotate-max-days config in _load_config_file
3. Add _rotate_logs() function with rotation logic
4. Call _rotate_logs() at end of load_config()
5. Update docs/config.md with config reference
6. Update README.md with log rotation note
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
**Implemented daily log rotation for audit logs**

Changes made:
- Added `log_rotate_max_days` field to Config dataclass (default: 30)
- Added config parser for `set log-rotate-max-days N` setting
- Implemented `_rotate_logs()` function that:
  - Checks if rotation already happened today (yesterday's log exists)
  - Renames audit.log to audit-YYYY-MM-DD.log (yesterday's date)
  - Deletes rotated logs older than log_rotate_max_days
- Called `_rotate_logs()` at end of `load_config()`
- Updated docs/config.md with setting reference and rotation explanation
- Updated README.md sample config

**Files modified:**
- src/dippy/core/config.py (added field, parser, rotation function, call)
- docs/config.md (added setting documentation)
- README.md (added sample config)
<!-- SECTION:NOTES:END -->
