//! Port of `src/dippy/cli/python.py`.
//!
//! Analyzes Python scripts to determine if they're safe (read-only, no I/O,
//! no code execution). Uses AST-based static analysis with a whitelist
//! approach. Conservative by design: if we can't prove it's safe, ask.
//!
//! Python's `ast.parse` is replaced by `ruff_python_parser` (crate
//! `rustpython-ruff_python_parser`) targeting Python 3.12, the oracle's
//! version. Any parse error or syntax unsupported in 3.12 is a `syntax`
//! violation (ask). The visitor walks nodes in CPython `_fields` order so
//! that the first violation (used in descriptions) matches Python.
//!
//! Known divergences (Rust only ever asks more):
//! - Syntax-error messages differ from CPython's (description only).
//! - Source the parser accepts but CPython 3.12's `ast.parse` rejects is
//!   rejected explicitly: a leading BOM, f-string format specs nested three
//!   deep or containing `=` (a ValueError in CPython), 150 nested f-strings,
//!   type parameter defaults, and `try`/`else` without `except`.
//! - `ast.NodeVisitor` raises `RecursionError` on deeply nested code; Rust
//!   emulates its stack use and asks a margin earlier (`RECURSION_FRAMES`).
//! - The parser's nesting limits are slightly stricter than CPython's
//!   (about 198 instead of 200 nested brackets).
//! - Code that CPython only rejects at compile time (e.g. `return` outside a
//!   function) is accepted by `ast.parse` and by this port alike.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use ruff_python_ast::{
    self as ast, ExceptHandler, Expr, FStringPartRef, InterpolatedStringElement, Parameters,
    Pattern, PythonVersion, Stmt, TypeParam, TypeParams,
};
use ruff_python_parser::{Mode, ParseOptions, parse};
use ruff_text_size::{Ranged, TextRange};

use super::{Classification, Describe, HandlerContext};
use crate::paths::{pathlib_str, resolve_arg_path};

pub const COMMANDS: &[&str] = &[
    "python",
    "python3",
    "python3.8",
    "python3.9",
    "python3.10",
    "python3.11",
    "python3.12",
    "python3.13",
    "python3.14",
    "python3.15",
    "python3.16",
    "python3.17",
    "python3.18",
    "python3.19",
];
pub const PORTED: bool = true;
pub const DESCRIPTION: Option<Describe> = Some(get_description);

// === Safe Module Whitelist ===
// Only modules that cannot perform I/O, execute code, or mutate external
// state. When in doubt, leave it out.
const SAFE_MODULES: &[&str] = &[
    // Core data structures
    "collections",
    "collections.abc",
    "dataclasses",
    "typing",
    "typing_extensions",
    "types",
    "enum",
    "array",
    // Math and algorithms
    "math",
    "cmath",
    "statistics",
    "decimal",
    "fractions",
    "random",
    "itertools",
    "functools",
    "operator",
    "bisect",
    "heapq",
    "graphlib",
    // Text processing
    "re",
    "string",
    "textwrap",
    "difflib",
    "unicodedata",
    // Data format parsing (in-memory only)
    "json",
    "csv",
    "tomllib",
    // Hashing and encoding (pure computation)
    "hashlib",
    "hmac",
    "base64",
    "binascii",
    "quopri",
    "uu",
    // Compression (in-memory only via compress/decompress)
    "zlib",
    // Date and time
    "datetime",
    "time",
    "calendar",
    "zoneinfo",
    // Introspection (AST only, not source reading)
    "ast",
    "dis",
    "tokenize",
    "token",
    "keyword",
    "symtable",
    // Other safe utilities
    "__future__",
    "copy",
    "pprint",
    "reprlib",
    "abc",
    "numbers",
    "contextlib",
    "warnings",
    "traceback",
    // Parsing
    "struct",
    // HTML (parsing only - html.parser)
    "html",
    "html.parser",
    "html.entities",
];

// Modules that are NEVER safe (Bandit blacklists, RestrictedPython, CVEs).
const DANGEROUS_MODULES: &[&str] = &[
    // Code execution
    "subprocess",
    "os",
    "sys",
    "shutil",
    "runpy",
    "compileall",
    "py_compile",
    "importlib",
    "pkgutil",
    "popen2",
    "commands",
    // File I/O
    "pathlib",
    "io",
    "fileinput",
    "tempfile",
    "glob",
    "fnmatch",
    "codecs",
    "linecache",
    "inspect",
    "configparser",
    "gzip",
    "bz2",
    "lzma",
    "tarfile",
    "zipfile",
    // Network
    "socket",
    "ssl",
    "http",
    "http.client",
    "http.server",
    "urllib",
    "urllib.request",
    "urllib.parse",
    "ftplib",
    "smtplib",
    "poplib",
    "imaplib",
    "nntplib",
    "telnetlib",
    "socketserver",
    "xmlrpc",
    "ipaddress",
    // XML parsing
    "xml",
    "xml.etree",
    "xml.etree.ElementTree",
    "xml.etree.cElementTree",
    "xml.sax",
    "xml.dom",
    "xml.dom.minidom",
    "xml.dom.pulldom",
    "xml.dom.expatbuilder",
    "xml.parsers",
    "xml.parsers.expat",
    // Process/threading
    "multiprocessing",
    "threading",
    "concurrent",
    "concurrent.futures",
    "asyncio",
    "signal",
    "mmap",
    // System interaction
    "ctypes",
    "platform",
    "sysconfig",
    "resource",
    "pty",
    "tty",
    "termios",
    "fcntl",
    "grp",
    "pwd",
    "spwd",
    "crypt",
    // Deserialization
    "pickle",
    "cPickle",
    "dill",
    "shelve",
    "marshal",
    "jsonpickle",
    // Databases
    "dbm",
    "sqlite3",
    // Code manipulation
    "code",
    "codeop",
    "gc",
    // Other dangerous
    "webbrowser",
    "cmd",
    "shlex",
    "getpass",
    "getopt",
    "argparse",
    "logging",
    "atexit",
    "cgi",
    "cgitb",
    "wsgiref.handlers",
];

// Builtins that are never safe. (Python's SAFE_BUILTINS whitelist only
// feeds no-op branches, so it is not ported.)
const DANGEROUS_BUILTINS: &[&str] = &[
    "eval",
    "exec",
    "compile",
    "__import__",
    "open",
    "input",
    "print",
    "globals",
    "locals",
    "vars",
    "dir",
    "setattr",
    "delattr",
    "getattr",
    "memoryview",
    "breakpoint",
];

// Attributes/methods that indicate dangerous operations.
const DANGEROUS_ATTRS: &[&str] = &[
    // File operations
    "write",
    "writelines",
    "truncate",
    "flush",
    "close",
    "read",
    "readline",
    "readlines",
    "read_text",
    "read_bytes",
    "write_text",
    "write_bytes",
    "open",
    // OS/Process operations
    "remove",
    "unlink",
    "rmdir",
    "rmtree",
    "mkdir",
    "makedirs",
    "rename",
    "replace",
    "chmod",
    "chown",
    "chroot",
    "link",
    "symlink",
    "system",
    "popen",
    "popen2",
    "popen3",
    "popen4",
    "spawn",
    "spawnl",
    "spawnle",
    "spawnlp",
    "spawnlpe",
    "spawnv",
    "spawnve",
    "spawnvp",
    "spawnvpe",
    "startfile",
    "fork",
    "forkpty",
    "exec",
    "execl",
    "execle",
    "execlp",
    "execlpe",
    "execv",
    "execve",
    "execvp",
    "execvpe",
    "kill",
    "killpg",
    "terminate",
    "wait",
    "waitpid",
    "wait3",
    "wait4",
    // Subprocess
    "call",
    "check_call",
    "check_output",
    "run",
    "Popen",
    "getoutput",
    "getstatusoutput",
    // Network
    "connect",
    "bind",
    "listen",
    "accept",
    "send",
    "sendall",
    "sendto",
    "sendmsg",
    "recv",
    "recvfrom",
    "recvmsg",
    "request",
    "urlopen",
    "urlretrieve",
    // Deserialization
    "Unpickler",
    // Reflection escape hatches
    "__dict__",
    "__class__",
    "__bases__",
    "__mro__",
    "__subclasses__",
    "__globals__",
    "__code__",
    "__closure__",
    "__reduce__",
    "__reduce_ex__",
    "__getstate__",
    "__setstate__",
    "tb_frame",
    "tb_next",
    "f_back",
    "f_builtins",
    "f_code",
    "f_globals",
    "f_locals",
    "f_trace",
    "co_code",
    "gi_frame",
    "gi_code",
    "gi_yieldfrom",
    "cr_await",
    "cr_frame",
    "cr_code",
    // Module manipulation
    "__import__",
    "__loader__",
    "__spec__",
    "__builtins__",
];

