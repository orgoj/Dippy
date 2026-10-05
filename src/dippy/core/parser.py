"""
Bash command parsing utilities using Parable.

Provides safe tokenization for analyzing shell commands.
"""

from __future__ import annotations

from dippy.vendor.parable import parse


def tokenize(command: str, *, raw: bool = False) -> list[str]:
    """Tokenize bash; raw mode requires one simple command without redirects."""
    if not command or not command.strip():
        return []

    try:
        nodes = parse(command)
        if raw:
            if (
                len(nodes) != 1
                or nodes[0].kind != "command"
                or getattr(nodes[0], "redirects", None)
            ):
                return []
            return [word.value for word in nodes[0].words]
        tokens = _extract_tokens(nodes)
        if tokens:
            return tokens
    except Exception:
        pass

    return []


def _strip_quotes(value: str) -> str:
    """Strip surrounding quotes from a value."""
    if len(value) >= 2:
        if (value[0] == '"' and value[-1] == '"') or (
            value[0] == "'" and value[-1] == "'"
        ):
            return value[1:-1]
    return value


def _extract_tokens(nodes: list) -> list[str]:
    """Recursively extract word tokens from Parable AST nodes."""
    tokens = []
    for node in nodes:
        if node.kind == "word":
            tokens.append(_strip_quotes(node.value))
        elif node.kind == "command":
            for word in node.words:
                tokens.append(_strip_quotes(word.value))
        elif node.kind == "pipeline":
            if node.commands:
                tokens.extend(_extract_tokens([node.commands[0]]))
        elif node.kind == "list":
            for part in node.parts:
                if part.kind != "operator":
                    tokens.extend(_extract_tokens([part]))
                    break
    return tokens
