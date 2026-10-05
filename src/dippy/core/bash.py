"""Bash quoting utilities for command reconstruction."""

from __future__ import annotations


def decode_literal_word(raw: str, *, reject_globs: bool = False) -> str | None:
    """Apply Bash quote removal to one word; reject shell expansion."""
    output: list[str] = []
    quote = ""
    i = 0
    while i < len(raw):
        char = raw[i]
        if quote == "'":
            if char == "'":
                quote = ""
            else:
                output.append(char)
        elif char == quote and quote:
            quote = ""
        elif char in "'\"" and not quote:
            quote = char
        elif char == "$":
            if quote != '"' or (
                i + 1 < len(raw)
                and (raw[i + 1].isalnum() or raw[i + 1] in "_({[*@#?-$!")
            ):
                return None
            output.append(char)
        elif char == "`":
            return None
        elif (
            reject_globs and not quote and (char in "*?[{}" or (char == "~" and i == 0))
        ):
            return None
        elif char == "\\":
            if i + 1 >= len(raw):
                return None
            following = raw[i + 1]
            if quote == '"' and following not in '"\\$`\n':
                output.append(char)
            else:
                if following != "\n":
                    output.append(following)
                i += 1
        else:
            output.append(char)
        i += 1
    return "".join(output) if not quote else None


def bash_quote(s: str) -> str:
    """Quote a string for safe use in bash.

    Uses single quotes (safest), with escape handling for embedded single quotes.
    Returns '' for empty strings. Returns unquoted if no special chars.
    """
    if not s:
        return "''"
    # Safe chars that don't need quoting
    safe = True
    for c in s:
        if not (c.isalnum() or c in "-_./=@:"):
            safe = False
            break
    if safe:
        return s
    # Single-quote, escaping embedded single quotes as '"'"'
    return "'" + s.replace("'", "'\"'\"'") + "'"


def bash_join(tokens: list[str]) -> str:
    """Join tokens into a bash command string with proper quoting."""
    return " ".join(bash_quote(t) for t in tokens)
