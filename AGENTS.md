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
- Treat an unintentionally broad `allow` as a safety bug. Dippy guides
  non-adversarial agents; it is not a security boundary. Accept a broad trust
  scope when the user explicitly chooses it. Test every new allow rule against
  a destructive bypass string using `dippy --cmd` or
  `scripts/debug/try-rules.sh`; never execute the bypass.
- Use fictional command names in rule tests; real names may hit `SIMPLE_SAFE`.
- Isolate config tests with `tmp_path`, `monkeypatch` and an empty `HOME`.
  `--config` adds an override; it does not replace `~/.dippy/config`.
- Validate config changes in the complete file and final rule order; isolated
  snippets can miss a later last-match-wins override.
- Run every direct-execution and hook-mode check (`dippy --agy`, `--claude`,
  `--codex`, `--gemini`) with an empty temporary `HOME` and a fake non-GUI
  askpass; never open the real approval dialog during verification. In hook
  modes an unmatched tool falls through to askpass, which prompts the live user.
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

## Feature and fix workflow

For every requested implementation that will be committed as `feat:` or `fix:`:

1. Before the first implementation, test, or documentation edit, choose the
   smallest reasonable version increment. Extend an existing feature with a
   patch release unless compatibility or product scope requires a minor release.
2. Update `pyproject.toml` and `src/dippy/__init__.py`, run `uv lock`, and
   create the dated version section in `CHANGELOG.md`.
3. Implement with the required tests and documentation, then run `just check`.
4. Run `just test` immediately before committing. Use a conventional commit
   with a `Co-Authored-By` trailer; never commit a red suite.
