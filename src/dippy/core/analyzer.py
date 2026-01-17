"""
Centralized AST analyzer for Dippy.

Single recursive walk of bash AST with consistent decision-making.
Unknown constructs default to ask. Decisions bubble up (deny > ask > allow).
"""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Literal

from dippy.core.config import Config, match_redirect, WrapperInfo
from dippy.core.allowlists import SIMPLE_SAFE, WRAPPER_COMMANDS
from dippy.cli import get_handler, get_description, HandlerContext
from dippy.vendor.parable import parse, ParseError

# Redirect targets that are always safe (no file write)
SAFE_REDIRECT_TARGETS = frozenset({"/dev/null", "-", "/dev/stdout", "/dev/stdin"})


@dataclass
class Decision:
    """Result of analyzing an AST node."""

    action: Literal["allow", "ask", "deny", "pass"]
    reason: str
    context_flags: frozenset[str] | None = None
    # For tracing: child decisions that contributed to this one
    children: list["Decision"] = field(default_factory=list)
    # Env-stripped command for audit log suggestion (ask decisions only)
    suggestion: str | None = None

    def __repr__(self) -> str:
        return f"Decision({self.action!r}, {self.reason!r})"


def analyze(
    command: str,
    config: Config,
    cwd: Path,
    context_flags: frozenset[str] | None = None,
    *,
    remote: bool = False,
) -> Decision:
    """
    Analyze a bash command string.

    Parses the command and recursively analyzes all nodes.
    Returns the combined decision (most restrictive wins).

    Args:
        command: Bash command string to analyze.
        config: Configuration with rules.
        cwd: Current working directory for path resolution.
        context_flags: Context flags for the command (@subshell, wrapper flags, etc.)
        remote: If True, command runs in remote context (container, ssh).
                Skips path-based checks since paths are remote, not local.
    """
    command = command.strip()
    if not command:
        return Decision("ask", "empty command")

    try:
        nodes = parse(command)
    except ParseError as e:
        return Decision("ask", f"parse error: {e.message}")

    if not nodes:
        return Decision("ask", "empty command")

    flags = context_flags or frozenset()
    decisions = [
        _analyze_node(node, config, cwd, flags, remote=remote) for node in nodes
    ]
    return _combine(decisions)


