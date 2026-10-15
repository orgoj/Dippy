---
id: TASK-28
title: Wrapper option --subcommand for two-word wrappers (cca run)
status: Done
assignee:
  - '@dippy_dev'
created_date: '2026-10-15 19:44'
updated_date: '2026-10-15 19:53'
labels: []
dependencies: []
priority: medium
type: feature
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Allow a custom wrapper to apply only when its first argument is a given subcommand, so 'cca run [--timeout SEC] -t SERVER -- CMD...' and 'cca run -t SERVER <<EOF' are unwrapped while other cca subcommands stay plain commands. Shape agreed with cca_dev as the minimal change on both sides (user decision, hcom #167186, #167245).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 wrapper cca --subcommand run --cmd -- --context -t --script-stdin - unwraps 'run -- CMD' and a quoted heredoc after the options with flags [cca] and [cca,SERVER]
- [x] #2 Other cca subcommands and positional-SERVER forms are matched as plain commands, not unwrapped
- [x] #3 docs/config.md and CHANGELOG describe --subcommand and --script-stdin -
<!-- AC:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Added WrapperInfo.subcommand (config --subcommand) and script marker '-' in the custom wrapper branch of analyzer.rs; verified with analyzer test wrapper_subcommand_scopes_unwrapping (allow, ask and bypass cases), cargo test under an empty HOME, just check and just test.
<!-- SECTION:FINAL_SUMMARY:END -->