// Reflection attributes that are dangerous even on access (not just call).
const REFLECTION_ATTRS: &[&str] = &[
    "__globals__",
    "__code__",
    "__closure__",
    "__dict__",
    "__class__",
    "__bases__",
    "__mro__",
    "__subclasses__",
    "__reduce__",
    "__reduce_ex__",
    "__builtins__",
    "__getattribute__",
    "tb_frame",
    "tb_next",
    "f_back",
    "f_builtins",
    "f_code",
    "f_globals",
    "f_locals",
    "f_trace",
    "co_code",
    "gi_frame",
    "gi_code",
    "gi_yieldfrom",
    "cr_await",
    "cr_frame",
    "cr_code",
];

/// Python frames `ast.NodeVisitor` may use before `analyze_python_source`
/// raises `RecursionError` (limit 1000, minus a margin for the callers'
/// frames). Deeper code asks.
const RECURSION_FRAMES: usize = 800;
/// Frames of a leaf child (operator or context node) below any node.
const LEAF_FRAMES: usize = 2;

/// CPython's tokenizer `MAXFSTRINGLEVEL`: this many nested f-strings fail.
const MAX_FSTRING_LEVEL: usize = 150;

/// A safety violation found during analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub kind: &'static str,
    pub detail: String,
}

/// Group validated `module.symbol` config values by module.
pub fn build_allowed_symbols(symbols: &[String]) -> HashMap<String, HashSet<String>> {
    let mut grouped: HashMap<String, HashSet<String>> = HashMap::new();
    for symbol in symbols {
        let Some((module, name)) = symbol.rsplit_once('.') else {
            continue;
        };
        if module.is_empty() || name.is_empty() {
            continue;
        }
        grouped
            .entry(module.to_string())
            .or_default()
            .insert(name.to_string());
    }
    grouped
}

/// Configurable module and symbol lists.
#[derive(Debug, Clone, Default)]
pub struct AnalysisConfig {
    pub extra_safe_modules: HashSet<String>,
    pub extra_deny_modules: HashSet<String>,
    pub allowed_symbols: HashMap<String, HashSet<String>>,
}

impl AnalysisConfig {
    fn is_safe_module(&self, m: &str) -> bool {
        SAFE_MODULES.contains(&m) || self.extra_safe_modules.contains(m)
    }
    /// `(DANGEROUS_MODULES | extra_deny) - extra_safe`.
    fn is_deny_module(&self, m: &str) -> bool {
        (DANGEROUS_MODULES.contains(&m) || self.extra_deny_modules.contains(m))
            && !self.extra_safe_modules.contains(m)
    }
    /// `extra_deny - extra_safe`.
    fn is_explicit_deny_module(&self, m: &str) -> bool {
        self.extra_deny_modules.contains(m) && !self.extra_safe_modules.contains(m)
    }
}

/// Offsets of line starts (`\n`, `\r\n` and `\r` end a line, as in CPython).
fn line_starts(source: &str) -> Vec<usize> {
    let bytes = source.as_bytes();
    let mut starts = vec![0];
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => starts.push(i + 1),
            b'\r' => {
                if bytes.get(i + 1) == Some(&b'\n') {
                    i += 1;
                }
                starts.push(i + 1);
            }
            _ => {}
        }
        i += 1;
    }
    starts
}

/// AST visitor that checks Python code for safety (`SafetyAnalyzer`).
///
/// Children are visited in CPython `_fields` order (`ast.NodeVisitor`).
struct SafetyAnalyzer<'c> {
    violations: Vec<Violation>,
    allow_print: bool,
    config: &'c AnalysisConfig,
    lines: Vec<usize>,
    /// First error CPython's `ast.parse` raises that the parser accepts.
    syntax_error: Option<Violation>,
    /// Emulated Python stack depth of the visitor (2 frames per node,
    /// 3 for node types with a `visit_*` method that calls `generic_visit`).
    frames: usize,
    /// Open f-strings (CPython's tokenizer allows at most 149 nested).
    fstring_level: usize,
}