def _analyze_node(
    node,
    config: Config,
    cwd: Path,
    context_flags: frozenset[str] = frozenset(),
    *,
    remote: bool = False,
) -> Decision:
    """Recursively analyze a single AST node."""
    kind = getattr(node, "kind", None)

    if kind == "command":
        return _analyze_command(node, config, cwd, context_flags, remote=remote)

    elif kind == "pipeline":
        # Add @pipeline and @compound context flags for commands inside pipeline
        pipeline_flags = context_flags | frozenset({"@pipeline", "@compound"})
        # All commands in pipeline must be safe
        decisions = [
            _analyze_node(cmd, config, cwd, pipeline_flags, remote=remote)
            for cmd in node.commands
        ]
        result = _combine(decisions)
        if result.action == "allow":
            reasons = [d.reason for d in decisions]
            return Decision(
                "allow",
                ", ".join(reasons),
                context_flags=pipeline_flags,
                children=decisions,
            )
        return result

    elif kind == "list":
        # Add @compound context flag for commands in list (cmd1 && cmd2 || cmd3)
        list_flags = context_flags | frozenset({"@compound"})
        # All parts must be safe (skip operators like && ||)
        parts = [p for p in node.parts if getattr(p, "kind", None) != "operator"]
        # Check if first part is `cd <literal>` - use that path for subsequent parts
        effective_cwd = cwd
        if parts and not remote:
            cd_target = _extract_cd_target(parts[0])
            if cd_target:
                effective_cwd = _resolve_cd_target(cd_target, cwd)

        # When remote, skip analyzing any 'cd' commands (safe in containers/ssh)
        # We don't track path changes in remote mode, so cd is a no-op for security
        decisions = []
        for p in parts:
            # Check if this is a cd command
            is_cd = False
            if getattr(p, "kind", None) == "command":
                words = getattr(p, "words", [])
                if words:
                    base = _get_word_value(words[0])
                    is_cd = base == "cd"

            if remote and is_cd:
                # Skip cd commands in remote mode
                continue
            decisions.append(
                _analyze_node(p, config, effective_cwd, list_flags, remote=remote)
            )

        result = (
            _combine(decisions)
            if decisions
            else Decision("allow", "empty list", context_flags=list_flags)
        )
        if result.action == "allow":
            reasons = [d.reason for d in decisions]
            return Decision(
                "allow",
                ", ".join(reasons),
                context_flags=list_flags,
                children=decisions,
            )
        return result

    elif kind == "if":
        decisions = [
            _analyze_node(node.condition, config, cwd, context_flags, remote=remote)
        ]
        decisions.append(
            _analyze_node(node.then_body, config, cwd, context_flags, remote=remote)
        )
        if hasattr(node, "else_body") and node.else_body:
            decisions.append(
                _analyze_node(node.else_body, config, cwd, context_flags, remote=remote)
            )
        # Also check redirects on the if itself
        decisions.extend(
            _analyze_redirects(node, config, cwd, context_flags, remote=remote)
        )
        return _combine(decisions)

    elif kind in ("while", "until"):
        decisions = [
            _analyze_node(node.condition, config, cwd, context_flags, remote=remote),
            _analyze_node(node.body, config, cwd, context_flags, remote=remote),
        ]
        decisions.extend(
            _analyze_redirects(node, config, cwd, context_flags, remote=remote)
        )
        return _combine(decisions)

    elif kind == "for":
        decisions = [
            _analyze_node(node.body, config, cwd, context_flags, remote=remote)
        ]
        # Check iteration words for cmdsubs
        for word in getattr(node, "words", []):
            decisions.extend(
                _analyze_word_parts(word, config, cwd, context_flags, remote=remote)
            )
        decisions.extend(
            _analyze_redirects(node, config, cwd, context_flags, remote=remote)
        )
        return _combine(decisions)

    elif kind == "for-arith":
        decisions = [
            _analyze_node(node.body, config, cwd, context_flags, remote=remote)
        ]
        # Check init/cond/incr expressions for cmdsubs (stored as raw strings)
        for expr in (node.init, node.cond, node.incr):
            if expr:
                decisions.extend(
                    _analyze_string_cmdsubs(
                        expr, config, cwd, context_flags, remote=remote
                    )
                )
        decisions.extend(
            _analyze_redirects(node, config, cwd, context_flags, remote=remote)
        )
        return _combine(decisions)

    elif kind == "select":
        decisions = [
            _analyze_node(node.body, config, cwd, context_flags, remote=remote)
        ]
        # Check selection words for cmdsubs
        for word in getattr(node, "words", []):
            decisions.extend(
                _analyze_word_parts(word, config, cwd, context_flags, remote=remote)
            )
        decisions.extend(
            _analyze_redirects(node, config, cwd, context_flags, remote=remote)
        )
        return _combine(decisions)

    elif kind == "case":
        decisions = []
        # Check case word for cmdsubs
        if hasattr(node, "word") and node.word:
            decisions.extend(
                _analyze_word_parts(
                    node.word, config, cwd, context_flags, remote=remote
                )
            )
        for pattern in node.patterns:
            if hasattr(pattern, "body") and pattern.body:
                decisions.append(
                    _analyze_node(
                        pattern.body, config, cwd, context_flags, remote=remote
                    )
                )
        decisions.extend(
            _analyze_redirects(node, config, cwd, context_flags, remote=remote)
        )
        return _combine(decisions) if decisions else Decision("allow", "empty case")

    elif kind == "function":
        # Function definition - analyze the body
        # Note: the function isn't executed when defined, but we still
        # want to know if it contains dangerous commands
        return _analyze_node(node.body, config, cwd, context_flags, remote=remote)

    elif kind == "subshell":
        # Add @subshell and @compound context flags for commands in subshell
        subshell_flags = context_flags | frozenset({"@subshell", "@compound"})
        decisions = [
            _analyze_node(node.body, config, cwd, subshell_flags, remote=remote)
        ]
        decisions.extend(
            _analyze_redirects(node, config, cwd, subshell_flags, remote=remote)
        )
        return _combine(decisions)

    elif kind == "brace-group":
        # Add @bracegroup and @compound context flags for commands inside brace group
        bracegroup_flags = context_flags | frozenset({"@bracegroup", "@compound"})
        decisions = [
            _analyze_node(node.body, config, cwd, bracegroup_flags, remote=remote)
        ]
        decisions.extend(
            _analyze_redirects(node, config, cwd, bracegroup_flags, remote=remote)
        )
        return _combine(decisions)

    elif kind == "time":
        # time command - analyze the pipeline being timed
        return _analyze_node(node.pipeline, config, cwd, context_flags, remote=remote)

    elif kind == "negation":
        # ! command - negates exit status, analyze the inner command
        return _analyze_node(node.pipeline, config, cwd, context_flags, remote=remote)

    elif kind == "coproc":
        # coproc [NAME] command - analyze the inner command
        return _analyze_node(node.command, config, cwd, context_flags, remote=remote)

    elif kind == "cond-expr":
        # [[ expression ]] - check for command substitutions in operands
        decisions = []
        if hasattr(node, "body") and node.body:
            decisions.extend(
                _analyze_cond_node(node.body, config, cwd, context_flags, remote=remote)
            )
        decisions.extend(
            _analyze_redirects(node, config, cwd, context_flags, remote=remote)
        )
        return _combine(decisions) if decisions else Decision("allow", "conditional")

    elif kind == "arith-cmd":
        # (( expr )) - check for command substitutions in the expression
        decisions = []
        for cmdsub in _find_cmdsubs_in_arith(node.expression):
            inner_decision = _analyze_node(
                cmdsub.command, config, cwd, context_flags, remote=remote
            )
            if inner_decision.action != "allow":
                decisions.append(
                    Decision(
                        inner_decision.action,
                        f"arithmetic cmdsub: {inner_decision.reason}",
                        children=[inner_decision],
                    )
                )
            else:
                decisions.append(inner_decision)
        decisions.extend(
            _analyze_redirects(node, config, cwd, context_flags, remote=remote)
        )
        return _combine(decisions) if decisions else Decision("allow", "arithmetic")

    elif kind == "comment":
        return Decision("allow", "comment")

    elif kind == "empty":
        return Decision("allow", "empty")

    else:
        # Unknown node type - default to ask
        return Decision("ask", f"unrecognized construct: {kind}")


