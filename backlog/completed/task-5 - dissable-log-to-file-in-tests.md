---
id: TASK-5
title: dissable log to file in tests
status: Done
assignee:
  - '@myself'
created_date: '2026-01-17 15:52'
updated_date: '2026-01-17 16:26'
labels:
  - log
dependencies: []
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
testy nesmi zapisovat do log file
v testech musi byt ignorovano zapis do log file nebo to musi byt nejaky test log file
mozna override pres env?
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 po susteni testu nesmi but v normalnim log file nic z testu
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Add DIPPY_TEST_NO_LOG environment variable support to disable logging
2. Modify configure_logging() to check for this env var
3. Add pytest fixture to conftest.py that sets this env var for all tests
4. Verify tests pass and no entries appear in production log file
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
- Added DIPPY_TEST_NO_LOG env var check to configure_logging()
- Added autouse fixture in conftest.py to set env var for all tests
- TestLogging class has own fixture to re-enable logging for its tests

Files modified:
- src/dippy/core/config.py
- tests/conftest.py
- tests/test_config.py
<!-- SECTION:NOTES:END -->
