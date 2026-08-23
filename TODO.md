# Dippy TODO

## Parser: valid commands rejected as parse errors

Dippy should classify these and answer automatically, but it asks instead:

- `parse error: Expected ) to close subshell`
- `parse error: Unterminated double quote`

## Script detection resolves against the wrong directory

```
(cd ide && python3 migrate-badges.py)
```

is reported as `python3 migrate-badges.py: file not found:
/path/to/project/migrate-badges.py` — the path is resolved against the outer
working directory instead of the one the subshell changed into.

## Claude Code subagents ignore `allow`

A rule that allows a command in the main session still prompts inside a
subagent. Unclear whether this is fixable from the hook side.

## Delegate an `ask` to a second opinion instead of the user

Idea: a decision kind that, instead of prompting, hands the command to an
external judge — e.g. `set judge-command "claude -m haiku -p ..."` — which
answers allow or deny. Every prompt the user would have answered becomes a
model call.

Open questions before this is worth building:

- Latency. The judge runs inside a PreToolUse hook, so every unmatched
  command pays for a full model round trip.
- Failure policy. A judge that times out, errors or answers garbage must
  fall back to `ask`, never to `allow`.
- Prompt injection. The command text comes from the agent being judged, so
  the judge is judging attacker-controlled input about itself.
- Scope. Probably only sensible as an explicit opt-in per rule
  (`judge <pattern>`), not as a replacement for the global default.

Related: nickdavies' `handler-plugins` branch (entry point group
`dippy.handlers`) is the plumbing this would need — an external classifier
loaded into the Dippy process. Same trust question applies: a plugin can
approve commands.

## Upstream and fork work not yet applied

- nickdavies `dippy-sign` — ssh-keygen signatures for project `.dippy`.
  Rejected: the signing key sits in `~/.ssh`, so an agent that can edit
  `.dippy` can usually re-sign it too. `ask-edit` covers the same ground.
- nickdavies `taskwarrior-handler` — not used here.