def _extract_wrapper_args(
    tokens: list[str],
    info: WrapperInfo | None = None,
) -> tuple[str | None, str, str | None]:
    """Extract wrapper destination, inner command, and context from tokens.

    Args:
        tokens: Command tokens starting with wrapper name
        info: Optional WrapperInfo from config

    Returns:
        (destination, inner_command, context_value) tuple
        - destination: None for trigger-only wrappers, server/host otherwise
        - inner_command: The command to analyze
        - context_value: Value of --context flag if defined (e.g. session name)

    Trigger-only wrappers (no destination):
        wrapper rtk                    -> everything after 'rtk' is the command
        wrapper tokf --cmd run         -> find 'run', everything after is the command
        wrapper cca --cmd run --context "-t"
                                     -> 'cca -t SESSION run CMD' -> SESSION in context

    Destination-based wrappers (ssh-style):
        wrapper docker --cmd exec --flag -t
        -> 'docker -t CONTAINER exec CMD' extracts 'CONTAINER' and 'CMD'

    Note: When info is None, defaults to SSH-style destination-based behavior
    for backward compatibility with existing tests.
    """
    if not tokens or len(tokens) < 2:
        return None, "", None

    trigger = info.trigger if info else None
    target_flag = info.target_flag if info else None
    context_flag = info.context_flag if info else None

    # Options that take an argument (same as ssh.py)
    opts_with_arg = {
        "-b",
        "-c",
        "-D",
        "-E",
        "-e",
        "-F",
        "-I",
        "-i",
        "-J",
        "-L",
        "-l",
        "-m",
        "-O",
        "-o",
        "-p",
        "-Q",
        "-R",
        "-S",
        "-W",
        "-w",
        "-t",  # Added -t as it's common for target/tty
    }

    # 1. If trigger is present, find it and split
    if trigger:
        try:
            idx = tokens.index(trigger)
            inner_cmd = " ".join(tokens[idx + 1 :]) if idx + 1 < len(tokens) else ""

            # Extract context value if context_flag is defined
            context_value = None
            if context_flag:
                try:
                    c_idx = tokens.index(context_flag, 0, idx)
                    if c_idx + 1 < idx:
                        context_value = tokens[c_idx + 1]
                except ValueError:
                    pass

            # If target_flag is specified, look for destination BEFORE trigger
            # Otherwise, this is a trigger-only wrapper (no destination)
            dest = None
            if target_flag:
                try:
                    t_idx = tokens.index(target_flag, 0, idx)
                    if t_idx + 1 < idx:
                        dest = tokens[t_idx + 1]
                except ValueError:
                    pass

                if dest is None:
                    # Fallback: find first non-option before trigger
                    i = 1
                    while i < idx:
                        tok = tokens[i]
                        if tok.startswith("-"):
                            if tok in opts_with_arg:
                                i += 2
                            else:
                                i += 1
                        else:
                            dest = tok
                            break

            # For trigger-only (no target_flag), dest stays None
            return dest, inner_cmd, context_value
        except ValueError:
            # Trigger not found in command - no inner command to analyze
            return None, "", None

    # 2. SSH-style destination-based behavior (default, no --cmd)
    # Standard SSH-like logic (destination is first non-option)
    # Also extracts context_value if context_flag is defined
    i = 1  # Skip wrapper name (tokens[0])
    dest = None

    while i < len(tokens):
        tok = tokens[i]
        if tok == "--":
            # End of options - next token is destination
            i += 1
            if i < len(tokens):
                dest = tokens[i]
                i += 1
            break
        elif tok.startswith("-"):
            if tok in opts_with_arg:
                # Skip option and its argument
                i += 2
            else:
                # Flag without argument
                i += 1
        else:
            # First non-option is the destination
            dest = tok
            i += 1
            break

    if dest is None:
        return None, "", None

    # Everything after destination is the inner command
    inner_cmd = " ".join(tokens[i:]) if i < len(tokens) else ""

    # Extract context value if context_flag is defined
    context_value = None
    if context_flag:
        try:
            c_idx = tokens.index(context_flag)
            if c_idx + 1 < len(tokens):
                context_value = tokens[c_idx + 1]
        except ValueError:
            pass

    return dest, inner_cmd, context_value


