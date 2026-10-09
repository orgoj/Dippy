# Rust port report

Status report for the unattended Rust port described in
[`rust-port.md`](rust-port.md). Updated at every phase.

## Summary

<!-- STATUS -->

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
