"""
SSH command handler for Dippy.

Handles ssh with remote command execution.
Delegates to inner command check with 'ssh' wrapper context.
"""

from pathlib import Path

from dippy.cli import Classification, HandlerContext

COMMANDS = ["ssh"]


def classify(ctx: HandlerContext) -> Classification:
    """Classify ssh command.

    SSH command forms:
    - ssh host                        # Interactive - ask
    - ssh host command args...        # Remote command - delegate
    - ssh -t host command args...     # With options - delegate
    - ssh user@host "command"         # Quoted command - delegate
    """
    if len(tokens) < 2:
        return Classification("ask", description="ssh (no target)")

    # Find the host argument (skip options like -p, -i, -l, etc.)
    # SSH options that take an argument
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
    }

    i = 1
    host = None
    while i < len(tokens):
        tok = tokens[i]
        if tok == "--":
            # End of options - next non-option is host (if not found yet)
            i += 1
            continue
        elif tok.startswith("-") and host is None:
            # Only parse options before we find the host
            if tok in opts_with_arg:
                # Skip option and its argument
                i += 2
            else:
                # Flag without argument (like -t, -v, -N)
                i += 1
        else:
            # First non-option is the host
            if host is None:
                host = tok
                i += 1
            else:
                # Already have host, rest is the command
                break

    if not host:
        return Classification("ask", description="ssh (no target)")

    # Everything after host is the remote command
    if i >= len(tokens):
        # No command - interactive session
        return Classification("ask", description=f"ssh {host}")

    # Skip any -- that appears between host and command
    if i < len(tokens) and tokens[i] == "--":
        i += 1

    if i >= len(tokens):
        # No command after --
        return Classification("ask", description=f"ssh {host}")

    # Join remaining tokens as the remote command
    remote_cmd = " ".join(tokens[i:])

    return Classification(
        "delegate",
        inner_command=remote_cmd,
        description=f"ssh {host}",
        wrapper_context=["ssh"],
    )
