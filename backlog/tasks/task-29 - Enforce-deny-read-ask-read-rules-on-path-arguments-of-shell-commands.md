---
id: TASK-29
title: Enforce deny-read/ask-read rules on path arguments of shell commands
status: Done
assignee:
  - '@dippy_dev'
created_date: '2026-10-18 15:43'
updated_date: '2026-10-18 19:05'
labels: []
dependencies: []
priority: high
type: feature
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Read rules (`allow-read` / `ask-read` / `deny-read`) are evaluated only for the agent's native Read tool. A shell command receives the same path unchecked: `cat .env`, `grep -r KEY .`, `git show HEAD:.env`, `base64 .env`, `python3 x.py secrets/key`. Any program that gets a path to a secret can read it, so a per-program list of "reading tools" cannot close this.

Verified 2026-10-18 in the roaming auditor policies (`~/.hcom/dippy/security.dippy`, `infra.dippy`): `deny-read **/.env*` blocks Read while `allow cat *` lets `cat .env` through. The same holds for `~/.dippy/config` in every project.

Read rules must act as an extra filter on top of command rules, not a replacement: only deny/ask are enforced on arguments, `allow-read` is never required for shell arguments (that would break `ls /usr/bin`), and no match means no effect. Decide by pattern match, never by file existence (no time-of-check races). Apply in normal and `DIPPY_CONFIG_ONLY` mode.

Path extraction covers non-option arguments, `--opt=value` and `--opt value`, everything after `--`, `~` expansion, resolution against the command's effective cwd including `cd` inside compound commands and subshells, and the part after the colon in `git show <rev>:<path>`. Normalization follows the existing read matcher (`..`, `//`, trailing `/`, lexical and physical symlink forms).

Directory arguments with recursion (`grep -r X .`, `rg X .`, `fd . dir`, `tar c dir`): deny when a `deny-read` pattern can match under the directory, with a message naming the alternative (`rg --glob '!.env*'` or explicit files). If too costly for the first version, ship file arguments first and record the directory case as a follow-up task, never as a silent gap.

Known limitation to document: paths built at runtime (`cat $(echo .env)`, a script opening the file itself) are out of reach of static checks.

Full brief: /home/michael/.hcom/tmp/dippy-read-rules-command-args.md (copied from the user request).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 A `deny-read` pattern matching any path-like argument denies the whole shell command with the message of that read rule, in normal and `DIPPY_CONFIG_ONLY` mode
- [x] #2 An `ask-read` pattern matching a path argument yields ask; a path with no matching read rule leaves the command decision unchanged versus today; `allow-read` is never required for shell arguments
- [x] #3 Documentation (README, using-dippy skill) describes the argument filter and the runtime-built-path limitation (`cat $(echo .env)`)
- [x] #4 Verification: `dippy --cmd` and `dippy --config-only FILE --cmd` evaluations of the test cases against `~/.dippy/config` and `~/.hcom/dippy/security.dippy` match the expected decisions; existing rule tests stay green; `just install-rs` completion gate passed
- [x] #5 Recursive directory arguments (grep -r, rg, fd, tar c) are out of scope; tracked in TASK-32 (user decision)
- [x] #6 Path extraction handles non-option arguments after Bash quote removal, --opt=value, --opt value, arguments after --, cwd changes via cd in compound commands and subshells, and the path of git show rev:path (only git show)
- [x] #7 A local argument is checked only when it names an existing entry (symlink_metadata; only NotFound counts as absent, a dangling link exists); git show rev:path and every remote word are always checked; matching normalizes like the native read matcher (.., //, symlink target). Best effort: files created after the check (incl. same-command symlinks), globs and runtime-built paths are documented limitations (user decision)
- [x] #8 Rust tests cover: cat .env, quoted/escaped/concatenated spellings, cat -- .env, head ./x/../.env, --file=.env, base64 .env, tar czf out.tgz secrets/key, (cd secrets && cat key), cd secrets; cat key, dangling and resolved symlinks, cat .env.example allowed by allow-read **/.env*.example*, an ask-read path, no read rule, missing-file grep patterns and commit messages unchanged, git show HEAD:.env without a worktree file, remote words, and askpass/dashboard re-evaluation of a shell read cause
<!-- AC:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
User decision 2026-10-18: recursive directory arguments split into TASK-32, not addressed now.

First version (every literal word) produced false positives in security.dippy (grep -rn credentials, commit messages); user chose the existence gate; agent_review pre-implementation review applied: NotFound-only absence, Bash quote removal via decode_literal_word, rev:path only for git show, lstat on cwd.join(word), docs state best effort. Remote: every word checked, flagged allow-read exempts (user decision).
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
deny-read/ask-read filter literal shell arguments naming an existing file (git show rev:path and remote words always), merged via combine in analyze_command; match_path unified for read/edit/redirect with remote semantics; askpass decide() analyzes shell requests whole. Verified with cargo test (444+38, empty HOME), clippy, just check, try-rules.sh against ~/.dippy/config and security.dippy, and normal-mode dippy --cmd.
<!-- SECTION:FINAL_SUMMARY:END -->
