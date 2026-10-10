# Rust port report

Status report for the unattended Rust port described in
[`rust-port.md`](rust-port.md). Updated at every phase.

## Summary

Phases 1-4 are complete. Out-of-scope items were not started.

Cost: phases 1-4 ran as one unattended Claude Code cloud session overnight
on 2026-10-09/10 (Opus 5.5, effort medium) and used about $72 of cloud
session credit.

| Measure | Result |
| --- | --- |
| Decision agreement (`rust/parity/run`, 13,422 cases, regenerated 2026-10-10 for 0.4.23) | **13,419 (99.98%)** |
| Unsafe divergences (Rust allow, Python not allow) | **0** |
| Remaining divergences | 3, all Python bugs where Rust deliberately asks (below) |
| Identical reason text (informational) | 13,182 (98.4%) |
| Handlers ported | 89 of 89 modules (140 command names) |
| Handler parity (`handler_compare.py`, identical inputs) | 8,843 same, 0 different, 0 unsafe |
| Parser trees identical to Parable (`ast_compare.py`) | 13,259 of 13,286 commands; the rest fail closed |
| Hook output, all modes (`hook_compare.py`, 1,594 payloads: Claude, Gemini, Codex, Cursor, AGY, auto-detected) | 1,594 same decision, 0 unsafe, 1,588 byte-identical; all 243 hand-written tool/event payloads byte-identical (the rest differ in corpus reason text only) |
| Hook logs and `audit` (`log_compare.py`: 937 runs of the hook_compare payloads, 24 `audit` queries, rotation) | audit log and `hook-approvals.log` identical (timestamps masked, format checked), 0 query differences, identical rotation; 18 spurious Codex `pass` entries from Python dropped (Python bug, below) |
| `cargo test` | 320 tests pass (309 unit, 11 CLI); `cargo clippy --all-targets -- -D warnings` clean on the pinned toolchain (`rust/rust-toolchain.toml`, 1.99.0) |
| Latency per call (same command, warm cache) | Python ~184 ms, dippy-rs ~2 ms |

Remaining divergences (Python allows, Rust asks): `echo $((1 + $(rm x)))`,
`echo "$((ls) && (rm x))"` and `a=(1 2 $(rm x))`. Python ignores command
substitutions inside arithmetic expansion and array literals; this is a
Python safety bug, recorded below, not fixed here because Python behaviour
must not change in this task.

`just check`: lint, format, lock and style checks pass. One Python test,
`tests/test_config.py::TestLoadConfig::test_unreadable_user_config`, fails
only because the container runs as root (`chmod 000` does not block root);
it passes as an unprivileged user. It fails identically on the unchanged
base branch.

### Top remaining divergence groups

None besides the three Python-bug cases above. Known classes where Rust
asks and Python allows, which the corpus exercises only lightly:

- SQL outside the conservative Rust grammar (STRUCT/array casts, `CONVERT`,
  `INTERVAL`, `EXTRACT`, lateral joins, tagged `$tag$` strings, ...): the
  Rust checker never proves these read-only; Python verifies them with
  SQLGlot. Measured directly: `is_readonly_sql` agrees on 964 of 1,060 SQL
  strings from tests and corpus, 0 unsafe; 90 of the 96 differences are
  Python `False` vs Rust `None` (both ask).
- Python source accepted by the Rust Python parser but not by CPython 3.12,
  very deep nesting, and syntax-error wording (python handler).
- Rable-accepted inputs Parable rejects (`3>`, `; SELECT 2`, `ls &&`): ask.

### Next concrete step

Ported (2026-10-10): classification, the MCP/web/file-tool matchers
(`match_mcp`, `match_web`, `match_edit`, `match_read`, `after-mcp`,
`after-web`), the Claude/Gemini/Codex/Cursor/AGY hook formats including AGY
askpass approval, the audit log, `hook-approvals.log` and `dippy-rs audit`.

Plan for the switch from Python (decided 2026-10-10):

The installed binary stays `dippy`; "dippy-rs" only names the Rust
implementation (and the development binary). Config paths stay `~/.dippy/`
and `.dippy`. There is one switch, when the Rust port covers everything: the
Rust binary is installed as `dippy` and the Python package removed; rollback
is reinstalling the Python package.

Done (0.4.24): `--help`, `--version` (the crate version follows
`just bump-version`) and `just install-rs [DEST]`, which installs the release
binary as `DEST/dippy` (default `~/.local/bin`); it is run for real only at
the switch.

