"""Explicit option declarations and literal argv matching for command rules."""

from __future__ import annotations

import fnmatch
import re

from dippy.core.bash import decode_literal_word
from dippy.core.parser import tokenize


def pattern_words(pattern: str) -> tuple[str, ...]:
    """Decode a simple rule pattern, retaining glob characters as patterns."""
    raw_words = tokenize(pattern, raw=True)
    words = [decode_literal_word(word) for word in raw_words]
    if not words or any(word is None for word in words):
        raise ValueError("requires a literal command pattern")
    return tuple(words)


def extract_options(pattern: str) -> tuple[str, dict[str, str | None] | None]:
    """Extract one leading opts block; commas inside quotes/globs are literal."""
    if not pattern.startswith("[opts:"):
        return pattern, None
    declarations = []
    start = len("[opts:")
    quote = ""
    escaped = False
    depth = 1
    for i in range(start, len(pattern)):
        char = pattern[i]
        if escaped:
            escaped = False
        elif char == "\\" and quote != "'":
            escaped = True
        elif quote:
            if char == quote:
                quote = ""
        elif char in "'\"":
            quote = char
        elif char == "[":
            depth += 1
        elif char == "]":
            depth -= 1
            if depth == 0:
                declarations.append(pattern[start:i].strip())
                remaining = pattern[i + 1 :].strip()
                break
        elif char == "," and depth == 1:
            declarations.append(pattern[start:i].strip())
            start = i + 1
    else:
        raise ValueError("unterminated opts block")

    if remaining.startswith("[opts:"):
        raise ValueError("only one opts block is permitted")
    options = {}
    if declarations == [""]:
        return remaining, options
    for declaration in declarations:
        words = pattern_words(declaration)
        if len(words) != 1:
            raise ValueError("option declarations require one name or name=value")
        name, separator, value = words[0].partition("=")
        if not re.fullmatch(r"-[A-Za-z0-9]|--[A-Za-z0-9][A-Za-z0-9_-]*", name):
            raise ValueError("invalid option name")
        if name in options:
            raise ValueError("duplicate option declaration")
        options[name] = value if separator else None
    return remaining, options


def positional_words(
    words: list[str], options: dict[str, str | None]
) -> list[str] | None:
    """Remove declared options, checking their values and preserving positionals."""
    if not words:
        return None
    positionals = [words[0]]
    seen = set()
    end_options = False
    i = 1
    while i < len(words):
        word = words[i]
        if end_options or word == "-" or not word.startswith("-"):
            positionals.append(word)
        elif word == "--":
            end_options = True
        else:
            if word.startswith("--"):
                name, separator, attached = word.partition("=")
                names = [name]
                value = attached if separator else None
            else:
                names = []
                value = None
                for j, char in enumerate(word[1:], 2):
                    name = "-" + char
                    names.append(name)
                    if name not in options:
                        return None
                    if options[name] is not None:
                        value = word[j:] or None
                        break

            for name in names:
                if name not in options or name in seen:
                    return None
                seen.add(name)
                value_pattern = options[name]
                if value_pattern is None:
                    if value is not None and name == names[-1]:
                        return None
                    continue
                if value is None:
                    i += 1
                    if i == len(words):
                        return None
                    value = words[i]
                if not fnmatch.fnmatchcase(value, value_pattern):
                    return None
        i += 1
    return positionals
