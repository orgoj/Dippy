# Porting Python Dippy to `dippy-rs`

The Python code in `src/dippy` is the specification. Port behaviour, not
style: same branches, same order of checks, same edge cases. When Python and
this guide disagree, Python wins - except for the safety rule below.

## Safety rule

Fail closed. If the Rust code cannot reproduce a Python decision exactly
(an unported helper, a Python library with no Rust equivalent, an unclear
regex), it must return `ask`, never `allow`. A Rust `allow` where Python asks
or denies is a safety bug; a Rust `ask` where Python allows is an accepted,
documented divergence. Never execute commands under test.

## Handlers (`src/cli/*.rs`)

Each file ports one `src/dippy/cli/<module>.py` and exports:

```rust
pub const COMMANDS: &[&str] = &[...];          // same list as Python
pub const PORTED: bool = true;                  // only when complete
pub const DESCRIPTION: Option<Describe> = Some(get_description); // if Python defines get_description
pub fn classify(ctx: &HandlerContext) -> Classification { ... }
```

`HandlerContext` mirrors Python: `tokens` (quote-stripped words starting with
the command), `remote`, `cwd`, `config: Option<&Config>`,
`word_has_expansions` and `raw_words` (may be empty, like Python's `()`).
`Classification` has constructors `allow()`, `ask()`, `delegate(inner)`,
`allow_desc(d)`, `ask_desc(d)` and builders `.desc()`, `.redirects()`,
`.wrapper()`, `.remote()`; fields are public.

Shared helpers already ported:

- `crate::bash::{decode_literal_word, bash_quote, bash_join}`
- `crate::parser::{tokenize, strip_quotes}` (Rable-backed `tokenize`)
- `crate::paths::{resolve_arg_path, expanduser, normpath, resolve, pathlib_str}`
- `crate::cli::{get_handler, get_description}`

Python semantics to keep in mind:

- `str.startswith(tuple)`, `in` on strings (substring), slicing past the end
  (no panic in Python: use `.get()`), negative indices.
- `str.isdigit()`/`isalnum()`/`isalpha()` are Unicode-aware; `isdigit()` is
  false for `""`.
- `str.split()` without arguments splits on runs of whitespace and drops
  empty strings; `split(" ")` does not.
- `re.match` anchors at the start only; `re.fullmatch` at both ends;
  `re.search` anywhere. The `regex` crate has no look-around or
  backreferences: rewrite such patterns by hand and test them.
  Use `std::sync::LazyLock<regex::Regex>` for module-level patterns.
- Python `frozenset`/`set` lookups become `matches!`/`contains` on slices.
- Iteration order of Python dicts is insertion order.

## Tests

Port the handler's unit tests from `tests/cli/test_<module>.py` into a
`#[cfg(test)] mod tests` in the same file. Tests that go through the whole
pipeline (`check(cmd)`) are covered by the parity corpus; port those that
call the handler directly, and add Rust tests for any tricky branch.
Take expected values of Python-compatibility unit tests from running the
Python implementation, never derive them by hand.

## Verification

```bash
cd rust && cargo build --release && cargo test
/home/user/Dippy/.venv-3.12/bin/python rust/parity/handler_compare.py --module git
```

`handler_compare.py` feeds identical tokens to the Python and Rust handlers
for every corpus command (and `--cases FILE` extra commands, one per line)
and prints differences, unsafe ones first. A module is done when it reports
`diff=0` (description differences are allowed but should be rare) and its
tests pass; then set `PORTED = true`. Unported handlers are never compared.
