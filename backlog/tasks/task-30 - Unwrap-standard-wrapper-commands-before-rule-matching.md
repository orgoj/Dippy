---
id: TASK-30
title: Unwrap standard wrapper commands before rule matching
status: To Do
assignee: []
created_date: '2026-10-18 16:39'
labels: []
dependencies: []
priority: high
type: feature
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Only commands declared with a `wrapper ...` line (e.g. `cca-tmux-cli --cmd run`) and a few built-in handlers are unwrapped. Standard POSIX/GNU wrappers are matched as ordinary commands, so a rule for the wrapper decides for whatever follows it.

Evidence (2026-10-18): `allow [cca-tmux-cli] nice *` in ansible-wdt/.dippy let `cca-tmux-cli -t gate2 run "nice rm -rf /opt/docker"` through as `allow: nice *` (verified with `dippy --cmd`; the rule is removed now). The WDT policy records the same for `timeout N CMD` and `nohup CMD`. The roaming policies review.dippy, security.dippy, infra.dippy and web-design.dippy each carry a hand-written deny list for `time timeout nice nohup strace ltrace command builtin` because nothing else protects them.

Wrappers in scope: `nice [-n N]`, `ionice [-c N] [-n N]`, `nohup`, `timeout [opts] DURATION`, `time`, `env [-i] [-u NAME] [NAME=VALUE...]`, `stdbuf [opts]`, `chronic`, `command [-p]`, `exec`, `xargs [opts]` (the argument command), `watch [opts]`. Keep the existing handling of `sudo`/`doas` (context flags) and `sh -c`/`bash -c` (delegation).

Full brief: /home/michael/.hcom/tmp/dippy-wrapper-unwrapping.md (copied from the user request).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Each in-scope wrapper prefix is stripped before matching and the inner command is decided by its own rules with the same context; a wrapper allow never decides for an inner command
- [ ] #2 The wrapper itself stays subject to rules: `deny nice *` keeps denying, and `allow nice *` allows only `nice` with no inner command
- [ ] #3 Nested wrappers (`nohup nice -n 19 ionice -c3 sh -c ...`) unwrap recursively and the context (`[cca-tmux-cli]`, `[ssh]`, `[sudo]`) is preserved
- [ ] #4 An unknown wrapper option stops unwrapping and the command asks with a message naming the wrapper
- [ ] #5 The audit log shows both the wrapper and the inner decision
- [ ] #6 Rust tests cover in local and `[cca-tmux-cli]` context at least: `nice rm -rf x`, `nice -n 19 cat file`, `ionice -c3 nice -n 19 tar czf ...`, `nohup sh -c "zgrep x log > /tmp/x" &`, `timeout 30 git status`, `time ls`, `env FOO=1 ./script`, `env -u GITHUB_TOKEN gh api ...`, `stdbuf -oL tail -f log`, `xargs -0 rm -f`, `command -v foo` (stays allowed where `command -v` is allowed), `exec bash`
- [ ] #7 Verification: `dippy --cmd` and `dippy --config-only FILE --cmd` evaluations of the test cases against `~/.dippy/cca-remote-readonly` (via a project .dippy that includes it) and the roaming policies match expectations; existing rule tests stay green; `just install-rs` completion gate passed
<!-- AC:END -->
