"""Literal SQL argument extraction shared by CLI handlers."""

from __future__ import annotations

from dippy.cli import HandlerContext
from dippy.core.bash import decode_literal_word


def literal_sql_arg(ctx: HandlerContext, index: int, *, prefix: str = "") -> str | None:
    """Return the SQL Bash passes for a literal word, or None if uncertain."""
    if index >= len(ctx.tokens) or len(ctx.raw_words) != len(ctx.tokens):
        return None
    if ctx.word_has_expansions and ctx.word_has_expansions[index]:
        return None
    raw = ctx.raw_words[index]
    if prefix:
        if not raw.startswith(prefix):
            return None
        raw = raw[len(prefix) :]
    return decode_literal_word(raw)