impl SafetyAnalyzer<'_> {
    fn line_of(&self, range: TextRange) -> usize {
        let offset = range.start().to_usize();
        self.lines.partition_point(|&s| s <= offset)
    }

    fn add(&mut self, range: TextRange, kind: &'static str, detail: String) {
        let line = self.line_of(range);
        self.violations.push(Violation { line, kind, detail });
    }

    /// Record source that `ast.parse` rejects (SyntaxError or ValueError)
    /// although `ruff_python_parser` accepts it.
    fn reject(&mut self, range: TextRange, detail: &str) {
        if self.syntax_error.is_none() {
            let line = self.line_of(range);
            self.syntax_error = Some(Violation {
                line,
                kind: "syntax",
                detail: format!("{detail} (<unknown>, line {line})"),
            });
        }
    }

    /// Visit a node costing `cost` Python frames; past the recursion limit
    /// Python raises `RecursionError`, so the analysis must fail.
    fn nested(&mut self, range: TextRange, cost: usize, visit: impl FnOnce(&mut Self)) {
        self.frames += cost;
        if self.syntax_error.is_none() {
            if self.frames + LEAF_FRAMES > RECURSION_FRAMES {
                self.reject(range, "maximum recursion depth exceeded");
            } else {
                visit(self);
            }
        }
        self.frames -= cost;
    }

    fn stmt(&mut self, stmt: &Stmt) {
        let cost = match stmt {
            Stmt::FunctionDef(_) | Stmt::Import(_) | Stmt::ImportFrom(_) | Stmt::Global(_) => 3,
            Stmt::With(w) if !w.is_async => 3,
            Stmt::Try(t) if !t.is_star => 3,
            _ => 2,
        };
        self.nested(stmt.range(), cost, |s| s.stmt_inner(stmt));
    }

    fn expr(&mut self, expr: &Expr) {
        let cost = match expr {
            Expr::Call(_)
            | Expr::Attribute(_)
            | Expr::Name(_)
            | Expr::Starred(_)
            | Expr::Await(_) => 3,
            _ => 2,
        };
        self.nested(expr.range(), cost, |s| s.expr_inner(expr));
    }

    fn pattern(&mut self, pattern: &Pattern) {
        self.nested(pattern.range(), 2, |s| s.pattern_inner(pattern));
    }

    fn body(&mut self, body: &[Stmt]) {
        for stmt in body {
            self.stmt(stmt);
        }
    }

    fn exprs(&mut self, exprs: &[Expr]) {
        for expr in exprs {
            self.expr(expr);
        }
    }

    fn opt_expr(&mut self, expr: Option<&Expr>) {
        if let Some(expr) = expr {
            self.expr(expr);
        }
    }

    fn stmt_inner(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::FunctionDef(f) => {
                if f.is_async {
                    // CPython's lineno is the `async def` line, not a decorator's.
                    let range = if f.decorator_list.is_empty() {
                        f.range()
                    } else {
                        f.name.range()
                    };
                    self.add(range, "async", "async functions require asyncio".into());
                }
                // name, args, body, decorator_list, returns, type_params
                self.nested(f.parameters.range(), 2, |s| s.parameters(&f.parameters));
                self.body(&f.body);
                for decorator in &f.decorator_list {
                    self.expr(&decorator.expression);
                }
                self.opt_expr(f.returns.as_deref());
                if let Some(tp) = &f.type_params {
                    self.type_params(tp);
                }
            }
            Stmt::ClassDef(c) => {
                // name, bases, keywords, body, decorator_list, type_params
                if let Some(arguments) = &c.arguments {
                    self.arguments(arguments);
                }
                self.body(&c.body);
                for decorator in &c.decorator_list {
                    self.expr(&decorator.expression);
                }
                if let Some(tp) = &c.type_params {
                    self.type_params(tp);
                }
            }
            Stmt::Return(r) => self.opt_expr(r.value.as_deref()),
            Stmt::Delete(d) => self.exprs(&d.targets),
            Stmt::TypeAlias(t) => {
                self.expr(&t.name);
                if let Some(tp) = &t.type_params {
                    self.type_params(tp);
                }
                self.expr(&t.value);
            }
            Stmt::Assign(a) => {
                self.exprs(&a.targets);
                self.expr(&a.value);
            }
            Stmt::AugAssign(a) => {
                self.expr(&a.target);
                self.expr(&a.value);
            }
            Stmt::AnnAssign(a) => {
                self.expr(&a.target);
                self.expr(&a.annotation);
                self.opt_expr(a.value.as_deref());
            }
            Stmt::For(f) => {
                self.expr(&f.target);
                self.expr(&f.iter);
                self.body(&f.body);
                self.body(&f.orelse);
            }
            Stmt::While(w) => {
                self.expr(&w.test);
                self.body(&w.body);
                self.body(&w.orelse);
            }
            Stmt::If(i) => {
                self.expr(&i.test);
                self.body(&i.body);
                // CPython nests each `elif` as an `If` in the previous orelse.
                let mut extra = 0;
                for clause in &i.elif_else_clauses {
                    if clause.test.is_some() {
                        extra += 2;
                        self.frames += 2;
                    }
                    self.nested(clause.range(), 0, |s| {
                        s.opt_expr(clause.test.as_ref());
                        s.body(&clause.body);
                    });
                }
                self.frames -= extra;
            }
            Stmt::With(w) => {
                if !w.is_async {
                    // visit_With: only `ast.With`, not `ast.AsyncWith`.
                    for item in &w.items {
                        if let Expr::Call(call) = &item.context_expr
                            && let Expr::Name(name) = call.func.as_ref()
                            && name.id.as_str() == "open"
                        {
                            self.add(w.range(), "io", "file open in with statement".into());
                        }
                    }
                }
                for item in &w.items {
                    self.nested(item.range(), 2, |s| {
                        s.expr(&item.context_expr);
                        s.opt_expr(item.optional_vars.as_deref());
                    });
                }
                self.body(&w.body);
            }
            Stmt::Match(m) => {
                self.expr(&m.subject);
                for case in &m.cases {
                    self.nested(case.range(), 2, |s| {
                        s.pattern(&case.pattern);
                        s.opt_expr(case.guard.as_deref());
                        s.body(&case.body);
                    });
                }
            }
            Stmt::Raise(r) => {
                self.opt_expr(r.exc.as_deref());
                self.opt_expr(r.cause.as_deref());
            }
            Stmt::Try(t) => {
                if t.handlers.is_empty() && (t.finalbody.is_empty() || !t.orelse.is_empty()) {
                    self.reject(t.range(), "expected 'except' or 'finally' block");
                }
                self.body(&t.body);
                for handler in &t.handlers {
                    let ExceptHandler::ExceptHandler(h) = handler;
                    self.nested(h.range(), 2, |s| {
                        s.opt_expr(h.type_.as_deref());
                        s.body(&h.body);
                    });
                }
                self.body(&t.orelse);
                self.body(&t.finalbody);
            }
            Stmt::Assert(a) => {
                self.expr(&a.test);
                self.opt_expr(a.msg.as_deref());
            }
            Stmt::Import(i) => {
                for alias in &i.names {
                    let module = alias.name.as_str();
                    let root = module.split('.').next().unwrap_or(module);
                    if self.config.is_deny_module(module) || self.config.is_deny_module(root) {
                        self.add(i.range(), "import", format!("dangerous module: {module}"));
                    } else if !self.config.is_safe_module(module)
                        && !self.config.is_safe_module(root)
                    {
                        self.add(i.range(), "import", format!("unknown module: {module}"));
                    }
                }
            }
            Stmt::ImportFrom(i) => self.import_from(i),
            // visit_Global does not recurse; Nonlocal has no child nodes.
            Stmt::Global(_) | Stmt::Nonlocal(_) => {}
            Stmt::Expr(e) => self.expr(&e.value),
            Stmt::Pass(_) | Stmt::Break(_) | Stmt::Continue(_) => {}
            // Not produced in module mode; never reached with valid syntax.
            Stmt::IpyEscapeCommand(e) => {
                self.add(e.range(), "syntax", "unsupported syntax".into());
            }
        }
    }

    fn import_from(&mut self, node: &ast::StmtImportFrom) {
        // `from .json import loads` reads a local file, not the stdlib module.
        if node.level != 0 {
            self.add(node.range(), "import", "relative import not allowed".into());
            return;
        }
        let Some(module) = &node.module else {
            self.add(node.range(), "import", "import without module".into());
            return;
        };
        let module = module.as_str();
        let root = module.split('.').next().unwrap_or(module);
        let cfg = self.config;
        if cfg.is_explicit_deny_module(module) || cfg.is_explicit_deny_module(root) {
            self.add(
                node.range(),
                "import",
                format!("dangerous module: {module}"),
            );
        } else if cfg.is_deny_module(module) || cfg.is_deny_module(root) {
            if cfg.allowed_symbols.contains_key(module) {
                self.check_allowed_symbols(node, module);
            } else {
                self.add(
                    node.range(),
                    "import",
                    format!("dangerous module: {module}"),
                );
            }
        } else if !cfg.is_safe_module(module) && !cfg.is_safe_module(root) {
            if cfg.allowed_symbols.contains_key(module) {
                self.check_allowed_symbols(node, module);
            } else {
                self.add(node.range(), "import", format!("unknown module: {module}"));
            }
        }
    }

    /// Require every imported name to be explicitly allowed for the module.
    fn check_allowed_symbols(&mut self, node: &ast::StmtImportFrom, module: &str) {
        let allowed = &self.config.allowed_symbols[module];
        let mut found = Vec::new();
        for alias in &node.names {
            let name = alias.name.as_str();
            if name == "*" {
                found.push(("symbol", format!("wildcard import from {module}")));
            } else if !allowed.contains(name) {
                found.push(("symbol", format!("disallowed import from {module}: {name}")));
            }
        }
        for (kind, detail) in found {
            self.add(node.range(), kind, detail);
        }
    }

    fn arguments(&mut self, arguments: &ast::Arguments) {
        self.exprs(&arguments.args);
        for keyword in arguments.keywords.iter() {
            self.nested(keyword.range(), 2, |s| s.expr(&keyword.value));
        }
    }

    /// `ast.arguments`: posonlyargs, args, vararg, kwonlyargs, kw_defaults,
    /// kwarg, defaults.
    fn parameters(&mut self, p: &Parameters) {
        for param in p.posonlyargs.iter().chain(p.args.iter()) {
            self.annotation(&param.parameter);
        }
        if let Some(vararg) = &p.vararg {
            self.annotation(vararg);
        }
        for param in p.kwonlyargs.iter() {
            self.annotation(&param.parameter);
        }
        for param in p.kwonlyargs.iter() {
            self.opt_expr(param.default.as_deref());
        }
        if let Some(kwarg) = &p.kwarg {
            self.annotation(kwarg);
        }
        for param in p.posonlyargs.iter().chain(p.args.iter()) {
            self.opt_expr(param.default.as_deref());
        }
    }

    /// `ast.arg` node with its annotation.
    fn annotation(&mut self, param: &ast::Parameter) {
        self.nested(param.range(), 2, |s| {
            s.opt_expr(param.annotation.as_deref())
        });
    }

    fn type_params(&mut self, tp: &TypeParams) {
        for param in &tp.type_params {
            let default = match param {
                TypeParam::TypeVar(t) => t.default.as_deref(),
                TypeParam::TypeVarTuple(t) => t.default.as_deref(),
                TypeParam::ParamSpec(t) => t.default.as_deref(),
            };
            if let Some(default) = default {
                // Type parameter defaults are Python 3.13 syntax.
                self.reject(default.range(), "invalid syntax");
            }
            self.nested(param.range(), 2, |s| match param {
                TypeParam::TypeVar(t) => {
                    s.opt_expr(t.bound.as_deref());
                    s.opt_expr(t.default.as_deref());
                }
                TypeParam::TypeVarTuple(t) => s.opt_expr(t.default.as_deref()),
                TypeParam::ParamSpec(t) => s.opt_expr(t.default.as_deref()),
            });
        }
    }

    fn pattern_inner(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::MatchValue(p) => self.expr(&p.value),
            Pattern::MatchSingleton(_) | Pattern::MatchStar(_) => {}
            Pattern::MatchSequence(p) => {
                for sub in p.patterns.iter() {
                    self.pattern(sub);
                }
            }
            Pattern::MatchMapping(p) => {
                self.exprs(&p.keys);
                for sub in p.patterns.iter() {
                    self.pattern(sub);
                }
            }
            Pattern::MatchClass(p) => {
                self.expr(&p.cls);
                for sub in p.arguments.patterns.iter() {
                    self.pattern(sub);
                }
                for keyword in &p.arguments.keywords {
                    self.pattern(&keyword.pattern);
                }
            }
            Pattern::MatchAs(p) => {
                if let Some(sub) = &p.pattern {
                    self.pattern(sub);
                }
            }
            Pattern::MatchOr(p) => {
                for sub in p.patterns.iter() {
                    self.pattern(sub);
                }
            }
        }
    }

    /// F-string elements; `depth` is the format-spec nesting level within
    /// one f-string (a nested f-string expression starts again at 0).
    fn interpolated(&mut self, elements: &[InterpolatedStringElement], depth: usize) {
        for element in elements {
            if let InterpolatedStringElement::Interpolation(i) = element {
                if depth >= 3 {
                    self.reject(i.range(), "f-string: expressions nested too deeply");
                } else if depth >= 1 && i.debug_text.is_some() {
                    // CPython 3.12 raises ValueError ("field 'value' is
                    // required for Constant") for `=` inside a format spec.
                    self.reject(i.range(), "field 'value' is required for Constant");
                }
                // FormattedValue(value, format_spec=JoinedStr(...)).
                self.nested(i.range(), 2, |s| {
                    s.expr(&i.expression);
                    if let Some(spec) = &i.format_spec {
                        s.nested(spec.range(), 2, |s| {
                            s.interpolated(&spec.elements, depth + 1)
                        });
                    }
                });
            }
        }
    }

    fn comprehensions(&mut self, generators: &[ast::Comprehension]) {
        for generator in generators {
            self.nested(generator.range(), 2, |s| {
                s.expr(&generator.target);
                s.expr(&generator.iter);
                s.exprs(&generator.ifs);
            });
        }
    }

    fn expr_inner(&mut self, expr: &Expr) {
        match expr {
            Expr::BoolOp(e) => self.exprs(&e.values),
            Expr::Named(e) => {
                self.expr(&e.target);
                self.expr(&e.value);
            }
            Expr::BinOp(e) => {
                self.expr(&e.left);
                self.expr(&e.right);
            }
            Expr::UnaryOp(e) => self.expr(&e.operand),
            Expr::Lambda(e) => {
                if let Some(p) = &e.parameters {
                    self.nested(p.range(), 2, |s| s.parameters(p));
                }
                self.expr(&e.body);
            }
            Expr::If(e) => {
                self.expr(&e.test);
                self.expr(&e.body);
                self.expr(&e.orelse);
            }
            Expr::Dict(e) => {
                // ast.Dict: all keys, then all values.
                for item in &e.items {
                    self.opt_expr(item.key.as_ref());
                }
                for item in &e.items {
                    self.expr(&item.value);
                }
            }
            Expr::Set(e) => self.exprs(&e.elts),
            Expr::ListComp(e) => {
                self.expr(&e.elt);
                self.comprehensions(&e.generators);
            }
            Expr::SetComp(e) => {
                self.expr(&e.elt);
                self.comprehensions(&e.generators);
            }
            Expr::Generator(e) => {
                self.expr(&e.elt);
                self.comprehensions(&e.generators);
            }
            Expr::DictComp(e) => {
                self.opt_expr(e.key.as_deref());
                self.expr(&e.value);
                self.comprehensions(&e.generators);
            }
            Expr::Await(e) => {
                self.add(e.range(), "async", "await requires asyncio".into());
                self.expr(&e.value);
            }
            Expr::Yield(e) => self.opt_expr(e.value.as_deref()),
            Expr::YieldFrom(e) => self.expr(&e.value),
            Expr::Compare(e) => self.exprs(&e.operands),
            Expr::Call(e) => {
                match e.func.as_ref() {
                    Expr::Name(name) => {
                        let name = name.id.as_str();
                        if DANGEROUS_BUILTINS.contains(&name)
                            && !(name == "print" && self.allow_print)
                        {
                            self.add(e.range(), "builtin", format!("dangerous builtin: {name}"));
                        }
                    }
                    Expr::Attribute(attr) => {
                        let attr = attr.attr.as_str();
                        if DANGEROUS_ATTRS.contains(&attr) {
                            self.add(e.range(), "method", format!("dangerous method: {attr}"));
                        }
                    }
                    _ => {}
                }
                self.expr(&e.func);
                self.arguments(&e.arguments);
            }
            Expr::FString(e) => {
                for part in &e.value {
                    if let FStringPartRef::FString(f) = part {
                        self.fstring_level += 1;
                        if self.fstring_level >= MAX_FSTRING_LEVEL {
                            self.reject(f.range(), "too many nested f-strings");
                        }
                        self.interpolated(&f.elements, 0);
                        self.fstring_level -= 1;
                    }
                }
            }
            Expr::TString(e) => {
                for t in e.value.iter() {
                    self.interpolated(&t.elements, 0);
                }
            }
            Expr::StringLiteral(_)
            | Expr::BytesLiteral(_)
            | Expr::NumberLiteral(_)
            | Expr::BooleanLiteral(_)
            | Expr::NoneLiteral(_)
            | Expr::EllipsisLiteral(_)
            | Expr::Constant(_) => {}
            Expr::Attribute(e) => {
                let attr = e.attr.as_str();
                if REFLECTION_ATTRS.contains(&attr) {
                    self.add(
                        e.range(),
                        "reflection",
                        format!("dangerous attribute: {attr}"),
                    );
                }
                self.expr(&e.value);
            }
            Expr::Subscript(e) => {
                self.expr(&e.value);
                self.expr(&e.slice);
            }
            Expr::Starred(e) => self.expr(&e.value),
            Expr::Name(e) => {
                let name = e.id.as_str();
                if matches!(name, "__builtins__" | "__loader__" | "__spec__") {
                    self.add(e.range(), "reflection", format!("dangerous name: {name}"));
                }
            }
            Expr::List(e) => self.exprs(&e.elts),
            Expr::Tuple(e) => self.exprs(&e.elts),
            Expr::Slice(e) => {
                self.opt_expr(e.lower.as_deref());
                self.opt_expr(e.upper.as_deref());
                self.opt_expr(e.step.as_deref());
            }
            // Not produced in module mode; never reached with valid syntax.
            Expr::IpyEscapeCommand(e) => {
                self.add(e.range(), "syntax", "unsupported syntax".into());
            }
        }
    }
}

