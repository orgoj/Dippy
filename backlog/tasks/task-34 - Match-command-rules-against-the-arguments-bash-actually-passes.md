---
id: TASK-34
title: Match command rules against the arguments bash actually passes
status: To Do
assignee: []
created_date: '2026-10-20 13:25'
labels: []
dependencies: []
priority: high
type: bug
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Command rules (allow/ask/deny and per-argument patterns) are matched against the raw shell input instead of the argv bash produces. Only a fully quoted word is unquoted, so mid-word quoting and backslash escapes bypass every ask/deny argument rule: with allow zorp * and ask zorp * --danger *, the commands zorp --dan\ger, zorp \--danger, zorp --dan''ger and zorp --dan"g"er are allowed. Found while adding the cca watchlist migrate --apply ask rule (--ap\ply allows). User decision: matching must use what bash really passes, not the input text; path arguments that exist must also be matched after symlink resolution, as the read filter already does.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Rule matching uses the Bash-decoded value of each literal word: mid-word single/double quotes, backslash escapes and ANSI-C quoting are removed before matching, so zorp --dan\ger, zorp \--danger, zorp --dan''ger and zorp --dan"g"er hit ask zorp * --danger *
- [ ] #2 A word with an expansion (parameter, command or arithmetic substitution, glob, brace) is never decoded into a value that satisfies an allow; it keeps asking as today
- [ ] #3 A path argument that names an existing file is also matched against its symlink-resolved target, sharing the read filter's resolver (symlink budget, .. after a directory link), so a link to a denied path hits the deny
- [ ] #4 Decoding applies equally to local commands, wrappers, SSH/remote words and delegated handlers, preserving context_flags and remote=ctx.remote (remote paths stay literal, no host symlink resolution)
- [ ] #5 Rust tests with fictional command names cover each quoting form, expansions, symlinked paths and remote words; docs/config.md and the using-dippy skill describe matching on decoded arguments
<!-- AC:END -->
