"""cp/mv command handler - routes the written paths through redirect rules.

Both commands write files, so their destination belongs to the same rules that
govern `>` and `tee`. mv also removes its sources, which is a write too, so a
path protected by a `deny-redirect` cannot be moved away either.

Without a matching redirect rule the analyzer asks, which is what cp and mv
did before this handler existed.
"""

from __future__ import annotations

from dippy.cli import Classification, HandlerContext

COMMANDS = ["cp", "mv"]

# Options whose argument is the destination directory
_TARGET_OPTS = frozenset({"-t", "--target-directory"})
# Options that take a separate argument which is not a path we care about
_ARG_OPTS = frozenset({"-S", "--suffix"})


def _basename(path: str) -> str:
    """Last component of a path, ignoring trailing slashes."""
    return path.rstrip("/").rsplit("/", 1)[-1] or "/"


def _parse(tokens: list[str]) -> tuple[list[str], str | None, bool]:
    """Split tokens into (positional args, explicit target dir, parsed_ok).

    parsed_ok is False when an option appeared that might swallow the next
    token, which would move the destination out from under us.
    """
    positional: list[str] = []
    target: str | None = None
    i = 1
    while i < len(tokens):
        token = tokens[i]

        if token == "--":
            positional.extend(tokens[i + 1 :])
            break

        if token.startswith("--"):
            name, equals, value = token.partition("=")
            if name in _TARGET_OPTS:
                if equals:
                    target = value
                elif i + 1 < len(tokens):
                    target = tokens[i + 1]
                    i += 1
                else:
                    return positional, target, False
            elif name in _ARG_OPTS:
                if not equals:
                    i += 1  # skip its argument
            elif not equals and name not in _KNOWN_LONG_FLAGS:
                # Unknown long option: it may or may not take an argument, and
                # guessing wrong would shift which token is the destination.
                return positional, target, False
        elif token.startswith("-") and token != "-":
            if token in _TARGET_OPTS:
                if i + 1 < len(tokens):
                    target = tokens[i + 1]
                    i += 1
                else:
                    return positional, target, False
            elif token in _ARG_OPTS:
                i += 1  # skip its argument
            elif token.endswith(("t", "S")):
                # Clustered form such as -rt DIR / -bS .bak
                if i + 1 >= len(tokens):
                    return positional, target, False
                if token.endswith("t"):
                    target = tokens[i + 1]
                i += 1
        else:
            positional.append(token)

        i += 1

    return positional, target, True


# Long options that never take a separate argument. Anything else makes the
# handler bail out to `ask` rather than risk misreading the destination.
_KNOWN_LONG_FLAGS = frozenset(
    {
        "--archive",
        "--attributes-only",
        "--backup",
        "--copy-contents",
        "--debug",
        "--dereference",
        "--exchange",
        "--force",
        "--help",
        "--interactive",
        "--link",
        "--no-clobber",
        "--no-dereference",
        "--no-target-directory",
        "--one-file-system",
        "--parents",
        "--preserve",
        "--recursive",
        "--reflink",
        "--remove-destination",
        "--sparse",
        "--strip-trailing-slashes",
        "--symbolic-link",
        "--update",
        "--verbose",
        "--version",
    }
)


def classify(ctx: HandlerContext) -> Classification:
    """Report the paths cp/mv writes so redirect rules can decide."""
    tokens = ctx.tokens
    base = tokens[0] if tokens else "cp"

    positional, target, parsed_ok = _parse(tokens)
    if not parsed_ok:
        return Classification("ask", description=base)

    if target is not None:
        sources = positional
        destination, into_directory = target, True
    else:
        if len(positional) < 2:
            # Nothing to copy, or no destination — let the user look at it.
            return Classification("ask", description=base)
        sources = positional[:-1]
        destination = positional[-1]
        # A trailing slash is the only way to know a destination is a directory
        # without touching the filesystem. Without one, the destination path is
        # used as written — conservative, since an unmatched target asks.
        into_directory = destination.endswith("/") or destination in (".", "..")

    if not sources:
        return Classification("ask", description=base)

    if into_directory:
        prefix = destination.rstrip("/") or "/"
        targets = [f"{prefix}/{_basename(src)}" for src in sources]
    else:
        targets = [destination]

    # mv unlinks its sources, so those paths are written as well.
    if base == "mv":
        targets = targets + sources

    # No custom description: the default "cp SRC" / "mv SRC" wording is what
    # users already read in approval prompts and audit entries.
    return Classification("allow", redirect_targets=tuple(targets))