/// Stack for parsing and dropping deeply nested ASTs (reserved lazily).
const ANALYSIS_STACK_BYTES: usize = 256 * 1024 * 1024;

/// Analyze Python source code for safety violations (`analyze_python_source`).
///
/// Returns the violations, empty if the code appears safe. Parsing and
/// dropping the AST recurse once per nesting level, so the work runs on a
/// thread with a large stack; any failure there is a syntax violation.
pub fn analyze_python_source(
    source: &str,
    allow_print: bool,
    config: &AnalysisConfig,
) -> Vec<Violation> {
    let failed = || {
        vec![Violation {
            line: 0,
            kind: "syntax",
            detail: "analysis failed".into(),
        }]
    };
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(ANALYSIS_STACK_BYTES)
            .spawn_scoped(scope, || analyze_source(source, allow_print, config))
            .ok()
            .and_then(|handle| handle.join().ok())
            .unwrap_or_else(failed)
    })
}

fn analyze_source(source: &str, allow_print: bool, config: &AnalysisConfig) -> Vec<Violation> {
    let lines = line_starts(source);
    let line_at = |offset: usize| lines.partition_point(|&s| s <= offset);
    // CPython 3.12: "source code string cannot contain null bytes".
    if source.contains('\0') {
        return vec![Violation {
            line: 0,
            kind: "syntax",
            detail: "source code string cannot contain null bytes".into(),
        }];
    }
    // The parser skips a leading BOM; `ast.parse` of a str rejects it.
    if source.starts_with('\u{feff}') {
        return vec![Violation {
            line: 1,
            kind: "syntax",
            detail: "invalid non-printable character U+FEFF (<unknown>, line 1)".into(),
        }];
    }
    let options = ParseOptions::from(Mode::Module).with_target_version(PythonVersion::PY312);
    let parsed = match parse(source, options) {
        Ok(parsed) => parsed,
        Err(err) => {
            let line = line_at(err.location.start().to_usize());
            return vec![Violation {
                line,
                kind: "syntax",
                detail: format!("invalid syntax (<unknown>, line {line})"),
            }];
        }
    };
    // Syntax newer than Python 3.12 is a syntax error for the oracle.
    if let Some(err) = parsed.unsupported_syntax_errors().first() {
        let line = line_at(err.range.start().to_usize());
        return vec![Violation {
            line,
            kind: "syntax",
            detail: format!("invalid syntax (<unknown>, line {line})"),
        }];
    }
    let Some(module) = parsed.syntax().as_module() else {
        return vec![Violation {
            line: 0,
            kind: "syntax",
            detail: "invalid syntax".into(),
        }];
    };
    let mut analyzer = SafetyAnalyzer {
        violations: Vec::new(),
        allow_print,
        config,
        lines,
        syntax_error: None,
        frames: 2, // Module
        fstring_level: 0,
    };
    analyzer.body(&module.body);
    match analyzer.syntax_error {
        Some(error) => vec![error],
        None => analyzer.violations,
    }
}