Done (0.4.25): `config get/set/unset` and `config server add/remove/list`
(`rust/parity/config_compare.py`: 90 steps, 0 differences). `config set`
rejects a value containing a line break; Python writes it and so adds the
following text as a new config line.

Done (0.4.26): `hooks list/install/uninstall/setup-gemini-yolo` with
Python's output (including the `--dry-run` difflib diff), JSON formatting,
backups and Codex `config.toml` edits (`rust/parity/hooks_compare.py`: 34
scenarios, 198 steps, 0 differences). Intentional divergences: a hook is
Dippy's only when its program is `dippy` or `dippy-hook` (Python matched any
command containing `/dippy` and removed such unrelated hooks on install), the
upgrade hint has no double space without `--global`, and a malformed hook
structure is an error instead of a traceback.

Done (0.4.27): `dippy-rs --pi` speaks the `pi_wrapper.py` JSON protocol
(`bash`/`read`/`edit`/`idle`, `deny-format` templates, audit log;
`rust/parity/pi_compare.py`: 166 payloads, 145 audit entries, 0
differences). `note` is always null (no notifier). The pi extension calls
`dippy --pi` and blocks on any reply without a known action, so the Python
`dippy --pi` (Claude format) cannot fail open.

Done (0.4.28): `doctor [--agent] [--verbose] [--json] [--quiet] [--fix]`
with Python's checks, output and exit codes; `--fix` runs `hooks install`
(`rust/parity/doctor_compare.py`: 28 scenarios, 129 steps, 0 differences).
Intentional divergences: no `--agent moltbot`; legacy hooks detected as in
`hooks`; no `pi_wrapper.py` bridge line; Codex matchers read from `matcher`;
a non-object hook config or `--agent` with only a project config no longer
raises a Python exception.

Done (0.4.31): the `dippy-askpass` approval dialog, redesigned rather than
ported (whole request, per-cause rule editor, `config add-rule`).

Done (0.4.32): `run`, `run-on-server` and `recover` with the ssh, tmux and
herdr backends, isolated SSH profiles and Python-compatible run state
(`~/.dippy/run-state`, same keys and JSON), so `recover` works across the
switch (`rust/parity/run_compare.py`: 15 scenarios, 63 steps, 0
differences; fake `ssh`/`tmux`/`herdr`/askpass, nothing connects).
Intentional divergences: a failed tmux command reports its stderr as text
(Python shows `b'...'`), a corrupt state file reports serde's JSON error, and
OS errors Python raised as tracebacks are printed as messages.

Done (0.4.33): askpass causes carry their context flags; `run`/
`run-on-server` send causes, `flags` and `remote`, and the dialog saves
tagged rules (`allow [run-on-server,srv] ...`).

