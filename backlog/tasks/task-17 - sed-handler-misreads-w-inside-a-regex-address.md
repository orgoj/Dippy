---
id: TASK-17
title: sed handler misreads w inside a regex address
status: Done
assignee:
  - '@dippy_dev'
created_date: '2026-10-13 05:06'
updated_date: '2026-10-14 19:46'
labels: []
dependencies: []
priority: medium
type: bug
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
In dippy-rs 0.4.60 `sed -n '/w x/p' f.md` asks with reason `sed w` when `x/p` is not an allowed redirect target, while `sed -n '/A w/,/C/p' f.md` is allowed. A `w` plus space inside a regex address is taken for the `w FILE` command. Checking real sed writes (w FILE, -i) against allow-redirect is intended (user, 2026-10-12).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 The sed handler finishes parsing address delimiters before looking for commands
- [x] #2 sed -n /w x/p f.md is allowed as a read
- [x] #3 Real w FILE commands and -i are still checked against allow-redirect
- [x] #4 Rust tests cover both address forms and custom delimiters
<!-- AC:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
dippy-rs 0.5.6: sed scripts are scanned command by command (GNU syntax) instead of regex search; addresses, regexes and replacements are consumed before commands. sed -n '/w x/p' allows; w/W file names (rest of line) and -i still go to allow-redirect; e detection also catches 1e, s///eg, echo; unparsed scripts ask. Verified: Rust sed tests (8), cargo test 448+36 under empty HOME, clippy -D warnings, fmt, just check, just test.
<!-- SECTION:FINAL_SUMMARY:END -->