/// `Path.suffix`.
fn path_suffix(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    match name.rfind('.') {
        Some(i) if i > 0 && i < name.len() - 1 => name[i..].to_string(),
        _ => String::new(),
    }
}

/// Analyze a Python file for safety (`analyze_python_file`).
///
/// Returns (is_safe, reason).
pub fn analyze_python_file(path: &Path, config: &AnalysisConfig) -> (bool, String) {
    if !path.exists() {
        return (false, format!("file not found: {}", path.display()));
    }
    if !path.is_file() {
        return (false, format!("not a file: {}", path.display()));
    }
    let suffix = path_suffix(path);
    if suffix != ".py" && suffix != ".pyw" {
        return (false, format!("not a Python file: {suffix}"));
    }
    match std::fs::metadata(path) {
        Ok(meta) if meta.len() > 100_000 => return (false, "file too large to analyze".into()),
        Ok(_) => {}
        Err(e) => return (false, format!("cannot stat file: {e}")),
    }
    let source = match std::fs::read_to_string(path) {
        // read_text() uses universal newlines.
        Ok(s) => s.replace("\r\n", "\n").replace('\r', "\n"),
        Err(e) => return (false, format!("cannot read file: {e}")),
    };
    let violations = analyze_python_source(&source, true, config);
    if let Some(v) = violations.first() {
        return (false, format!("{}: {} (line {})", v.kind, v.detail, v.line));
    }
    (true, "static analysis passed".into())
}

// === Python Flag Parsing ===

/// Python flags that take an argument.
const FLAGS_WITH_ARG: &[&str] = &["-c", "-m", "-W", "-X", "--check-hash-based-pycs"];

/// Python flags that are safe (no code execution).
const SAFE_FLAGS: &[&str] = &["-V", "--version", "-h", "--help", "-VV"];

/// Find the script path in Python command tokens (`_find_script_path`).
fn find_script_path(tokens: &[String], cwd: &Path) -> Option<(PathBuf, usize)> {
    let mut i = 1;
    while i < tokens.len() {
        let token = tokens[i].as_str();
        if SAFE_FLAGS.contains(&token) {
            return None;
        }
        if token == "-c" || token == "-m" {
            return None;
        }
        if FLAGS_WITH_ARG.contains(&token) {
            i += 2;
            continue;
        }
        if token.starts_with('-') {
            i += 1;
            continue;
        }
        return Some((resolve_arg_path(token, cwd), i));
    }
    None
}

/// `Path(token).name`.
fn pathlib_name(token: &str) -> String {
    let s = pathlib_str(token);
    if s == "." {
        return String::new();
    }
    s.rsplit('/').next().unwrap_or("").to_string()
}