def _analyze_command(
    node,
    config: Config,
    cwd: Path,
    context_flags: frozenset[str] = frozenset(),
    *,
    remote: bool = False,
) -> Decision:
    """Analyze a simple command node."""
    decisions = []

    # Get base command for injection check
    words = [_get_word_value(w) for w in node.words]
    # Track which words contain bash expansions (param, cmdsub, procsub)
    word_has_expansions = tuple(bool(getattr(w, "parts", [])) for w in node.words)
    # Skip env var assignments to find base command
    base_idx = 0
    while (
        base_idx < len(words)
        and "=" in words[base_idx]
        and not words[base_idx].startswith("-")
    ):
        base_idx += 1
    base = words[base_idx] if base_idx < len(words) else ""
    has_handler = get_handler(base) is not None
    is_simple_safe = base in SIMPLE_SAFE

    # 1. Check for process substitutions and command substitutions in words
    for position, word in enumerate(node.words):
        parts = getattr(word, "parts", [])
        word_value = getattr(word, "value", "")
        # Check if this is a pure cmdsub (entire word is just a cmdsub)
        is_pure_cmdsub = (
            len(parts) == 1
            and getattr(parts[0], "kind", None) == "cmdsub"
            and word_value.startswith("$(")
            and word_value.endswith(")")
        )

        for part in parts:
            part_kind = getattr(part, "kind", None)
            if part_kind == "procsub":
                # Process substitution: <(...) or >(...)
                inner_decision = _analyze_node(
                    part.command, config, cwd, context_flags, remote=remote
                )
                if inner_decision.action != "allow":
                    direction = getattr(part, "direction", "?")
                    return Decision(
                        inner_decision.action,
                        f"process substitution {direction}(...): {inner_decision.reason}",
                        children=[inner_decision],
                    )
                decisions.append(inner_decision)
            elif part_kind == "cmdsub":
                # Command substitution: $(...)
                inner_decision = _analyze_node(
                    part.command, config, cwd, context_flags, remote=remote
                )
                if inner_decision.action != "allow":
                    return Decision(
                        inner_decision.action,
                        f"command substitution: {inner_decision.reason}",
                        children=[inner_decision],
                    )
                decisions.append(inner_decision)
                # Check for injection risk: pure cmdsub in arg position of handler CLI
                # But allow if outer command is read-only (handler approves it)
                if (
                    is_pure_cmdsub
                    and has_handler
                    and not is_simple_safe
                    and position > base_idx
                ):
                    handler = get_handler(base)
                    outer_result = handler.classify(
                        HandlerContext(words[base_idx:], remote=remote, cwd=cwd)
                    )
                    if outer_result.action != "allow":
                        inner_cmd = _get_word_value(word).strip("$()")
                        return Decision("ask", f"cmdsub injection risk: {inner_cmd}")
            elif part_kind == "param":
                # Parameter expansion - check for cmdsubs in arg (raw string)
                arg = getattr(part, "arg", None)
                if arg and isinstance(arg, str):
                    param_decisions = _analyze_string_cmdsubs(
                        arg, config, cwd, context_flags, remote=remote
                    )
                    for pd in param_decisions:
                        if pd.action != "allow":
                            return pd
                    decisions.extend(param_decisions)

    # 2. Check redirects
    redirect_decisions = _analyze_redirects(
        node, config, cwd, context_flags, remote=remote
    )
    for rd in redirect_decisions:
        if rd.action != "allow":
            return rd
    decisions.extend(redirect_decisions)

    # 3. Check the command itself
    if not words:
        return Decision("allow", "empty command")

    # Conditional test commands ([ and test) - read-only, safe after cmdsub check
    if base in ("[", "test"):
        decisions.append(Decision("allow", "conditional test"))
        return _combine(decisions)

    cmd_decision = _analyze_simple_command(
        words,
        config,
        cwd,
        context_flags,
        remote=remote,
        word_has_expansions=word_has_expansions,
    )
    decisions.append(cmd_decision)

    return _combine(decisions)