Done (0.4.34-0.4.41): `dashboard` (node and hub) and the dashboard
approval queue; see [Dashboard and approval queue](#dashboard-and-approval-queue).
Nothing remains before the switch.

Nothing calls Python after the switch. Not ported: `dippy-statusline` (a generic
Claude Code status line, unrelated to approvals), `idle-notifier-command` and
moltbot (renamed upstream many times).

From 0.4.23 on, safety fixes go to `dippy-rs` only; a Python difference is
recorded as an intentional divergence (Rust asks, Python allows).

Intentional hook divergences: a config error fails closed in Gemini and AGY
modes (Python allows everything), and where Python prints `null` to Codex
(rule-matched allows without `hook_event`) dippy-rs prints nothing. For the
same reason Python follows each such Codex allow with a `pass`/`no matching
rule` audit entry (`approve()` returns `None`); dippy-rs logs only the
allow. `log-rotate-max-days` and `log-hook-approvals` merge by membership in
`configured_settings`, so a higher scope can restore the default (Python
compares with the default). Rotation file errors are ignored (Python
raises), and Python's stderr copy of hook warnings is not written.

Rable bugs listed under phase 1 are reported upstream as mpecan/rable#75
(`$'`), #76 (backticks), #77 (silent recovery) and #78 (`(( ))` span).


## Dashboard and approval queue

Second unattended session (2026-10-10, brief
[`rust-dashboard.md`](rust-dashboard.md)). All nine steps were done in the
brief's order; none was skipped. Every step ended green and was committed
and pushed separately.

| Step | Version | Result |
| --- | --- | --- |
| 1. Dashboard port (parity) | 0.4.34 | `dippy-rs dashboard [--host] [--port] [--token-file] [--hub]`; `dashboard_compare.py`: 136 steps, 0 differences |
| 2. Settings, payload v2 | 0.4.35 | `approval`, `approval-timeout-message`, user-only `dashboard-url`/`dashboard-token-file`; `approval.rs` builds and validates version 2 |
| 3. Node queue (REST) | 0.4.36 | `/api/approvals` contract first, router compared with it by a test |
| 4. Claude, AGY, `run` client | 0.4.37 | end-to-end tests against an in-process node; per-mode identity test for `set approval ask` |
| 5. Web UI (node) | 0.4.38 | Approvals view and popup; smoke-tested headless with Playwright/Chromium (queue, live check, save, arming, Ctrl+Enter, deny with note) |
| 6. Gemini, Cursor, Codex, pi | 0.4.39 | Codex only on `PermissionRequest`; the pi extension has no timeout of its own, unchanged |
| 7. Hub | 0.4.40 | aggregated list and forwarded decision/check/rule; smoke-tested headless through a hub |
| 8. Docs | - | `docs/config.md` "Dashboard Approvals (dippy-rs)", README, systemd user unit, `docs/dashboard-api.json` |
| 9. Report | - | this section |

A fix found in review (0.4.41): a Gemini `ask` for an unmatched tool reached
the queue without its tool name.

### Verification

Every parity tool was run on the pre-session commit (`1039435`) and on the
final code with identical inputs; the outputs are identical (only sandbox
paths differ):

| Tool | Result (before = after) |
| --- | --- |
| `rust/parity/run` | 13,422 cases, 13,418 agree, 0 unsafe |
| `hook_compare.py` | 1,594 payloads, 1,585 identical output, 1,594 same decision, 0 unsafe |
| `log_compare.py` | audit/`hook-approvals.log` differences in corpus reason text only (exit 1 before and after), 24 queries 0 differ, rotation identical |
| `config_compare.py` | 90 steps, 0 differ |
| `hooks_compare.py` | 198 steps, 0 differ |
| `pi_compare.py` | 166 payloads, 0 differ |
| `doctor_compare.py` | 129 steps, 0 differ |
| `run_compare.py` | 63 steps, 0 differ |
| `dashboard_compare.py` (new) | 136 steps, 0 differences |

The corpus numbers differ from the summary table above (13,419 agree, 3
divergences): the pre-session code already gives 13,418 in this container;
the fourth divergence, `cd ~ && ./script arg` (Python allow, Rust ask), is
safe and depends on the parity environment. `rust/parity/report.md` was left
as committed. `cargo test`: 422 tests (395 unit, 19 CLI, 8 approval
end-to-end); clippy and fmt clean. `just check`/`just test`: everything
passes except `tests/test_config.py::TestLoadConfig::test_unreadable_user_config`,
which fails only as root (as before); it was run as an unprivileged user
and passes.

### New dependencies

- `rustls` 0.23 (features `ring`, `std`, `tls12`; no default features) and
  `rustls-native-certs` 0.8: TLS for `https` node URLs and `https`
  `dashboard-url`, matching Python's `HTTPSHandler` (certificate
  verification against the system store, honouring `SSL_CERT_FILE`). Both
  are maintained by the rustls project; `ring` needs only a C compiler (no
  cmake, unlike the default `aws-lc-rs`). They add `ring`, `untrusted`,
  `rustls-webpki`, `rustls-pki-types`, `subtle`, `zeroize` and
  `openssl-probe`.