/// `str.isspace()` for one character.
fn py_isspace(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// Get description for Python command.
pub fn get_description(tokens: &[String]) -> String {
    if tokens.len() < 2 {
        return tokens[0].clone();
    }
    for token in &tokens[1..] {
        if SAFE_FLAGS.contains(&token.as_str()) {
            return format!("{} {}", tokens[0], token);
        }
        if token == "-c" {
            return format!("{} -c", tokens[0]);
        }
        if token == "-m" {
            let idx = tokens.iter().position(|t| t == "-m").unwrap_or(0);
            if idx + 1 < tokens.len() {
                return format!("{} -m {}", tokens[0], tokens[idx + 1]);
            }
            return format!("{} -m", tokens[0]);
        }
        if !token.starts_with('-') {
            return format!("{} {}", tokens[0], pathlib_name(token));
        }
    }
    tokens[0].clone()
}

/// Classify Python command for approval.
pub fn classify(ctx: &HandlerContext) -> Classification {
    let tokens = &ctx.tokens;
    let config = match ctx.config {
        Some(c) => AnalysisConfig {
            extra_safe_modules: c.python_allow_modules.iter().cloned().collect(),
            extra_deny_modules: c.python_deny_modules.iter().cloned().collect(),
            allowed_symbols: build_allowed_symbols(&c.python_allow_symbols),
        },
        None => AnalysisConfig::default(),
    };

    let desc = get_description(tokens);

    if tokens.len() < 2 {
        return Classification::ask_desc(format!("{} interactive", tokens[0]));
    }

    // Check for -c/-m BEFORE safe flags: after -c, remaining tokens are
    // sys.argv for the script, not interpreter options.
    let c_idx = tokens.iter().position(|t| t == "-c");
    let m_idx = tokens.iter().position(|t| t == "-m");

    if c_idx.is_none() && m_idx.is_none() {
        for token in &tokens[1..] {
            if SAFE_FLAGS.contains(&token.as_str()) {
                return Classification::allow_desc(desc);
            }
        }
    }

    if let Some(idx) = c_idx {
        let code_idx = idx + 1;
        if code_idx >= tokens.len() {
            return Classification::ask_desc(desc);
        }
        if ctx.word_has_expansions.get(code_idx) == Some(&true) {
            return Classification::ask_desc(format!("{desc} (bash expansion)"));
        }
        let code = &tokens[code_idx];
        if code.trim_matches(py_isspace).is_empty() {
            return Classification::ask_desc(desc);
        }
        let violations = analyze_python_source(code, true, &config);
        return match violations.first() {
            None => Classification::allow_desc(format!("{desc} (analyzed)")),
            Some(v) => Classification::ask_desc(format!("{desc}: {}: {}", v.kind, v.detail)),
        };
    }

    if let Some(idx) = m_idx {
        // Only calendar is truly inert (prints output, no I/O or code exec).
        if tokens.get(idx + 1).map(String::as_str) == Some("calendar") {
            return Classification::allow_desc(desc);
        }
        return Classification::ask_desc(desc);
    }

    if tokens.iter().any(|t| t == "-i") {
        return Classification::ask_desc(desc);
    }

    let Some((script_path, _)) = find_script_path(tokens, &ctx.cwd) else {
        return Classification::ask_desc(desc);
    };

    let (is_safe, reason) = analyze_python_file(&script_path, &config);
    if is_safe {
        Classification::allow_desc(format!("{desc} (analyzed)"))
    } else {
        Classification::ask_desc(format!("{desc}: {reason}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Action;
    use crate::config::Config;

    fn tokens(cmd: &[&str]) -> Vec<String> {
        cmd.iter().map(|s| s.to_string()).collect()
    }

    fn run(cmd: &[&str]) -> Classification {
        classify(&HandlerContext::new(cmd))
    }

    fn run_in(cmd: &[&str], cwd: &Path) -> Classification {
        let mut ctx = HandlerContext::new(cmd);
        ctx.cwd = cwd.to_path_buf();
        classify(&ctx)
    }

    fn run_cfg(code: &str, config: &Config) -> Action {
        let mut ctx = HandlerContext::new(&["python3", "-c", code]);
        ctx.config = Some(config);
        classify(&ctx).action
    }

    fn analyze(source: &str) -> Vec<Violation> {
        analyze_python_source(source, true, &AnalysisConfig::default())
    }

    /// Temporary directory removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("dippy-python-test-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
        fn write(&self, name: &str, content: &str) -> String {
            let path = self.0.join(name);
            std::fs::write(&path, content).unwrap();
            path.to_string_lossy().into_owned()
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Classify `python <script>` for a script with `content`.
    fn script(name: &str, content: &str) -> Action {
        let dir = TempDir::new(name);
        let path = dir.write(name, content);
        run(&["python", &path]).action
    }

    #[test]
    fn version_help_approved() {
        for cmd in [
            &["python", "--version"][..],
            &["python", "-V"],
            &["python", "-VV"],
            &["python", "--help"],
            &["python", "-h"],
            &["python3", "--version"],
            &["python3", "-V"],
            &["python3.11", "--version"],
            &["python3.12", "--version"],
        ] {
            assert_eq!(run(cmd).action, Action::Allow, "{cmd:?}");
        }
    }

    #[test]
    fn code_execution_needs_confirmation() {
        for cmd in [
            &["python3", "-c", "import os; os.system(\"ls\")"][..],
            &["python", "-m", "http.server"],
            &["python", "-m", "pip", "install", "foo"],
            &["python", "-m", "pytest"],
            &["python", "-m", "venv", ".venv"],
            &["python"],
            &["python", "-i", "script.py"],
            &["python", "-m", "timeit", "import os"],
            &["python", "-m", "timeit", "-s", "import os", "os.getcwd()"],
            &["python", "-m", "json.tool", "foo.json"],
            &["python", "-m", "pydoc", "os"],
            &["python", "-m"],
            &["python", "-c"],
            &["python", "-c", "  "],
            &["python", "-c", "os.system('rm')", "--help"],
        ] {
            assert_eq!(run(cmd).action, Action::Ask, "{cmd:?}");
        }
        assert_eq!(
            run(&["python"]).description.as_deref(),
            Some("python interactive")
        );
    }

    #[test]
    fn inline_safe_code_and_calendar_approved() {
        assert_eq!(run(&["python", "-c", "print(1)"]).action, Action::Allow);
        assert_eq!(run(&["python", "-c", "x=1"]).action, Action::Allow);
        let r = run(&["python", "-m", "calendar"]);
        assert_eq!(r.action, Action::Allow);
        assert_eq!(r.description.as_deref(), Some("python -m calendar"));
    }

    #[test]
    fn inline_code_with_expansion_asks() {
        let mut ctx = HandlerContext::new(&["python", "-c", "print($X)"]);
        ctx.word_has_expansions = vec![false, false, true];
        let r = classify(&ctx);
        assert_eq!(r.action, Action::Ask);
        assert_eq!(r.description.as_deref(), Some("python -c (bash expansion)"));
    }

    #[test]
    fn inline_code_analysis() {
        for code in [
            "import json; d=json.loads('{\"a\":1}'); print(d['a'])",
            "import json; json.dumps({'x': 1})",
            "x=1; y=2; print(x+y)",
            "from collections import OrderedDict; d=OrderedDict()",
            "import math; print(math.pi)",
        ] {
            assert_eq!(run(&["python", "-c", code]).action, Action::Allow, "{code}");
        }
        for code in [
            "import os; os.system('rm -rf /')",
            "import subprocess; subprocess.run(['ls'])",
            "open('/etc/passwd').read()",
            "import shutil; shutil.rmtree('/tmp')",
            "import pathlib; p=pathlib.Path('/tmp')",
        ] {
            assert_eq!(run(&["python", "-c", code]).action, Action::Ask, "{code}");
        }
        let r = run(&["python", "-c", "import os"]);
        assert_eq!(
            r.description.as_deref(),
            Some("python -c: import: dangerous module: os")
        );
        let r = run(&["python", "-c", "x = 1"]);
        assert_eq!(r.description.as_deref(), Some("python -c (analyzed)"));
    }

    #[test]
    fn inline_code_with_config_modules() {
        let config = Config {
            python_allow_modules: vec!["graphify".into()],
            ..Config::default()
        };
        assert_eq!(
            run_cfg("import graphify; print(graphify)", &config),
            Action::Allow
        );
        let config = Config {
            python_deny_modules: vec!["json".into()],
            ..Config::default()
        };
        assert_eq!(
            run_cfg("import json; print(json.dumps({}))", &config),
            Action::Ask
        );
    }

    #[test]
    fn allow_symbol() {
        let sym = |s: &[&str]| Config {
            python_allow_symbols: s.iter().map(|x| x.to_string()).collect(),
            ..Config::default()
        };
        let stdin = sym(&["sys.stdin"]);
        assert_eq!(
            run_cfg("from sys import stdin\nprint(stdin.isatty())", &stdin),
            Action::Allow
        );
        assert_eq!(
            run_cfg("from sys import stdin as s\nprint(s.isatty())", &stdin),
            Action::Allow
        );
        assert_eq!(
            run_cfg("from sys import exit\nexit(1)", &stdin),
            Action::Ask
        );
        assert_eq!(
            run_cfg("from sys import stdin, argv\nprint(stdin, argv)", &stdin),
            Action::Ask
        );
        assert_eq!(run_cfg("import sys\nprint(sys.stdin)", &stdin), Action::Ask);
        assert_eq!(run_cfg("from sys import *", &stdin), Action::Ask);
        assert_eq!(run_cfg("from socket import stdin", &stdin), Action::Ask);
        let deny = Config {
            python_deny_modules: vec!["sys".into()],
            ..sym(&["sys.stdin"])
        };
        assert_eq!(run_cfg("from sys import stdin", &deny), Action::Ask);
        assert_eq!(
            run_cfg(
                "from zonklib import helper\nprint(helper)",
                &sym(&["zonklib.helper"])
            ),
            Action::Allow
        );
        assert_eq!(run_cfg("from .sys import stdin", &stdin), Action::Ask);
    }

    #[test]
    fn relative_imports() {
        let config = Config::default();
        assert_eq!(
            run_cfg("from .json import loads\nprint(loads)", &config),
            Action::Ask
        );
        assert_eq!(
            run_cfg("from json import loads\nprint(loads)", &config),
            Action::Allow
        );
        assert_eq!(run_cfg("from . import x", &config), Action::Ask);
    }

    #[test]
    fn build_allowed_symbols_groups_by_module() {
        let grouped = build_allowed_symbols(&[
            "sys.stdin".into(),
            "a.b.c".into(),
            "nodot".into(),
            ".x".into(),
            "x.".into(),
        ]);
        assert_eq!(grouped.len(), 2);
        assert!(grouped["sys"].contains("stdin"));
        assert!(grouped["a.b"].contains("c"));
    }

    #[test]
    fn unit_analysis() {
        assert!(analyze("\nimport json\ndata = json.loads('{}')\nprint(data)\n").is_empty());
        let v = analyze("\nimport os\nos.system('ls')\n");
        assert!(v.iter().any(|v| v.kind == "import"));
        assert_eq!(v[0].line, 2);
        let v = analyze("\nx = eval(\"1 + 1\")\n");
        assert!(
            v.iter()
                .any(|v| v.kind == "builtin" && v.detail.contains("eval"))
        );
        for (source, needle) in [
            ("import xml.etree.ElementTree", "xml"),
            ("import marshal", "marshal"),
            ("import tarfile", "tarfile"),
            (
                "\ndef foo():\n    pass\ng = foo.__globals__\n",
                "__globals__",
            ),
        ] {
            let v = analyze(source);
            assert!(v.iter().any(|v| v.detail.contains(needle)), "{source}");
        }
        assert!(!analyze("\nimport sys\nf = sys._getframe()\ng = f.f_globals\n").is_empty());
        assert!(
            analyze(
                "\nimport json\nimport math\nfrom collections import Counter\n\n\
                 data = json.dumps({\"x\": math.pi})\ncounts = Counter([1, 2, 2, 3])\n\
                 print(data, counts)\n"
            )
            .is_empty()
        );
        let v = analyze_python_source("print(1)", false, &AnalysisConfig::default());
        assert_eq!(v[0].detail, "dangerous builtin: print");
    }

    #[test]
    fn violation_order_follows_cpython_fields() {
        // ast.Dict visits keys before values.
        let v = analyze("{1: eval(x), exec(y): 2}");
        assert_eq!(v[0].detail, "dangerous builtin: exec");
        // Assign visits targets before value.
        let v = analyze("a[eval(x)] = exec(y)");
        assert_eq!(v[0].detail, "dangerous builtin: eval");
        // FunctionDef visits body before decorators.
        let v = analyze("@exec\ndef f():\n    eval(x)\n");
        assert_eq!(v[0].detail, "dangerous builtin: eval");
        // Comprehension element before generators.
        let v = analyze("[eval(x) for x in exec(y)]");
        assert_eq!(v[0].detail, "dangerous builtin: eval");
        // A call is reported before its children.
        let v = analyze("open(x).read()");
        assert_eq!(v[0].detail, "dangerous method: read");
        assert_eq!(v[1].detail, "dangerous builtin: open");
        // Async def at the `async` line, decorated or not.
        let v = analyze("@dec\nasync def f():\n    pass\n");
        assert_eq!((v[0].kind, v[0].line), ("async", 2));
    }

    #[test]
    fn analyzer_constructs() {
        for (source, kind) in [
            ("with open('f') as f:\n    pass\n", "io"),
            ("async def f():\n    await g()\n", "async"),
            ("x = __builtins__", "reflection"),
            ("x = y.__class__", "reflection"),
            ("x = y.__getattribute__", "reflection"),
            ("f'{eval(x)}'", "builtin"),
            ("f'{x:{eval(y)}}'", "builtin"),
            (
                "match x:\n    case Foo(a=1) if eval(y):\n        pass\n",
                "builtin",
            ),
            (
                "match x:\n    case {1: v}:\n        y.__dict__\n",
                "reflection",
            ),
            ("def f(a=eval(x)):\n    pass\n", "builtin"),
            ("def f(*, a: exec(x) = 1):\n    pass\n", "builtin"),
            ("lambda: eval(x)", "builtin"),
            ("class A(metaclass=eval(x)):\n    pass\n", "builtin"),
            ("try:\n    pass\nexcept eval(x):\n    pass\n", "builtin"),
            ("type X = eval(y)", "builtin"),
            ("def f[T: eval(x)]():\n    pass\n", "builtin"),
            ("x = (y := eval(z))", "builtin"),
            ("import foo", "import"),
            ("from . import x", "import"),
            ("def f(:", "syntax"),
            ("x = 1 +", "syntax"),
            ("type X[T = int] = list[T]", "syntax"),
            ("t'{x}'", "syntax"),
            ("x = 1\0", "syntax"),
            // Accepted by the parser, rejected by CPython 3.12's ast.parse.
            ("\u{feff}x = 1", "syntax"),
            ("f'{x:{y:{z:{w}}}}'", "syntax"),
            ("f'{x:{y}{z:{w:{v}}}}'", "syntax"),
            ("f'{x:{y=}}'", "syntax"),
            ("f'{x:{y:{z=}}}'", "syntax"),
            ("import os\nf'{x:a{y=}b}'", "syntax"),
            ("type X[*Ts = int] = int", "syntax"),
            ("type X[**P = int] = int", "syntax"),
            (
                "try:\n    pass\nelse:\n    pass\nfinally:\n    pass\n",
                "syntax",
            ),
        ] {
            let v = analyze(source);
            assert_eq!(v.first().map(|v| v.kind), Some(kind), "{source:?}");
        }
        for source in [
            "",
            "# comment\n\"\"\"doc\"\"\"\n",
            "global x\nx = 1\n",
            "async with a:\n    pass\n",
            "with a() as b:\n    pass\n",
            "x = f'{y!r:>10}'",
            "match x:\n    case [1, *rest]:\n        pass\n",
            "from __future__ import annotations",
            "def f[T](x: T) -> T:\n    return x\n",
            "x = {**a, 'b': 1}",
            "print(f'{x[\"k\"]}')",
            "f'{x:{y:{z}}}'",
            "f'{x:{f\"{y:{z:{w}}}\"}}'",
            "f'{x=}'",
            "f'{x:{f\"{y=}\"}}'",
            "# \u{feff}\nx = 1",
            "try:\n    pass\nfinally:\n    pass\n",
        ] {
            assert!(analyze(source).is_empty(), "{source:?}");
        }
    }

    #[test]
    fn deep_nesting_fails_closed() {
        // ast.NodeVisitor raises RecursionError near 500 nested BinOps.
        let chain = |n: usize| format!("x = {}1", "1+".repeat(n));
        assert!(analyze(&chain(300)).is_empty());
        assert_eq!(analyze(&chain(500))[0].kind, "syntax");
        assert_eq!(analyze(&chain(40000))[0].kind, "syntax");
        for deep in [
            format!("x = {}1", "-".repeat(120_000)),
            format!("x{}", "[0]".repeat(40_000)),
            format!("f{}", "()".repeat(60_000)),
            format!("{}{}", "(".repeat(60_000), ")".repeat(60_000)),
        ] {
            assert_eq!(analyze(&deep)[0].kind, "syntax");
        }
        let attrs = format!("x{}", ".a".repeat(400));
        assert_eq!(analyze(&attrs)[0].kind, "syntax");
        // CPython's tokenizer rejects 150 nested f-strings.
        let fstr = |n: usize| format!("{}x{}", "f'{".repeat(n), "}'".repeat(n));
        assert!(analyze(&fstr(149)).is_empty());
        assert_eq!(analyze(&fstr(150))[0].kind, "syntax");
    }

    #[test]
    fn unicode_identifiers_are_nfkc_normalized() {
        // CPython normalizes identifiers with NFKC: these are `eval`/`os`.
        assert_eq!(
            run(&["python", "-c", "\u{ff45}val('1')"]).action,
            Action::Ask
        );
        assert_eq!(
            run(&["python", "-c", "import \u{ff4f}s"]).action,
            Action::Ask
        );
        assert_eq!(
            run(&["python", "-c", "x.__\u{ff44}ict__"]).action,
            Action::Ask
        );
    }

    #[test]
    fn description_variants() {
        let d = |cmd: &[&str]| get_description(&tokens(cmd));
        assert_eq!(d(&["python"]), "python");
        assert_eq!(d(&["python", "-V"]), "python -V");
        assert_eq!(d(&["python", "-u", "-c", "x"]), "python -c");
        assert_eq!(d(&["python", "-m", "pip", "x"]), "python -m pip");
        assert_eq!(d(&["python", "-m"]), "python -m");
        assert_eq!(d(&["python", "dir/sub/script.py"]), "python script.py");
        assert_eq!(d(&["python", "dir/"]), "python dir");
        assert_eq!(d(&["python", "."]), "python ");
        assert_eq!(d(&["python", "-u"]), "python");
    }

    #[test]
    fn find_script_path_skips_flags() {
        let cwd = Path::new("/base");
        let t = tokens(&["python", "-W", "ignore", "-X", "dev", "-u", "s.py"]);
        assert_eq!(
            find_script_path(&t, cwd),
            Some((PathBuf::from("/base/s.py"), 6))
        );
        let t = tokens(&["python", "--check-hash-based-pycs=always", "s.py"]);
        assert_eq!(find_script_path(&t, cwd).map(|(_, i)| i), Some(2));
        assert_eq!(find_script_path(&tokens(&["python", "-u"]), cwd), None);
        assert_eq!(find_script_path(&tokens(&["python", "-c", "x"]), cwd), None);
    }

    #[test]
    fn safe_scripts_approved() {
        for (name, content) in [
            (
                "safe.py",
                "\nimport json\nimport re\nfrom collections import defaultdict\n\n\
                 data = {'key': 'value'}\ntext = json.dumps(data)\n\
                 pattern = re.compile(r'\\d+')\nresult = [x * 2 for x in range(10)]\n\
                 print(result)\n",
            ),
            (
                "math_script.py",
                "\nimport math\nimport statistics\nfrom decimal import Decimal\n\n\
                 values = [1, 2, 3, 4, 5]\nmean = statistics.mean(values)\n\
                 result = math.sqrt(sum(x**2 for x in values))\n\
                 print(f\"Result: {result}\")\n",
            ),
            (
                "dataclass_script.py",
                "\nfrom dataclasses import dataclass, field\nfrom typing import List\n\
                 import json\n\n@dataclass\nclass Person:\n    name: str\n    age: int\n\
                 \x20   tags: List[str] = field(default_factory=list)\n\n\
                 p = Person(\"Alice\", 30, [\"dev\", \"py\"])\n\
                 print(json.dumps({\"name\": p.name, \"age\": p.age}))\n",
            ),
            (
                "algorithm.py",
                "import heapq\nimport bisect\nfrom itertools import permutations\n\
                 from functools import reduce\nheap = [3, 1]\nheapq.heapify(heap)\n\
                 product = reduce(lambda x, y: x * y, [1, 2, 3, 4])\n",
            ),
            (
                "hash_encode.py",
                "import hashlib\nimport hmac\nimport base64\nimport binascii\n\
                 data = b\"hello\"\nmac = hmac.new(b\"s\", data, hashlib.sha256).hexdigest()\n\
                 b64 = base64.b64encode(data).decode()\n",
            ),
            ("empty.py", ""),
            ("comments.py", "\n# comment\n\"\"\"\nA docstring\n\"\"\"\n"),
            (
                "print_test.py",
                "print(\"Hello\")\nprint(1, 2, sep=\", \")\n",
            ),
            (
                "class_def.py",
                "from abc import ABC, abstractmethod\n\nclass Base(ABC):\n\
                 \x20   @abstractmethod\n    def method(self):\n        pass\n\n\
                 class Derived(Base):\n    def method(self):\n        return 42\n",
            ),
            (
                "comprehensions.py",
                "squares = [x**2 for x in range(10)]\nm = {x: x**2 for x in range(10)}\n\
                 s = {x for x in range(3)}\nt = sum(x for x in range(10))\n",
            ),
            (
                "zlib_safe.py",
                "import zlib\nc = zlib.compress(b'x')\nprint(zlib.decompress(c))\n",
            ),
            ("crlf.py", "x = 1\r\ny = 2\r\n"),
        ] {
            assert_eq!(script(name, content), Action::Allow, "{name}");
        }
    }

    #[test]
    fn dangerous_scripts_blocked() {
        for (name, content) in [
            ("os.py", "import os\nprint(os.getcwd())\n"),
            ("sp.py", "import subprocess\nsubprocess.run(['ls'])\n"),
            (
                "pl.py",
                "from pathlib import Path\nPath('t').write_text('h')\n",
            ),
            ("sock.py", "import socket\ns = socket.socket()\n"),
            ("req.py", "import requests\n"),
            ("ev.py", "code = '1'\nresult = eval(code)\n"),
            ("ex.py", "exec('x = 1')\n"),
            ("op.py", "with open('f', 'w') as f:\n    f.write('d')\n"),
            ("di.py", "os = __import__('os')\n"),
            (
                "refl.py",
                "class Foo:\n    pass\nprint(Foo.__subclasses__())\n",
            ),
            ("as.py", "async def fetch():\n    return 1\n"),
            (
                "syn.py",
                "\ndef foo(\n    # Missing closing paren\nprint(\"hello\")\n",
            ),
            ("codecs.py", "import codecs\n"),
            ("gz.py", "import gzip\n"),
            ("insp.py", "import inspect\n"),
            ("lc.py", "import linecache\n"),
            ("comp.py", "modules = [__import__('os') for _ in [1]]\n"),
            ("ga.py", "open_func = getattr(__builtins__, 'open')\n"),
            ("sub.py", "s = ().__class__.__bases__[0].__subclasses__()\n"),
            ("gl.py", "g = globals()\n"),
            ("mar.py", "import marshal\n"),
            ("pk.py", "from pickle import loads, dumps\n"),
            ("xml.py", "from xml.etree import ElementTree\n"),
            ("tar.py", "from tarfile import open as tar_open\n"),
            ("wsgi.py", "from wsgiref.handlers import CGIHandler\n"),
            (
                "gen.py",
                "def gen():\n    yield 1\ng = gen()\nf = g.gi_frame\n",
            ),
            ("cc.py", "def foo():\n    pass\nb = foo.__code__.co_code\n"),
            ("rd.py", "f = x\ndata = f.read()\n"),
            ("conn.py", "s.connect(('localhost', 80))\n"),
            ("url.py", "from urllib.request import urlopen\n"),
        ] {
            assert_eq!(script(name, content), Action::Ask, "{name}");
        }
    }

    #[test]
    fn script_file_checks() {
        let dir = TempDir::new("checks");
        let large = dir.write("large.py", &"x = 1\n".repeat(20000));
        let r = run(&["python", &large]);
        assert_eq!(r.action, Action::Ask);
        assert!(
            r.description
                .unwrap()
                .ends_with("file too large to analyze")
        );
        let txt = dir.write("script.txt", "print('hello')");
        let r = run(&["python", &txt]);
        assert_eq!(
            r.description.as_deref(),
            Some("python script.txt: not a Python file: .txt")
        );
        let missing = format!("{}/nonexistent.py", dir.0.display());
        assert_eq!(run(&["python", &missing]).action, Action::Ask);
        std::fs::create_dir(dir.0.join("pkg.py")).unwrap();
        let r = run_in(&["python", "pkg.py"], &dir.0);
        assert!(r.description.unwrap().contains("not a file"));
        let bad = dir.write("bad.py", "\nimport os\n");
        let r = run(&["python", &bad]);
        assert_eq!(
            r.description.as_deref(),
            Some("python bad.py: import: dangerous module: os (line 2)")
        );
        assert_eq!(path_suffix(Path::new("/x/.py")), "");
        assert_eq!(path_suffix(Path::new("/x/a.")), "");
        assert_eq!(path_suffix(Path::new("/x/a.b.pyw")), ".pyw");
    }

    #[test]
    fn script_flags_and_cwd() {
        let dir = TempDir::new("flags");
        let safe = dir.write("safe.py", "import json\nprint(json.dumps({}))");
        for cmd in [
            &["python", "-u", &safe][..],
            &["python", "-O", &safe],
            &["python", "-u", "-B", "-O", &safe],
            &["python", "-W", "ignore", &safe],
        ] {
            assert_eq!(run(cmd).action, Action::Allow, "{cmd:?}");
        }
        // Relative script anchored to the context cwd.
        let r = run_in(&["python", "safe.py"], &dir.0);
        assert_eq!(r.action, Action::Allow);
        assert_eq!(r.description.as_deref(), Some("python safe.py (analyzed)"));
        dir.write("dangerous.py", "import os\n");
        assert_eq!(
            run_in(&["python", "dangerous.py"], &dir.0).action,
            Action::Ask
        );
        let other = TempDir::new("flags-other");
        assert_eq!(run_in(&["python", "safe.py"], &other.0).action, Action::Ask);
        assert_eq!(
            run(&["python3", "~nosuchuser12345/probe.py"]).action,
            Action::Ask
        );
        assert_eq!(run_in(&["python", "-u"], &dir.0).action, Action::Ask);
    }
}
