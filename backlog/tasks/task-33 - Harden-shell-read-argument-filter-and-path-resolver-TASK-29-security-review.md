---
id: TASK-33
title: Harden shell read-argument filter and path resolver (TASK-29 security review)
status: Done
assignee:
  - '@dippy_dev'
created_date: '2026-10-18 19:32'
updated_date: '2026-10-18 19:42'
labels: []
dependencies:
  - TASK-29
priority: high
type: bug
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Findings of the agent_security review of TASK-29 (commit 8605e6f). Resolver: paths::resolve resets its symlink budget per recursive call, so a symlink cycle overflows the stack (also native Read/Edit/redirect); an absolute argument with symlink then .. is collapsed lexically before link resolution and checks a different file. Read filter gaps: unquoted ~ is dropped by decode_literal_word and quoted '~' is expanded to HOME; input redirects (cat < .env) and attached short-option values (grep -f.env) are unchecked; git show rev:path is missed with git -C/-c, wrappers (timeout, command) and git cat-file; only a first-command cd changes the cwd. User decisions: two or more cd in one command are denied; a single cd changes the cwd for the commands after it; false positives when a search term names an existing protected file (rg -F .env README.md, ls -l .env) are accepted and documented.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 A self-link, a two-link cycle and a legitimate short chain resolve without stack overflow; resolution failure is treated conservatively
- [x] #2 An absolute or relative argument through an existing directory symlink followed by .. is matched against the file the OS opens
- [x] #3 Unquoted ~ and ~/x arguments expand to HOME and are checked; quoted or escaped ~ stays a literal relative path
- [x] #4 Literal input redirect targets (cat < .env) are checked by read rules; attached short-option values naming an existing file (grep -f.env) are checked
- [x] #5 git show/cat-file rev:path is checked with git -C DIR, git -c k=v, and behind transparent wrappers (timeout, command)
- [x] #6 Two or more cd commands in one command are denied with a message naming the allowed form; a single cd changes the cwd for the commands after it, including cd -- DIR
- [x] #7 Docs state the accepted search-term false positives (rg -F .env README.md, ls -l .env when .env exists) and the cd rule
- [x] #8 Rust tests cover every case above; just install-rs completion gate passed
<!-- AC:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Shared link budget in paths::resolve (no stack overflow on cycles); symlink target computed from the uncollapsed path; read filter: literal_arg expands unquoted ~ only, input redirects and -fVALUE checked, git show/cat-file rev:path via git find_action behind transparent wrappers; one cd moves the cwd of later commands, several cd denied. Verified with cargo test (448+38, empty HOME), clippy, just check, try-rules.sh against security.dippy.
<!-- SECTION:FINAL_SUMMARY:END -->