def _analyze_redirects(
    node,
    config: Config,
    cwd: Path,
    context_flags: frozenset[str] | None = None,
    *,
    remote: bool = False,
) -> list[Decision]:
    """Analyze redirects on a node."""
    decisions = []
    redirects = getattr(node, "redirects", None) or []

    for r in redirects:
        r_kind = getattr(r, "kind", None)
        if r_kind == "heredoc":
            # Unquoted heredocs expand command substitutions
            if not getattr(r, "quoted", True):
                content = getattr(r, "content", "")
                if content:
                    decisions.extend(
                        _analyze_string_cmdsubs(
                            content, config, cwd, context_flags, remote=remote
                        )
                    )
            continue

        op = getattr(r, "op", "")
        target = _get_word_value(r.target) if r.target else ""

        # Check for cmdsubs in redirect target
        if r.target:
            target_cmdsub_decisions = _analyze_word_parts(
                r.target, config, cwd, remote=remote
            )
            decisions.extend(target_cmdsub_decisions)

        # In remote mode, skip path-based redirect checks (paths are container-local)
        if remote:
            continue

        # Skip safe redirects
        if target in SAFE_REDIRECT_TARGETS or target.startswith("&"):
            continue

        # Check output redirects against config
        if op in (">", ">>", "&>", "&>>", "2>", "2>>"):
            redirect_match = match_redirect(target, config, cwd)
            if redirect_match:
                if redirect_match.decision == "allow":
                    decisions.append(Decision("allow", f"redirect to {target}"))
                elif redirect_match.decision == "deny":
                    msg = redirect_match.message or redirect_match.pattern
                    decisions.append(Decision("deny", f"redirect to {target}: {msg}"))
                else:  # ask
                    msg = redirect_match.message or redirect_match.pattern
                    decisions.append(Decision("ask", f"redirect to {target}: {msg}"))
            else:
                # No rule matched - default ask for output redirects
                decisions.append(Decision("ask", f"redirect to {target}"))

    return decisions


