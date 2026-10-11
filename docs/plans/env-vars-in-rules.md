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

## Report

### Implemented

Version 0.4.48 (`just bump-version 0.4.48`, dated `CHANGELOG.md` section).
Commits on `rust-port`:

- `9aae03e` feat: Python implementation and `tests/test_env_rule_patterns.py`.
- `c1920f2` feat(rust): `dippy-rs` port, unit and CLI tests, rule-editor
  prefill, parity cases.
- `b3ef2c1` feat: bracket-expression check (both ports), `docs/config.md`
  section "Environment Variables in Patterns", README fork-enhancements entry.

Both parsers expand `${NAME}` at config load through one function per port
(Python `expand_env`/`_with_env`/`_add_rule` in `src/dippy/core/config.py`,
Rust `expand_env`/`with_env`/`push_rule` in `rust/dippy-rs/src/config.rs`).
`Rule` gains `written` (pattern as written, `None` without a reference) and
`inactive`. Matching uses the expanded `pattern`; every matcher skips inactive
rules; every `Match` carries `rule_display(rule)` / `Rule::display()`, so the
audit `rule` field, reasons, `DIPPY_RULE`/askpass message and askpass/dashboard
causes show `${NAME}`. The askpass dialog and the dashboard rule editor prefill
a literal `${` as `$${` (Python has no rule editor). `dippy config add-rule`
stores the user's own text unchanged.

Expansion order per rule: context flags, `[opts:]` block, message and `|`
anchor are split off first; tilde expansion runs on the written pattern; then
`${NAME}` is substituted. Values are written literally per kind: `*`, `?`, `[`
become `[*]`, `[?]`, `[[]` in glob kinds; in `[opts:]` patterns the escaped
value is additionally backslash-escaped for Bash decoding; option-rule words
(exact comparison) take the value as is.

### Rule kinds supported

| Directives | Kind |
|---|---|
| `allow`, `ask`, `deny`, `delegate`, `after` | command glob |
| `allow`/`ask`/`deny`/`delegate` `[opts: ...] PATTERN` | positional Bash words after the block |
| `allow-opt`, `ask-opt`, `deny-opt` (prefix and every item) | exact words |
| `allow-redirect`, `ask-redirect`, `deny-redirect` | path glob |
| `allow-edit`, `ask-edit`, `deny-edit` | path glob |
| `allow-read`, `ask-read`, `deny-read` | path glob |
| `allow-mcp`, `ask-mcp`, `deny-mcp`, `after-mcp` | text glob |
| `allow-web`, `ask-web`, `deny-web`, `after-web` | text glob |

Not expanded: messages, `set` values, `include` paths, `alias`, `wrapper`,
`server`, `python-*` arguments, `[flags]` brackets, the `[opts: ...]` block.

### Warnings (identical in both ports)

- `FILE: line N: ${NAME}: unset or empty (rule inactive)`
- `FILE: line N: ${NAME}: value is not valid UTF-8 (rule inactive)`
- `FILE: line N: ${NAME}: value contains a NUL or newline (rule inactive)`
- `FILE: line N: ${NAME}: value contains whitespace (rule inactive)`
- `FILE: line N: ${NAME}: value contains a '.' or '..' path component (rule inactive)`
- `FILE: line N: ${NAME}: value starts with '~' (rule inactive)`
- `FILE: line N: unterminated variable reference '${...' (skipped)`
- `FILE: line N: invalid variable reference '${...}' (skipped)`
- `FILE: line N: variable reference inside quotes '${NAME}' (skipped)`
- `FILE: line N: variable reference inside a bracket expression '${NAME}' (skipped)`

### Deviations from the specification (all narrower)

1. **Invalid `${` skips the line with a warning** (`FILE: line N: ... (skipped)`),
   the channel every other rule syntax error uses, instead of a fatal
   `ConfigError`. A fatal error would be broader in Gemini and AGY hook modes,
   which answer `allow` on a config error.
2. **`~` at the start of a word or path from a value makes the rule inactive.**
   Path classification happens again at match time and would tilde-expand it;
   no glob escape keeps a leading `~` literal without turning the token into
   a relative path.
