# Dippy

## Language

Write all repository files in English. Answer users in their language.

## Commands

```bash
just check   # lint + format check + tests
just test    # tests only, Python 3.12
just lint    # ruff check
just fmt     # ruff format --check
```

## Development rules

- Use TDD for every code fix: add a failing test first.
- Treat an overly broad `allow` as a security bug. Test every new allow rule
  against a destructive bypass string using `dippy --cmd` or
  `scripts/debug/try-rules.sh`; never execute the bypass.
- Use fictional command names in rule tests; real names may hit `SIMPLE_SAFE`.
- Isolate config tests with `tmp_path`, `monkeypatch` and an empty `HOME`.
  `--config` adds an override; it does not replace `~/.dippy/config`.
- Validate config changes in the complete file and final rule order; isolated
  snippets can miss a later last-match-wins override.
- Run every direct-execution check with an empty temporary `HOME` and a fake
  non-GUI askpass; never open the real approval dialog during verification.
- Merge scalar settings by membership in `configured_settings`, never by
  comparing their value to the default. Test that a higher-priority scope can
  explicitly restore the default value.
- Use `monkeypatch.setattr` for module globals; bare assignment leaks state
  between tests.
- Never modify the user's live Dippy configuration during repository work.
- Use `dippy.core.parser.tokenize`, never `shlex`.
- Never import inside a function or duplicate a list derivable from code.
- Propagate `context_flags` through every `Decision` and delegated
  `Classification`; preserve `remote=ctx.remote`.
- Propagate global `--config` and `--cwd` through config loading,
  classification, approval and execution for `run`, `run-on-server` and
  `recover`; local execution must pass `cwd` to Bash.
- Treat paths as literal when `remote=True`; never expand them against the host
  working directory.
- Prefer native read and edit matchers over simulated shell commands.

## Git

- Use conventional commits with a `Co-Authored-By` trailer.
- Run `just test` before committing; never commit a red suite.
- Before a `feat:` or `fix:`, bump the version in `pyproject.toml` and
  `src/dippy/__init__.py`, run `uv lock`, and move the changelog entries from
  `[Unreleased]` under the new version.