def _analyze_simple_command(
    words: list[str],
    config: Config,
    cwd: Path,
    context_flags: frozenset[str] = frozenset(),
    *,
    remote: bool = False,
    word_has_expansions: tuple[bool, ...] = (),
) -> Decision:
    """Analyze a simple command (list of words)."""
    if not words:
        return Decision("allow", "empty", context_flags=context_flags)

    # Skip leading environment variable assignments (FOO=bar)
    i = 0
    while i < len(words) and "=" in words[i] and not words[i].startswith("-"):
        i += 1

    if i >= len(words):
        return Decision("allow", "env assignment", context_flags=context_flags)

    base = words[i]
    tokens = words[i:]

    # Compute suggestion for ask decisions: env-stripped command.
    # Always set — _analyze_simple_command is ONLY reached from command-matching.
    # Redirect/substitution asks come from _analyze_command and skip this function.
    # So suggestion is not None iff ask came from command matching.
    suggestion = " ".join(tokens)

    # 1. Check config rules first (highest priority)
    from dippy.core.config import SimpleCommand, match_command

    cmd = SimpleCommand(words=words)
    config_match = match_command(cmd, config, cwd, context_flags, remote=remote)
    if config_match:
        if config_match.decision == "allow":
            # If pattern already starts with base, avoid redundant "base (pattern)" format
            pattern = config_match.pattern
            if pattern.startswith(base + " "):
                reason = pattern
            else:
                reason = f"{base} ({pattern})"
            return Decision("allow", reason, context_flags=context_flags)
        elif config_match.decision == "deny":
            msg = config_match.message or config_match.pattern
            return Decision("deny", f"{base}: {msg}", context_flags=context_flags)
        else:  # ask
            msg = config_match.message or config_match.pattern
            return Decision(
                "ask",
                f"{base}: {msg}",
                context_flags=context_flags,
                suggestion=suggestion,
            )

    # 2. Handle wrapper commands (time, timeout, etc.) - analyze inner command
    if base in WRAPPER_COMMANDS and len(tokens) > 1:
        if base == "command" and len(tokens) > 1 and tokens[1] in ("-v", "-V"):
            return Decision("allow", "command -v", context_flags=context_flags)

        # Skip numeric arguments and flags until we find the actual command
        j = 1
        while j < len(tokens):
            token = tokens[j]
            if token.isdigit() or token.replace(".", "").isdigit():
                j += 1
                continue
            if token.startswith("-") and token != "--":
                j += 1
                continue
            if token == "--":
                j += 1
            break

        if j < len(tokens):
            return _analyze_simple_command(
                tokens[j:],
                config,
                cwd,
                context_flags,
                remote=remote,
                word_has_expansions=word_has_expansions[j:]
                if word_has_expansions
                else (),
            )
        return Decision("ask", base, context_flags=context_flags, suggestion=suggestion)

    # 3. Simple safe commands
    if base in SIMPLE_SAFE:
        return Decision("allow", base, context_flags=context_flags)

    # 4. Version/help checks
    if _is_version_or_help(tokens):
        return Decision("allow", f"{base} --help", context_flags=context_flags)

    # 5. Custom wrapper commands (configured via 'wrapper' directive)
    if base in config.wrappers:
        info = config.wrappers[base]
        dest, inner_cmd, context_value = _extract_wrapper_args(tokens, info)

        if not inner_cmd:
            # No inner command (interactive session or trigger not found)
            reason = f"{base} {dest}" if dest else base
            return Decision(
                "ask", reason, context_flags=context_flags, suggestion=suggestion
            )

        # Build wrapper_context: wrapper name, destination if context_first, context_value if present
        wrapper_context = [base]
        if dest and info.context_first:
            wrapper_context.append(dest)
        if context_value:
            wrapper_context.append(context_value)
        inner_flags = context_flags | frozenset(wrapper_context)

        # We assume custom wrappers execute commands REMOTELY
        inner_decision = analyze(inner_cmd, config, cwd, inner_flags, remote=True)
        return inner_decision

    # 6. CLI-specific handlers
    handler = get_handler(base)
    if handler:
        result = handler.classify(
            HandlerContext(
                tokens,
                remote=remote,
                cwd=cwd,
                config=config,
                word_has_expansions=word_has_expansions,
            )
        )
        desc = result.description or get_description(tokens, base)
        # Check handler-provided redirect targets against config
        if result.redirect_targets:
            for target in result.redirect_targets:
                redirect_match = match_redirect(target, config, cwd)
                if redirect_match:
                    if redirect_match.decision == "deny":
                        msg = redirect_match.message or redirect_match.pattern
                        return Decision("deny", f"{desc}: {msg}")
                    elif redirect_match.decision == "ask":
                        msg = redirect_match.message or redirect_match.pattern
                        return Decision("ask", f"{desc}: {msg}")
                    # allow - continue checking other targets
                else:
                    # No matching rule - ask by default for file writes
                    return Decision("ask", desc)
        if result.action == "approve":
            return Decision("allow", desc)
        elif result.action == "delegate" and result.inner_command:
            # Delegate to inner command (e.g., bash -c 'inner')
            # Combine existing context_flags with wrapper_context from handler
            inner_flags = context_flags
            if result.wrapper_context:
                inner_flags = context_flags | frozenset(result.wrapper_context)

            inner_decision = analyze(
                result.inner_command,
                config,
                cwd,
                inner_flags,
                remote=remote or result.remote,
            )
            # For opt-in handlers (UV), apply outer suggestion on delegated ask
            # Only when inner ask came from command matching (has suggestion set)
            if (
                suggestion
                and inner_decision.action == "ask"
                and inner_decision.suggestion is not None
                and result.replace_suggestion
            ):
                inner_decision.suggestion = suggestion
            return inner_decision
        else:
            return Decision(
                "ask", desc, context_flags=context_flags, suggestion=suggestion
            )

    # 7. Special handling for sh/bash -c (delegate to analyzing the script string)
    if base in ("sh", "bash", "zsh") and len(tokens) >= 3 and tokens[1] == "-c":
        # Extract the script string (tokens[2])
        script = tokens[2]
        # Strip quotes if present
        script = _strip_quotes(script)
        # Delegate to analyzing the script string, preserving remote flag
        return analyze(script, config, cwd, context_flags, remote=remote)

    # 8. Unknown command - default ask
    return Decision(
        "ask",
        get_description(tokens, base),
        context_flags=context_flags,
        suggestion=suggestion,
    )


def _is_version_or_help(tokens: list[str]) -> bool:
    """Check if command is a version/help check."""
    if len(tokens) < 2:
        return False

    if len(tokens) == 2 and tokens[1] in ("help", "version"):
        return True

    if len(tokens) == 2 and tokens[1] in ("--version", "--help", "-h"):
        return True

    if tokens[-1] in ("--help", "-h") and len(tokens) <= 4:
        # After -c/-m, remaining tokens are script args, not interpreter flags
        if "-c" in tokens or "-m" in tokens:
            return False
        return True

    return False