3. **`.`/`..` components are also unsafe in command and `[opts:]` patterns**,
   not only in path patterns: command tokens are path-normalized at match time
   too. A non-UTF-8 value is unsafe in every kind.
4. **A reference inside a glob bracket expression (`[${NAME}]`) is a syntax
   error**: inside a set, a value like `a-z` would be a range, not literal text.
   The check over-approximates (an unclosed `[` before a reference also
   errors).
5. **`[opts:]` patterns**: a reference inside quotes or after a backslash is a
   syntax error (the escaped value cannot be inserted there); the
   `[opts: ...]` block itself is not expanded.
6. **A value with glob metacharacters turns an implicit-prefix command rule
   into an exact glob rule**: the escaped `[*]` counts as a glob character, so
   `allow tool ${X}` with `X='a*'` matches `tool 'a*'` but not
   `tool 'a*' more` (add ` *` to the pattern). Identical in both ports.
7. Rust unit tests for the expansion were written after the Rust code (the
   Python tests, written first, define the behaviour and were ported); the
   bracket check was test-first in both ports.

### Verification

Code at `b3ef2c1` (the report commit changes only this file):

| Command | Result |
|---|---|
| `just lint` | All checks passed |
| `just fmt` | 278 files already formatted |
| `just lock-check` | resolved, up to date |
| `just check-style` | no findings |
| `just test` (as root) | 12735 passed, 1 failed, 1 skipped: `tests/test_config.py::TestLoadConfig::test_unreadable_user_config` fails only because root can read a mode-000 file (fails on the base commit too) |
| `pytest -n auto` as user `nobody` (empty `HOME`) | 12736 passed, 1 skipped |
| `cargo test --manifest-path rust/Cargo.toml` | 411 + 9 + 21 passed, 0 failed |
| `cargo clippy --manifest-path rust/Cargo.toml --all-targets -- -D warnings` | clean |
| `cargo fmt --manifest-path rust/Cargo.toml --check` | clean |
| `rust/parity/run` (release build) | 13422 cases, 13419 agree, 3 diverge (pre-existing Python allow -> Rust ask), 0 unsafe |
| `rust/parity/config_compare.py` | 10 scenarios, 91 steps, 0 differ (new scenario: env-rule warnings) |
| `rust/parity/hook_compare.py` | 1734 payloads, 1734 same decision, 0 unsafe; 383/383 hand-written identical (new: 140 env-rule payloads in Claude and AGY modes over 5 environments); 2 corpus reason-text diffs (parse-error wording, pre-existing) |
| `rust/parity/pi_compare.py` | 166 payloads, 0 differ |
| `rust/parity/run_compare.py` | 15 scenarios, 63 steps, 0 differ |
| `rust/parity/log_compare.py` | 0 differ |
| `rust/parity/dashboard_compare.py` | 136 steps, 0 differences |
| `rust/parity/doctor_compare.py` | 28 scenarios, 129 steps, 0 differ |
| `dippy --config-only RULES --cmd ...` and `dippy-rs` (empty `HOME`, `DIPPY_ASKPASS=/bin/false`) with the bundle example | allow only `.../agents/bot1/bin/tool x`; ask for `; rm -rf /`, `../../bin/rm -rf /`, `$(rm -rf /)`, `> /etc/passwd`, `bot10`, unset variables and `HCOM_INSTANCE_NAME='*'` (identical in both ports) |

Hook-mode end-to-end tests (`test_claude_hook_end_to_end`,
`test_agy_askpass_receives_rule_as_written`,
`env_rule_patterns_in_claude_hook_mode`) run with an empty `HOME` and a fake
askpass that records its input and exits 2.

### Open questions

- Should an invalid `${` be fatal (`ConfigError`) after all? That would need a
  decision on the Gemini/AGY fail-open answer to config errors.
- Python passes no `rule` to askpass (only the message, which contains
  `[${NAME}... @ FILE]`); dippy-rs sends causes with the written pattern.
  Pre-existing difference, unchanged.
- `just test` cannot pass as root (`test_unreadable_user_config`); the cloud
  sessions run as root.
