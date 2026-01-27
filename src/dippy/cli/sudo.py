"""
Sudo command handler for Dippy.

Handles sudo with command execution.
Delegates to inner command check with 'sudo' wrapper context.
"""

from dippy.cli import Classification, HandlerContext

COMMANDS = ["sudo", "doas", "pkexec"]


def classify(ctx: HandlerContext) -> Classification:
    """Classify sudo command.

    Sudo command forms:
    - sudo                            # Interactive - ask
    - sudo command args...            # Run command - delegate
    - sudo -u user command args...    # With user - delegate
    - sudo -i                         # Interactive shell - ask
    - sudo -s                         # Shell - ask
    """
    tokens = ctx.tokens
    base = tokens[0] if tokens else "sudo"
    if len(tokens) < 2:
        return Classification("ask", description=f"{base} (no command)")

    # Options that take an argument
    opts_with_arg = {
        "-C",
        "-D",
        "-g",
        "-h",
        "-p",
        "-R",
        "-r",
        "-T",
        "-t",
        "-U",
        "-u",
    }

    # Options that mean interactive/shell mode
    interactive_opts = {"-i", "-s", "--shell", "--login"}

    i = 1
    while i < len(tokens):
        tok = tokens[i]
        if tok == "--":
            # Everything after -- is the command
            i += 1
            break
        elif tok in interactive_opts:
            # Interactive shell mode
            return Classification("ask", description=f"{base} {tok}")
        elif tok.startswith("-"):
            if tok in opts_with_arg:
                # Skip option and its argument
                i += 2
            else:
                # Flag without argument (like -n, -v, -k)
                i += 1
        else:
            # First non-option is the command
            break

    if i >= len(tokens):
        return Classification("ask", description=f"{base} (no command)")

    # Join remaining tokens as the command
    inner_cmd = " ".join(tokens[i:])

    return Classification(
        "delegate",
        inner_command=inner_cmd,
        description=base,
        wrapper_context=["sudo"],
    )