def _get_word_value(word) -> str:
    """Extract string value from a word node, stripping outer quotes."""
    if isinstance(word, str):
        value = word
    else:
        value = getattr(word, "value", str(word))
    return _strip_quotes(value)


def _strip_quotes(value: str) -> str:
    """Strip surrounding quotes from a value."""
    if len(value) >= 2:
        if (value[0] == '"' and value[-1] == '"') or (
            value[0] == "'" and value[-1] == "'"
        ):
            return value[1:-1]
    return value


def _find_cmdsubs_in_arith(node) -> list:
    """Recursively find command substitutions in an arithmetic expression AST."""
    results = []
    if node is None:
        return results
    kind = getattr(node, "kind", None)
    if kind == "cmdsub":
        results.append(node)
        return results
    # Walk all child attributes that might contain nested expressions
    for attr in ("value", "target", "left", "right", "operand", "index", "expression"):
        child = getattr(node, attr, None)
        if child is not None:
            results.extend(_find_cmdsubs_in_arith(child))
    return results


def _analyze_cond_node(
    node,
    config: Config,
    cwd: Path,
    context_flags: frozenset[str] = frozenset(),
    *,
    remote: bool = False,
) -> list[Decision]:
    """Recursively analyze a conditional expression node for cmdsubs."""
    if node is None:
        return []
    kind = getattr(node, "kind", None)
    if kind == "unary-test":
        # -f file, -z string - check operand for cmdsubs
        return _analyze_word_parts(
            node.operand, config, cwd, context_flags, remote=remote
        )
    elif kind == "binary-test":
        # $a == $b - check both operands for cmdsubs
        decisions = []
        decisions.extend(
            _analyze_word_parts(node.left, config, cwd, context_flags, remote=remote)
        )
        decisions.extend(
            _analyze_word_parts(node.right, config, cwd, context_flags, remote=remote)
        )
        return decisions
    elif kind in ("cond-and", "cond-or"):
        # expr1 && expr2, expr1 || expr2 - recurse both sides
        decisions = []
        decisions.extend(
            _analyze_cond_node(node.left, config, cwd, context_flags, remote=remote)
        )
        decisions.extend(
            _analyze_cond_node(node.right, config, cwd, context_flags, remote=remote)
        )
        return decisions
    elif kind == "cond-not":
        # ! expr - recurse into operand
        return _analyze_cond_node(
            node.operand, config, cwd, context_flags, remote=remote
        )
    elif kind == "cond-paren":
        # ( expr ) - recurse into inner
        return _analyze_cond_node(node.inner, config, cwd, context_flags, remote=remote)
    return []


def _analyze_word_parts(
    word,
    config: Config,
    cwd: Path,
    context_flags: frozenset[str] = frozenset(),
    *,
    remote: bool = False,
) -> list[Decision]:
    """Analyze word parts for command/process substitutions, including nested ones."""
    decisions = []
    parts = getattr(word, "parts", [])
    for part in parts:
        part_kind = getattr(part, "kind", None)
        if part_kind == "cmdsub":
            inner_decision = _analyze_node(
                part.command, config, cwd, context_flags, remote=remote
            )
            if inner_decision.action != "allow":
                decisions.append(
                    Decision(
                        inner_decision.action,
                        f"cmdsub: {inner_decision.reason}",
                        children=[inner_decision],
                    )
                )
            else:
                decisions.append(inner_decision)
        elif part_kind == "procsub":
            inner_decision = _analyze_node(
                part.command, config, cwd, context_flags, remote=remote
            )
            if inner_decision.action != "allow":
                direction = getattr(part, "direction", "?")
                decisions.append(
                    Decision(
                        inner_decision.action,
                        f"procsub {direction}(...): {inner_decision.reason}",
                        children=[inner_decision],
                    )
                )
            else:
                decisions.append(inner_decision)
        elif part_kind == "param":
            # Parameter expansion - check for cmdsubs in arg value (raw string)
            # ${x:-$(cmd)}, ${x:=$(cmd)}, ${x:+$(cmd)}, ${x:?$(cmd)}
            arg = getattr(part, "arg", None)
            if arg and isinstance(arg, str):
                decisions.extend(
                    _analyze_string_cmdsubs(
                        arg, config, cwd, context_flags, remote=remote
                    )
                )
    return decisions


