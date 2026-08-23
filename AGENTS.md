# Dippy

Shell command approval hook for AI coding assistants. See [README.md](README.md).

## Language

This repository is English-only: code, comments, documentation, tests, commit
messages, log output and user-facing strings. Answer the user in their own
language, but write every file in English.

## Commands

```bash
just check   # lint + format check + tests in parallel — the gate before committing
just test    # tests only (sequential, Python 3.12)
just lint    # ruff check
just fmt     # ruff format --check
```

Never mutate a module global in a test with a bare assignment; `MODE` was leaked
that way for months and made `test_gemini_failopen.py` pass for the wrong reason.
Use `monkeypatch.setattr`.

`scripts/debug/check-path.py` reports why a path is allowed/asked/denied by the
live config: `PYTHONPATH=src python3 scripts/debug/check-path.py PATH [CWD]`.

`scripts/debug/try-rules.sh RULES CMD...` tries candidate rules against sample
commands with an empty HOME, so the live config cannot leak into the result.

## Paths

- Config: `~/.dippy/config` (global), `.dippy` (project-local)
- Audit log: `~/.dippy/audit.log` — JSONL, fields `cwd`, `decision`, `cmd`, `agent`, `ts`
- Hook log: `~/.claude/hook-approvals.log`
- Written by `log_decision()` in `src/dippy/core/config.py`
- Installed with `uv tool install --force .` from this repo

## Testing

- TDD is mandatory, including one-line bug fixes: failing test first.
- **A wrong rule is a security bug.** Test every new `allow` against a bypass
  attempt, not just the happy path — e.g. `wrapper sub run "rm -rf /"` for each
  wrapper subcommand rule. Run the bypass through `dippy --cmd` or
  `scripts/debug/try-rules.sh`, never by executing it. The whole point of the
  test is that the command is destructive.
- Use fictional command names in rule tests; real ones hit the SIMPLE_SAFE allowlist.
- Isolate from the live system with `tmp_path` and `monkeypatch`.
- `--config X` is an override, not a replacement — `~/.dippy/config` still loads.
  Set `HOME=/tmp/empty` to prove a project config stands alone.
- Never modify the user's live config from a development task.

## Config semantics

- Last match wins.
- Rules beat the SIMPLE_SAFE allowlist; `set` does not.
- Patterns are matched after the Parable parser strips quotes.
- A glob-free pattern matches as a prefix; `|` anchors it exactly.
- Only `ask` and `deny` take a message. `allow` does not.
- `allow-edit` covers Write, Edit and MultiEdit; `allow-read`/`ask-read`/`deny-read` cover Read.

## Landmines

- `shlex` is banned. Use `dippy.core.parser.tokenize`.
- Never import inside a function.
- Never maintain a list that duplicates something derivable from code (e.g. `BUILTIN_COMMANDS`).
- Pass `context_flags` explicitly when constructing `Decision`; when a handler
  delegates via `Classification`, carry `remote=ctx.remote` through.
- `remote=True` means paths are literal — never expand them against the host cwd.
- Prefer the native matchers (`match_read`, `match_edit`) over simulating a bash command.
- Keep `pi_wrapper.py` in sync with `dippy.py` for logging, agent identification and tool handling.
- pi-mono agent-initiated turns need `deliverAs: "followUp"`, or they collide with user input.
- Rotated log files are named for yesterday; the active file keeps the current name.
- Adding a directive? Update the regex in `editors/vscode/syntaxes/dippy.tmLanguage.json` by hand.
- A shell variable in a path defeats every path rule: Dippy sees the literal
  `$R/scripts/...` and cannot normalize it. Write the path out.
- `.dippy` and `~/.dippy/config` get edited by the user mid-turn. Re-read
  immediately before appending, or you commit a duplicate rule.

## Documentation

`README.md` owns user-facing features, `docs/config.md` owns the reference.
Adding tool support also touches `pi-extension/README.md` and `docs/hook-systems/`.

## Git

- Remotes: `upstream`=ldayton/Dippy, `origin`=fork (orgoj/Dippy); `nickdaview`, `tony`, `temathe` are contributors.
- Conventional commits (`feat:`, `fix:`, `chore:`, `docs:`) with a `Co-Authored-By` trailer.
- Run `just test` before committing. Never commit a red suite.
- Bump the version before a `feat:` or `fix:` — the user should never have to ask.
  Both `src/dippy/__init__.py` and `pyproject.toml`, then `uv lock`, then move
  `CHANGELOG.md`'s `[Unreleased]` entries under the new heading.
  (`src/dippy/dippy.py` reads `__version__` dynamically.)
- Verify feature claims against `git diff` and `git log --all --source` before
  writing them down.

## Working with the user

- Explain the intended change and the reasoning first; wait for approval before editing.
- Implement the stated requirement literally. Do not simplify it without asking.
- "Explain this" is not "change this."
- Wait for explicit confirmation before `ExitPlanMode`.
- Fix only confirmed bugs. If a change touches more than five files, get evidence first.
