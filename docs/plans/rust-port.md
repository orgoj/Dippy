# Rust port of Dippy (unattended cloud task)

Brief for an autonomous overnight session. Read it fully, then work through
the phases in order without asking questions. The Python implementation in
this repository is the specification; where this document and the Python
code disagree, the Python code wins.

## Goal

A Rust binary `dippy-rs` that makes the same approval decisions as Python
Dippy for the same command, config and working directory. Parsing uses the
[`rable`](https://github.com/mpecan/rable) crate (MIT), a Rust Bash 5.3 parser
whose AST and S-expression output match Parable, the parser Python Dippy
vendors in `src/dippy/vendor/parable.py`.

Success is measured, not claimed: the parity report from phase 2 is the
deliverable that tells the user how far the port got.

## Non-negotiable invariants

- Fail closed. Anything the Rust code does not recognise or cannot decide -
  unknown command, unported handler, parse error, unsupported config
  directive - must produce `ask`, never `allow`. An `allow` where Python says
  `ask` or `deny` is a safety bug and outranks every other failure.
- Never execute the commands under test. Classification only.
- Do not change Python behaviour. Phase 1 may add an optional import path;
  every existing test must stay green.
- Never read or write the real `~/.dippy`. Tests use temporary `HOME`.
- Commit after each completed phase and after every working group of
  handlers, so an interrupted session keeps its progress. Conventional
  commits (`feat:`, `test:`, `docs:`). Push only the working branch you were
  given; never force-push, never touch `main` or `orgoj`.

## Repository facts

- Entry point: `src/dippy/dippy.py`. `dippy --cmd 'CMD' --json [--cwd DIR]
  [--config FILE]` prints `{"decision": ..., "reason": ...}`; exit code
  0=allow, 1=deny, 2=ask (`cli_mode`). This is the parity oracle.
- Core: `src/dippy/core/` - `analyzer.py` (AST walk, decision combination),
  `config.py` (config language, rule matching, last match wins),
  `allowlists.py` (`SIMPLE_SAFE` etc.), `paths.py`, `sql.py`, `bash.py`,
  `parser.py` (`tokenize`), `options.py`, `template.py`. About 10k lines
  including the top-level modules.
- Handlers: `src/dippy/cli/*.py`, one per CLI (git, sed, docker, kubectl,
  SQL clients, ...), about 12k lines. Discovered by module name.
- Parable is imported in exactly two places:
  `src/dippy/core/parser.py` and `src/dippy/core/analyzer.py`
  (`parse`, `ParseError`).
- Tests: `tests/` (~12.6k cases). `just check` = tests + ruff + lock and
  vendor checks; `just test` = tests on Python 3.12. Config language
  reference: `docs/config.md`. Project rules: `AGENTS.md` (read it).

## Phase 1 - prove Rable in Python Dippy

Rable's Python package (`pip install rable`, Python >= 3.12) claims to be a
drop-in replacement for Parable (`from rable import parse, ParseError`).

1. In a scratch run, switch both import sites to `rable` and run the full
   suite on Python 3.12.
2. Record every failure in `docs/plans/rust-port-report.md` with the
   command, the Parable result and the Rable result. Distinguish Rable bugs
   from Dippy relying on Parable internals (attributes, node kinds, error
   types).
3. Commit only the report, plus - if the suite is green or nearly so - an
   opt-in switch: environment variable `DIPPY_PARSER=rable` selects Rable,
   default stays the vendored Parable, Python 3.8 support unchanged. Keep the
   switch at module import in `parser.py`, re-exported to `analyzer.py`.

Divergences found here are the known parser risk for the Rust port; list
them so phase 3 does not rediscover them.

## Phase 2 - parity harness (do this before porting logic)

Build the measurement first so every later step is visible.

- Corpus: extract command strings from the test suite into
  `rust/parity/corpus.jsonl` (`{"cmd", "cwd", "config"}` per line). Use a
  small Python script under `rust/parity/` that imports the test modules'
  parametrised cases where practical; add hand-written cases for config
  rules from `docs/config.md`. Aim for thousands of cases, deduplicated.
- Oracle: run Python `dippy --cmd ... --json` for each case with an empty
  temporary `HOME` and store expected decisions in the corpus.
- Runner: `rust/parity/run` (script or cargo test) that runs `dippy-rs` on
  the corpus and writes `rust/parity/report.md`: totals, agreement rate,
  and divergences grouped by first command word and by direction.
  Separate counter for **unsafe divergences** (Rust `allow`, Python not
  `allow`); this must be zero at every commit.
- Compare decisions only. Reasons may differ in wording.

## Phase 3 - Rust core

Cargo workspace in `rust/`, crate `dippy-rs`, depending on `rable`.

Port in this order, running the harness after each step:

1. CLI surface: `dippy-rs --cmd CMD [--json] [--cwd DIR] [--config FILE]`
   with the same output and exit codes as `cli_mode`.
2. Tokenisation and quote handling equivalent to `core/parser.py` and
   `core/bash.py`.
3. Analyzer: AST walk, pipelines, lists, subshells, command substitution,
   redirects, decision combination, `SIMPLE_SAFE` and other allowlists.
4. Config: loading order (user, project `.dippy`, `--config`), every
   directive in `docs/config.md`, path globs with component boundaries,
   context flags, last-match-wins. Unsupported directives are a config
   error that yields `ask`, never silently ignored.
5. Hook output for Claude Code (`--claude`): read the hook JSON on stdin,
   emit the same JSON as Python. Other hook modes later.

## Phase 4 - handlers

Port `src/dippy/cli/*.py` in descending order of corpus divergences (the
harness tells you which). Port each handler together with its unit tests
from `tests/cli/`. SQL handlers share one SQL checker (`core/sql.py`); port
it once. An unported handler falls through to `ask`.

## Out of scope tonight

Dashboard, askpass GUI, `run`/`run-on-server`/`recover`, SSH transport,
statusline, notifier, audit log writing, `config` admin subcommands, the pi
wrapper. Do not start them even if everything else is done; spend leftover
time raising parity instead.

## Definition of done for the session

At the end, whatever phase you reached:

- `cargo test` and `just check` green; Python behaviour unchanged.
- `rust/parity/report.md` regenerated, unsafe divergences = 0.
- `docs/plans/rust-port-report.md` summarises: phases completed, agreement
  rate, top remaining divergence groups, Rable issues found, and the next
  concrete step.
- Everything committed and pushed to the working branch.
