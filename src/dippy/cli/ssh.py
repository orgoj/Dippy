"""
SSH command handler for Dippy.

Handles ssh with remote command execution.
Delegates to inner command check with `ssh` and exact target wrapper contexts.
"""

from dippy.cli import Classification, HandlerContext

COMMANDS = ["ssh"]

OPTIONS_WITH_ARG = {
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

# These options can execute a local helper, write a file, create forwarding,
# or manipulate a persistent control connection. They must not inherit an
# allow decision from an otherwise read-only remote command.
RISKY_OPTIONS_WITH_ARG = {
    "-D",
    "-E",
    "-F",
    "-I",
    "-J",
    "-L",
    "-O",
    "-R",
    "-S",
    "-W",
    "-o",
    "-w",
}
RISKY_FLAGS = {"-A", "-f", "-K", "-M", "-N", "-X", "-Y"}


def _risky_option(token: str) -> str | None:
    if not token.startswith("-") or token.startswith("--"):
        return None

    # OpenSSH accepts clusters such as -vA and -vL8080:host:80. Walk each
    # option until one consumes the rest of the token as its argument.
    cluster = token[1:]
    for char in cluster:
        option = f"-{char}"
        if option in RISKY_FLAGS or option in RISKY_OPTIONS_WITH_ARG:
            return option
        if option in OPTIONS_WITH_ARG:
            break
    return None


def classify(ctx: HandlerContext) -> Classification:
    """Classify ssh command.

    SSH command forms:
    - ssh host                        # Interactive - ask
    - ssh host command args...        # Remote command - delegate
    - ssh -t host command args...     # With options - delegate
    - ssh user@host "command"         # Quoted command - delegate
    """
    tokens = ctx.tokens
    if len(tokens) < 2:
        return Classification("ask", description="ssh (no target)")

    # Find the host argument (skip options like -p, -i, -l, etc.)
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
            risky = _risky_option(tok)
            if risky:
                return Classification("ask", description=f"ssh option {risky}")
            if tok in OPTIONS_WITH_ARG:
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

    # Join remaining tokens as the remote command. Unlike sudo or env, ssh
    # concatenates its arguments with spaces and hands the result to a remote
    # shell, so the metacharacters are syntax there. Do not use bash_join() —
    # re-quoting would hide a remote compound command from analysis.
    remote_cmd = " ".join(tokens[i:])

    return Classification(
        "delegate",
        inner_command=remote_cmd,
        description=f"ssh {host}",
        wrapper_context=["ssh", host],
        remote=True,
    )