- No HTTP framework and no async runtime: the server is std `TcpListener`
  with one thread per request (`http.rs`, about 700 lines with tests, including a small
  client and Python's `urlsplit`/`parse_qs`). A client library (`ureq`) was
  not used because the approval client must tell "never sent" (connection
  refused, connect timeout) from "maybe received" apart, which needs control
  over each phase.

### Intentional divergences (dashboard)

- `--port` outside 0-65535 is reported by the listener ("port must be
  0-65535") instead of Python's `OverflowError` text; `--host` accepts IPv6
  and names (Python's server is IPv4 only).
- Unsupported methods answer `501` with a JSON error (Python sends its HTML
  error page); `POST` is a real method on the Rust node (approval routes).
- Proxy and registry errors carry Rust's wording after the fixed prefix
  (`node unreachable: ...`, the JSON parser message after the file name);
  the parity script normalizes these.
- An `https` node is verified with rustls and the system roots; TLS
  failures are `node unreachable: TLS handshake failed: ...`.
- The UI is a new page (approvals view first); the legacy API is unchanged.
- `docs/dashboard-api.json` marks the queue operations `x-dippy-rs-only`;
  the Python contract test skips them (test-only change, Python Dippy
  unchanged).

### Next step

The switch: install the Rust binary as `dippy` (`just install-rs`), remove
the Python package. Python Dippy is no longer needed for anything: the
dashboard was the last part. Before relying on `set approval dashboard`,
raise each agent's hook timeout above `askpass-timeout` (table in
`docs/config.md`) and run the node as a user service.

## Phase 1 - Rable in Python Dippy

### Result: Rable's Python package is not a drop-in replacement for Dippy

Rable 0.2.1 publishes only `cp313` wheels, so it was built from source
(`maturin`, Rust 1.97) for Python 3.12. Its Python API is
`parse(source, extglob=False) -> list[ParsedNode]`, `ParseError` and
`MatchedPairError`, but `ParsedNode` exposes only `to_sexp()`
(`src/python.rs`). Dippy reads Parable's Python node attributes
(`kind`, `words`, `parts`, `redirects`, `op`, `target`, `content`, ...)
everywhere in `core/analyzer.py` and `core/parser.py`.

The scratch run with both import sites switched to `rable` failed across the
suite (the first failures were ordinary `tests/test_dippy.py::test_command`
cases such as `aws s3api head-bucket --bucket mybucket`): every analysis ends
in an `AttributeError` on `ParsedNode`, not in a parse difference. The
baseline suite on vendored Parable has one failure that is unrelated and
environmental: `tests/test_config.py::TestLoadConfig::test_unreadable_user_config`
expects `chmod 000` to block reads, which does not hold when the suite runs as
root (as in this cloud container).

Because the suite is nowhere near green with Rable, the opt-in
`DIPPY_PARSER=rable` switch was **not** added; Python behaviour is unchanged.
Making Rable usable from Python would need attribute-level bindings in Rable
itself (or a Python adapter that rebuilds Parable nodes from the S-expression),
which is out of scope here: the Rust port uses Rable's Rust AST directly.

### Parser comparison through the Rust adapter

Instead of the Python binding, the Rust port compares trees directly:
`rust/parity/ast_dump.py` dumps the Parable attributes the analyzer reads, and
`dippy-rs --dump-ast-jsonl` dumps the same Parable-shaped tree rebuilt from
Rable's Rust AST (`rust/dippy-rs/src/ast.rs`). `rust/parity/ast_compare.py`
compares both over every corpus command and writes
`rust/parity/ast-report.md`.

Structural differences between Rable's Rust AST and Parable's Python objects
that the adapter has to undo (none of them are bugs; Rable mirrors Parable's
S-expressions, not its object model):

| Area | Parable (Python objects) | Rable (Rust AST) | Adapter |
| --- | --- | --- | --- |
| Redirect | textual: `2>&1` is op `2>`, target `&1`; `3<&-` is op `3<`, target `&-` | normalised: op `>&`, `fd: 2`, target `1`; `2>&1-` loses the `-` | rebuild op/target from the source span plus the fd/`{var}` prefix |
| Lists | flat `parts` with operator nodes; trailing `;`/`&` kept in top-level, `{ }` and `( )` bodies | nested by precedence; trailing separators dropped | flatten, re-add trailing operators from source |
| Newlines at top level | separate top-level nodes | one list joined by `;` | split top-level lists at newlines |
| Word parts | expansion nodes only | also `WordLiteral` and `BraceExpansion` segments | drop literal segments |
| Assignments | in `words` | separate `assignments` field | prepend to words |
| `[[ ]]` operands | `Word` with parts | `CondTerm` text only | re-parse the term as a word |
| Substitution bodies | own node trees | spans relative to the substitution | re-parse bodies from the word text |
| `a \|& b` | `pipe-both` marker in `commands` | synthetic `2>&1` redirect on `a` | marker from separators, drop span-less redirect |
| Comment-only input | one `empty` node | no nodes | add `empty` |
| `$[expr]` | `arith-deprecated` part | literal | detect in word scan |

### Rable bugs found (parser risk for the Rust port)

1. **Backtick substitution inside double quotes is not a word part.**
   `echo "`rm -rf x`"`: Parable reports a `cmdsub` part, Rable only a literal.
   An analyzer trusting Rable's parts would never inspect `rm`. Adapter: scan
   every raw word for `$(...)`, backticks, `<(...)`/`>(...)` and `$((...))`
   itself (`src/scan.rs`) and re-parse each body.
2. **`$'` inside double quotes starts ANSI-C quoting.** `mysql -e "SELECT 'a$'"`
   parses as `mysql -e`; the quoted argument (and anything after it on the
   line) disappears. In Bash and Parable `$'` inside double quotes is literal.
   Adapter: replace that quote with a same-width placeholder before parsing
   and restore it in every string.
3. **Silent error recovery.** Rable accepts input Bash and Parable reject and
   drops text: `ls ;; rm x` parses as `ls`; `echo "unterminated` as `echo`;
   `(SELECT 1) UNION ALL (SELECT 2)` as three commands; `3>`/`<>` as commands
   with an empty target; `; SELECT 2` and `ls && && rm` contain empty
   commands. Adapter: every top-level character must be covered by a node
   span, a separator, a comment or a here-document body, consecutive nodes
   need a separator, and empty commands become unsupported (ask).
4. **Inaccurate spans.** Function definitions and `for` loops start after the
   first token; `(( ... ))` ends after `((`; command and process substitution
   nodes have empty spans. The coverage check corrects for these.
5. **Arithmetic subscripts.** `(( arr[$(rm -rf /)] ))` has no command
   substitution node in Rable's arithmetic tree. Adapter: scan the raw
   arithmetic text instead.

### Python Dippy issues found while building the adapter (not changed)

- `echo $(( $(rm -rf x) ))` and `x=$((1+$(rm z)))` are **allowed**: the
  analyzer ignores `arith` word parts, so command substitutions inside
  arithmetic expansion are never analysed. Same for `a=(1 2 $(rm x))`
  (`array` parts). The Rust port asks for these (safe divergence).
- `a |& b` always asks with "unrecognized construct: pipe-both".

### Remaining tree differences

See `rust/parity/ast-report.md`. All remaining differences end in `ask` on the
Rust side: Parable-rejected input that Rable accepts with empty targets or
empty commands, `$(case ...)` bodies the substitution scanner cannot balance,
and a bare `!`.

## Phase 2 - Parity harness

All tools live in `rust/parity/`:

| File | Purpose |
| --- | --- |
| `collect_plugin.py` | pytest plugin: harvests every string test parameter and every top-level `analyze()` call with a default or `parse_config` config |
| `handwritten.jsonl` | 721 config cases covering every directive in `docs/config.md` |
| `parser_cases.jsonl` | 164 parser stress cases (quoting, substitutions, redirects, heredocs, compound commands, malformed input) |
| `gen_fuzz.py` / `fuzz_cases.jsonl` | 3,578 corpus commands placed in 40 shell contexts (wrappers, pipelines, lists, substitutions, redirects, `sudo`, `ssh`, ...) |
| `build_corpus.py` + `oracle.py` | harvest with `HOME` set to the parity home, dedupe, drop configs that run programs or write files and cases naming pytest `tmp_path` files, run Python `cli_mode` in process with an empty `HOME`, write `corpus.jsonl` |
| `run` | run `dippy-rs` per case, write `report.md`; exit 1 on any unsafe divergence |
| `ast_compare.py` / `ast_dump.py` | Parable vs adapter tree comparison, `ast-report.md` |
| `handler_compare.py` | identical `HandlerContext` fed to Python and Rust handlers |
| `hook_compare.py` | `dippy` vs `dippy-rs` hook mode on the same payloads in every agent mode (fake askpass) |
| `log_compare.py` | the same payloads with logging on in a separate HOME per run: audit log, `hook-approvals.log`, `audit` queries and rotation |
| `config_compare.py` | `dippy config` scenarios in one sandbox per implementation: output of every step, resulting files and their modes |
| `hooks_compare.py` | `dippy hooks` scenarios in a sandbox HOME and project per implementation: output of every step, the whole resulting tree (backups included) and modes |
| `doctor_compare.py` | `dippy doctor` scenarios in a sandbox HOME, project and PATH (fake `dippy`) per implementation: output of every step and the resulting tree |
| `dashboard_compare.py` | Python and Rust dashboard node and hub on port 0 in sandbox HOMEs: the legacy REST API (`/api/entries`, `/api/nodes`, `/n/{name}/api/entries`, tokens, errors), status and body after normalizing ports and platform error texts |
| `run_compare.py` | `dippy run`/`run-on-server`/`recover` scenarios in a sandbox HOME, project and PATH (fake `ssh`, `tmux`, `herdr`, askpass) per implementation: output of every step, run state, audit log and pane logs |

The in-process oracle was checked against the real `dippy` executable on 100
cases (0 differences). All runs share fixed paths under `/tmp/dippy-parity`
(`home`, `work`, `configs`) so decisions that depend on paths are comparable.

## Phase 3 - Rust core

Crate `rust/dippy-rs` (binary `dippy-rs`, library `dippy_rs`):

- `ast.rs`, `scan.rs`: Parable-shaped tree from Rable (see phase 1).
- `parser.rs`, `bash.rs`, `paths.rs`, `fnmatch.rs`: `tokenize`, quote
  handling, `os.path`/`pathlib` semantics, Python `fnmatch`.
- `analyzer.rs`: line-by-line port of `core/analyzer.py`.
- `config.rs`: config language (`parse_config`, every directive, settings
  validation with fatal SSH-profile errors), scopes (user, project `.dippy`,
  `--config`/`DIPPY_CONFIG`, `DIPPY_CONFIG_ONLY`, `set final`), includes
  with globbing and cycle detection, command/redirect/option-block/`after`
  matching, context flags, last-match-wins.
- `allowlists.rs`: generated at build time from `core/allowlists.py`
  (`build.rs`), so the lists cannot drift.
- `hook.rs`: hook mode for Claude Code (also `--pi`, `--moltbot`,
  `--windsurf`, `--pearai`), `--gemini`, `--codex`, `--cursor` and
  `--agy`/`--antigravity`, by flag, `DIPPY_<AGENT>` variable or payload
  shape; AGY `ask` runs the askpass program (`DIPPY_ASKPASS` or
  `set askpass`, `set askpass-timeout`).
- `logging.rs`: the audit log (`set log`, `log-full`; Python key order and
  `isoformat()` timestamps) and the per-agent `hook-approvals.log`
  (`log-hook-approvals`), including config warnings; daily rotation
  (`log-rotate-max-days`) runs on every config load in `config.rs`.
- `audit.rs`: `dippy-rs [--cwd DIR] [--config FILE|--config-only FILE]
  audit` with every Python filter, grouping and limit (dates
  `YYYY-MM-DD` only).
- `admin.rs`: `config get/set/unset [--user|--project]` and `config server
  add/remove/list`; edits one line, keeps every other line and the file
  mode, writes atomically (a new file gets 0600). `config.rs` keeps the
  `approval-wait-message` and `run-on-server-*` values for it.
- `dashboard.rs`, `http.rs`: `dashboard` node and hub (std server, rustls
  client); `queue.rs`: the node's approval queue; `approval.rs`: the
  version-2 payload and the hook/`run` client for `set approval dashboard`.
- `hooks.rs`: `hooks list/install/uninstall/setup-gemini-yolo` for Claude,
  Gemini, AGY, Cursor, Windsurf and Codex (`config.toml` included), with a
  port of difflib's unified diff for `--dry-run`.
- CLI (`main.rs`, clap): `dippy-rs --cmd CMD|--stdin [--json] [--cwd DIR]
  [--config FILE|--config-only FILE] [--agent NAME] [--remote]`, same output
  (Python `json.dumps` formatting) and exit codes as `cli_mode`; `--help`,
  `--version`; hidden hook mode flags. Without `--cmd`, `--stdin` or a
  subcommand it runs as a hook, as Python does.

Not ported (fail closed or out of scope): notifier programs (never run),
execution subcommands.

## Phase 4 - Handlers

All 89 handler modules are ported by seven parallel agents in separate
worktrees and merged; each is verified with `handler_compare.py` on
identical inputs. Per-module extra cases are in `rust/parity/cases/`.

- `python`: static analysis uses `rustpython-ruff_python_parser` 0.16.10
  (Python 3.12 target). First-violation agreement was checked on about
  54,000 Python sources; Rust asks for syntax CPython 3.12 rejects and for
  nesting deeper than CPython's recursion limit allows.
- SQL (`core/sql.py`, `psql`, `mysql`, `sqlite3`, `duckdb`, `sqlcmd`,
  Athena in `aws`): lexical masking, statement splitting and DuckDB write
  checks are exact ports. SQLGlot's structural verification is replaced by a
  conservative query grammar that only proves read-only for constructs whose
  SQLGlot verdict was reproduced; differential fuzzing (21 seeds, ~21k cases
  each, `sql_fuzz.py`) found 0 unsafe results on the final code.
- Python's `str.isdigit`, `\s` and `str.isspace` Unicode semantics are
  reproduced where handlers rely on them.