def _analyze_string_cmdsubs(
    s: str,
    config: Config,
    cwd: Path,
    context_flags: frozenset[str] | None = None,
    *,
    remote: bool = False,
) -> list[Decision]:
    """Extract and analyze command substitutions from a raw string."""
    decisions = []
    i = 0
    while i < len(s):
        # Look for $( pattern
        if s[i : i + 2] == "$(":
            # Find matching closing paren, accounting for nesting
            depth = 1
            start = i + 2
            j = start
            while j < len(s) and depth > 0:
                if s[j : j + 2] == "$(":
                    depth += 1
                    j += 2
                elif s[j] == ")":
                    depth -= 1
                    j += 1
                else:
                    j += 1
            if depth == 0:
                inner_cmd = s[start : j - 1]
                inner_decision = analyze(
                    inner_cmd, config, cwd, context_flags, remote=remote
                )
                if inner_decision.action != "allow":
                    decisions.append(
                        Decision(
                            inner_decision.action,
                            f"cmdsub: {inner_decision.reason}",
                            children=[inner_decision],
                        )
                    )
                else:
                    decisions.append(inner_decision)
                i = j
            else:
                i += 1
        # Look for backtick pattern
        elif s[i] == "`":
            # Find closing backtick (no nesting for backticks)
            j = i + 1
            while j < len(s) and s[j] != "`":
                j += 1
            if j < len(s):
                inner_cmd = s[i + 1 : j]
                inner_decision = analyze(
                    inner_cmd, config, cwd, context_flags, remote=remote
                )
                if inner_decision.action != "allow":
                    decisions.append(
                        Decision(
                            inner_decision.action,
                            f"cmdsub: {inner_decision.reason}",
                            children=[inner_decision],
                        )
                    )
                else:
                    decisions.append(inner_decision)
                i = j + 1
            else:
                i += 1
        else:
            i += 1
    return decisions


def _is_pure_cd_command(node) -> bool:
    """Check if a node is a pure 'cd <path>' command (no side effects).

    Used to skip cd commands when analyzing remote commands (container/ssh context).
    """
    if getattr(node, "kind", None) != "command":
        return False

    words = getattr(node, "words", [])
    if len(words) < 2:
        return False

    base = _get_word_value(words[0])
    if base != "cd":
        return False

    # Check for options or dangerous features in cd command
    for word in words[1:]:
        parts = getattr(word, "parts", [])
        if parts:
            # Has expansions - not a simple literal cd
            return False

    return True


def _extract_cd_target(node) -> str | None:
    """Extract target path from a `cd <literal>` command, or None if not applicable."""
    if getattr(node, "kind", None) != "command":
        return None
    words = getattr(node, "words", [])
    if len(words) != 2:
        return None
    base = _get_word_value(words[0])
    if base != "cd":
        return None
    target_word = words[1]
    # Only literal paths - no variables, command substitutions, etc.
    if getattr(target_word, "parts", None):
        for part in target_word.parts:
            part_kind = getattr(part, "kind", None)
            if part_kind in ("cmdsub", "param", "procsub"):
                return None
    return _get_word_value(target_word)


def _resolve_cd_target(target: str, cwd: Path) -> Path:
    """Resolve a cd target path to an absolute Path."""
    if target.startswith("~"):
        home = Path.home()
        if target == "~":
            return home
        return home / target[2:]  # ~/foo -> home / foo
    if target.startswith("/"):
        return Path(target)
    return (cwd / target).resolve()


def _combine(decisions: list[Decision]) -> Decision:
    """Combine multiple decisions - most restrictive wins, all reasons at that level."""
    if not decisions:
        return Decision("allow", "empty")

    # Collect reasons by decision level
    deny_reasons = [d.reason for d in decisions if d.action == "deny"]
    ask_reasons = [d.reason for d in decisions if d.action == "ask"]
    allow_reasons = [d.reason for d in decisions if d.action == "allow"]

    # Collect context_flags from children
    context_flags = frozenset()
    for d in decisions:
        if d.context_flags:
            context_flags = context_flags | d.context_flags

    # Propagate suggestion from ask decisions (only the command-matching one has it)
    ask_suggestion = None
    for d in decisions:
        if d.action == "ask" and d.suggestion is not None:
            ask_suggestion = d.suggestion
            break

    # deny > ask > allow
    if deny_reasons:
        return Decision(
            "deny",
            ", ".join(deny_reasons),
            context_flags=context_flags,
            children=decisions,
        )

    if ask_reasons:
        return Decision(
            "ask",
            ", ".join(ask_reasons),
            context_flags=context_flags,
            children=decisions,
            suggestion=ask_suggestion,
        )

    # All allowed
    return Decision(
        "allow",
        ", ".join(allow_reasons),
        context_flags=context_flags,
        children=decisions,
    )
