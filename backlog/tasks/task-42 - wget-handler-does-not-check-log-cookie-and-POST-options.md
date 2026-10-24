---
id: TASK-42
title: 'wget handler does not check log, cookie and POST options'
status: Done
assignee:
  - '@dippy_dev'
created_date: '2026-10-24 15:07'
updated_date: '2026-10-24 15:36'
labels: []
dependencies: []
priority: high
type: bug
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
rust/dippy-rs/src/cli/wget.rs allows wget when a standalone -O/--output-document FILE is present (FILE is a redirect target) or --spider is present, and never looks at other options. Allowed without checks today: -o/--output-file and -a/--append-output logs, --save-cookies, --rejected-log, --hsts-file, --warc-file; data sends (--post-data, --post-file, --body-data, --body-file, --method POST); command and config execution (-e/--execute, --config, --use-askpass); -b/--background (writes wget-log). Bundled short options (-qO f, -qO-) and getopt_long abbreviations (--output-doc=f, --post-d=x) are not recognized at all.

Fix: parse wget arguments like getopt_long (short bundles; the short options taking an argument use the rest of the word or the next word, including -n; long options by exact name or unique prefix; -- ends options), check every written file against redirect rules and ask for data sends and executed commands. A download without -O still asks; --spider stays allowed when nothing else asks.

In scope: wget handler and tests, CHANGELOG. Out of scope: ~/.wgetrc contents, the default ~/.wget-hsts database, curl.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 -O/--output-document, -o/--output-file, -a/--append-output, --save-cookies, --rejected-log and --hsts-file report their file as a redirect target, also in bundled (-qO f, -qOf) and abbreviated (--output-doc=f, --output-f f) forms
- [x] #2 --post-data, --post-file, --body-data, --body-file, --method with a method other than GET/HEAD/OPTIONS/TRACE, -e/--execute, --config, --use-askpass, --warc-file and -b/--background ask, also bundled or abbreviated and together with -O or --spider
- [x] #3 --spider allows only when no option asks, and its log files are redirect targets
- [x] #4 An option argument is not read as an option (wget --header --spider URL asks as a download) and -- ends the options
- [x] #5 dippy --cmd with deny-redirect **/.bashrc denies wget -O - -o ~/.bashrc URL and wget -qO ~/.bashrc URL
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Bump 0.6.12 + CHANGELOG. 2. Failing Rust tests in cli/wget.rs. 3. getopt_long-style parser, classify over it. 4. Verify with dippy --cmd (empty HOME). 5. just check, just test, commit, review.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Review finding (agent_review_dippy_865): write targets with shell expansions (wget -O /tmp/$T) are matched literally. Same project-wide for shell redirects, cp and curl; pre-existing, not a regression. User decision 2026-10-24: keep literal matching (Dippy guides non-adversarial agents, not a security boundary).
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
wget.rs parses options like getopt_long (SHORT_WITH_ARG incl. -n, exact or unique-prefix long names from the full wget 1.21 list of argument-taking options, -- ends options). Write options are redirect targets, ASK_FLAGS (post/body data, execute, config, use-askpass, warc-file, background) and non-safe --method ask, also with -O or --spider. Tests written_files_are_redirect_targets and data_sends_and_commands_ask. Verified: cargo test (empty HOME), clippy, rustfmt, just check, and the debug binary with deny-redirect **/.bashrc: wget -O - -o ~/.bashrc URL and wget -qO ~/.bashrc URL deny, wget -qO /tmp/x.deb allow, wget -O - --post-d=x ask.
<!-- SECTION:FINAL_SUMMARY:END -->
