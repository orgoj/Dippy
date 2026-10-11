# Environment variables in rule patterns (unattended cloud task)

Brief for one autonomous session; nobody can answer questions. Read it
fully, then work through the steps in order. Read `AGENTS.md`,
`rust/PORTING.md` and `docs/config.md` (sections on rules, context flags,
path rules and tilde expansion) first. Work on branch `rust-port`.

## Goal

Rule patterns can reference environment variables of the Dippy process as
`${NAME}`. This is a general feature for every rule kind, in Python Dippy
and in `dippy-rs` with identical behaviour. A typical use (documentation
example only, nothing in the code is specific to it): one shared config
lets each agent edit only its own directory,

```text
allow-edit ${HCOM_DIR}/agents/${HCOM_INSTANCE_NAME}/**
allow ${HCOM_DIR}/agents/${HCOM_INSTANCE_NAME}/bin/* *
```

Dippy catches mistakes of non-adversarial agents; it is not a security
boundary. Still, an unintentionally broad `allow` is a safety bug: the
design below makes every failure case narrower, never broader.

## Specification (decided; implement exactly this)

1. **Syntax.** `${NAME}` with `NAME` matching `[A-Za-z_][A-Za-z0-9_]*`.
   `$NAME` without braces keeps today's meaning (literal text); `[$NAME=value]`
   context flags are unchanged. `$${` writes a literal `${`. Any other
   `${` (unterminated, empty or invalid name) is a config error with the
   file and line, like other syntax errors.
2. **Where.** In the pattern of every rule directive: command rules
   (`allow`/`ask`/`deny`, `delegate` and any other directive whose pattern
   is matched against a command), redirect, edit and read path rules, MCP
   and web rules, and every other place where `docs/config.md` documents a
   rule pattern. Find the full list in the parsers (Python
   `src/dippy/core/config.py`, Rust `rust/dippy-rs/src/config.rs`) and list
   it in the docs. Not in rule messages, `set` values, `include` paths or
   context flag brackets.
3. **When and from where.** Expand at config load from the environment of
   the Dippy process. Hook modes see the agent CLI's environment;
   `dippy run`/`--cmd` see the caller's environment. Document this; a
   process that sets the variable itself can change what the rule means
   (accepted: non-adversarial model).
4. **Unset or empty variable.** The rule is inactive: it never matches,
   and config load emits one warning naming the file, line and variable
   (through the existing warning channel). Never substitute an empty
   string.
5. **Values are literal.** Glob and pattern metacharacters in a value
   (`*`, `?`, `[`, `]` and anything else special in that rule kind's
   pattern syntax) match only themselves. A value is never tilde-expanded;
   only a `~` written in the pattern keeps its current meaning.
6. **Unsafe values make the rule inactive with a warning:** a value with a
   NUL or newline; in command patterns, a value with whitespace (it would
   change token boundaries); in path patterns, a value with a `.` or `..`
   path component. A value containing `/` is allowed (variables often hold
   paths).
7. **Display.** Matching uses the expanded pattern. Everything that shows
   or writes a rule (audit `rule` field, askpass/dashboard causes and rule
   editor, `dippy config` output, `config add-rule`) shows the pattern as
   written, with `${NAME}`. Rules written by Dippy itself never introduce
   `${`.
8. **Remote.** `remote=True` behaviour does not change; expansion happens
   before matching and paths stay literal there.
9. **Python and Rust behave identically,** including warnings text and
   config errors; extend the parity tools (`rust/parity/config_compare.py`,
   `hook_compare.py` or whichever compares rule matching) with env cases.

## Required tests (TDD: each test fails before its implementation)

Use fictional command names, an empty temporary `HOME`, `tmp_path` /
temp dirs and explicit test environments; never read the real
`~/.dippy`. Cover in Python and Rust:

- expansion in each rule kind from point 2;
- unset and empty variable: rule inactive, warning emitted, later rules
  and default decision unaffected;
- literal metacharacters: `NAME='*'` does not widen a rule;
- lookalikes: with `HCOM_INSTANCE_NAME=bot1`, the pattern
  `${HCOM_DIR}/agents/${HCOM_INSTANCE_NAME}/**` allows
  `<dir>/agents/bot1/x` but not `<dir>/agents/bot1_evil/x`,
  `<dir>/agents/bot10/x` or `<dir>/agents/x`;
- unsafe values from point 6 (`../x`, `.`, value with space, newline);
- `$${` literal, `$NAME` literal, invalid `${...}` config error;
- last-match-wins order with env rules in the complete file;
- display keeps `${NAME}` (audit `rule`, askpass causes);
- hook mode end to end with the variable set and unset
  (`--claude` payload, fake non-GUI askpass, empty `HOME`).

## Workflow and stop rules

- Before the first edit: `just bump-version 0.4.48` and a dated
  `CHANGELOG.md` section (feat). Then implement Python and Rust, docs
  (`docs/config.md`: syntax, where, unset/unsafe behaviour, trust note,
  the bundle example; replace the sentence "Environment variables (`$HOME`)
  are not expanded" accordingly), and the README fork-enhancements list.
- Gates before the commit: `just check`, `just test`,
  `cargo test --manifest-path rust/Cargo.toml`,
  `cargo clippy --manifest-path rust/Cargo.toml --all-targets -- -D warnings`,
  `cargo fmt --manifest-path rust/Cargo.toml --check`, every parity tool
  under `rust/parity/` that touches config or matching (rebuild
  `rust/target/release/dippy-rs` first). Never commit a red suite.
- Conventional commit `feat: ...` with the `Co-Authored-By` trailer; push
  `rust-port`. Commit and push working intermediate states (Python first,
  then Rust) so an interruption leaves finished steps. Never force-push,
  never touch `main` or `orgoj`.
- Never start a GUI, a browser, a real agent, `tmux`, `ssh` or `herdr`.
- If a point of the specification turns out impossible or contradicts
  existing behaviour, choose the narrower (safer) option, implement it,
  and record the deviation in the report. After three failed attempts at
  one step, revert its uncommitted changes, record the blocker and stop.

## Report

Append a section `## Report` to this file (committed with the final
change): what was implemented, the exact list of rule kinds supported,
deviations from the specification with reasons, every verification command
with its result and the commit hash it applies to, and open questions.
