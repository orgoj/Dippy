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
