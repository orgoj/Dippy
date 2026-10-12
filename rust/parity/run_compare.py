#!/usr/bin/env python3
"""Compare `dippy run`, `run-on-server` and `recover` of Python Dippy and dippy-rs.

Each scenario runs once per implementation in the same fresh sandbox: an
empty HOME, a project directory as cwd with a `.dippy`, and a `bin`
directory first on PATH with fake `ssh`, `tmux`, `herdr` and askpass
programs. Nothing connects anywhere: the fake `ssh` runs `bash -s` locally
and the fake terminals run what is typed into a pane with local Bash and
append it to a log that `capture-pane`/`pane read` return. Every step
compares exit code, stdout and stderr; random markers (32 hex digits) and
audit timestamps are normalized. After the scenario the sandbox tree (run
state, locks, audit log, pane logs) must be identical, except that without
`set log` dippy-rs writes `~/.dippy/audit.log` (intended).

Python divergences fixed in dippy-rs (tracebacks, `b'...'` stderr of failed
tmux commands, a corrupt state file's JSON error wording) are covered by
Rust unit tests, not here. The askpass payload is compared without the keys
only dippy-rs sends (`flags`, `remote`, `causes`), which must be present.

Usage: run_compare.py [SCENARIO-SUBSTRING]
"""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

from hook_compare import PY, RS  # noqa: E402
from parity_env import ROOT  # noqa: E402

from dippy.execution import _operation_key  # noqa: E402

BOX = ROOT / "run"
HOME = BOX / "home"
PROJECT = BOX / "project"
BIN = BOX / "bin"
STATE = ".dippy/run-state"

MARKER = re.compile(r"(?<![0-9a-f])[0-9a-f]{32}(?![0-9a-f])")
# A pane log shows the typed transport, whose base64 encodes the markers.
ENCODED = re.compile(r"printf %s [A-Za-z0-9+/=]{40,}")
TS = re.compile(r'"ts": "[^"]*"')
# Askpass payload keys dippy-rs adds for its dialog (rule causes and tags).
RUST_ONLY = ("flags", "remote", "causes")

FAKE_SSH = r"""#!/bin/bash
# -G prints the effective profile; otherwise run the remote Bash locally.
for arg in "$@"; do
  if [ "$arg" = -G ]; then
    printf '%s\n' "${FAKE_SSH_G:-identityfile none}"
    exit "${FAKE_SSH_G_EXIT:-0}"
  fi
done
[ -n "$FAKE_SSH_EXIT" ] && exit "$FAKE_SSH_EXIT"
echo "ssh-auth-sock=${SSH_AUTH_SOCK-unset}" >&2
exec bash -s
"""

FAKE_TMUX = r"""#!/bin/bash
dir="$HOME/../tmux"
mkdir -p "$dir/sessions" "$dir/panes"
new_pane() {
  n=$(( $(cat "$dir/count" 2>/dev/null || echo 0) + 1 ))
  echo "$n" > "$dir/count"
  echo "%$n"
}
case "$1" in
  has-session) [ -e "$dir/sessions/${3#=}" ] ;;
  new-session) touch "$dir/sessions/$4"; new_pane ;;
  new-window) new_pane ;;
  send-keys)
    pane="$dir/panes/$3"
    if [ "$4" = -l ]; then printf '%s' "$5" > "$pane.pending"; exit 0; fi
    text=$(cat "$pane.pending")
    if [ -n "$FAKE_TMUX_FAIL" ]; then echo "tmux: $FAKE_TMUX_FAIL" >&2; exit 1; fi
    { printf '$ %s\n' "$text"; bash -c "$text" 2>&1; } >> "$pane.log"
    ;;
  capture-pane) cat "$dir/panes/$6.log" 2>/dev/null ;;
  *) echo "fake tmux: $*" >&2; exit 1 ;;
esac
"""

FAKE_HERDR = r"""#!/bin/bash
dir="$HOME/../herdr"
mkdir -p "$dir"
shift 2  # --session NAME
case "$1 $2" in
  "workspace create") echo '{"result": {"root_pane": {"pane_id": "w1:p1"}}}' ;;
  "pane run")
    case "$4" in
      "exec /bin/bash"*) ;;
      *) { printf '$ %s\n' "$4"; bash -c "$4" 2>&1; } >> "$dir/$3.log" ;;
    esac
    echo '{"result": {}}'
    ;;
  "pane read") cat "$dir/$3.log" 2>/dev/null ;;
  *) echo "fake herdr: $*" >&2; exit 1 ;;
esac
"""

ASKPASS = {
    "askpass-allow": 'cat > "$HOME/../askpass-input"; '
    'printf "%s|%s|%s\\n" "$1" "$DIPPY_TOOL" "$DIPPY_SERVER" >> "$HOME/../askpass-args"; '
    'echo \'{"decision": "allow", "note": "looks fine"}\'',
    "askpass-deny": 'echo \'{"decision": "deny", "note": "not now"}\'; exit 1',
    "askpass-exit1": "exit 1",
    "askpass-invalid": 'echo \'{"decision": "maybe"}\'',
    "askpass-silent": "exit 0",
    "askpass-slow": "sleep 5",
}

BASE = (
    f"set log {HOME}/audit.log\nset log-full\nserver srv\n"
    "allow printf *\nallow fictional-ok *\nallow-redirect out.txt\n"
)


def config(extra: str = "") -> str:
    return BASE + extra


def step(*args: str, stdin: str | None = None, **env: str) -> dict:
    return {"args": list(args), "stdin": stdin, "env": env}


KEY_STATE = f"home/{STATE}/{_operation_key(PROJECT, 'srv')}.json"
PENDING_TMUX = (
    '{"backend": "tmux", "marker": "' + "a" * 32 + '", "profile": "user", '
    '"session": "dippy", "status": "indeterminate", "tmux_pane": "%7"}'
)
PANE_LOG = "$ something\nremote out\nDIPPY_END_" + "a" * 32 + ":9\n"
PENDING_HERDR = PENDING_TMUX.replace('"tmux"', '"herdr"').replace(
    '"tmux_pane": "%7"', '"herdr_pane": "w9:p9"'
)

SCENARIOS = [
    (
        "local allow, deny, ask",
        {"project/.dippy": config("deny fictional-bad *\n")},
        [
            step("run", "printf '%s\\n' local; fictional-ok 3"),
            step("run", "printf 'a b' | cat > out.txt"),
            step("run", "fictional-bad --erase /x"),
            step("run", "fictional-unknown --go"),
            step(
                "run",
                "fictional-unknown --go",
                DIPPY_ASKPASS=str(BIN / "askpass-allow"),
            ),
            step(
                "run", "fictional-unknown --go", DIPPY_ASKPASS=str(BIN / "askpass-deny")
            ),
            step(
                "run",
                "fictional-unknown --go",
                DIPPY_ASKPASS=str(BIN / "askpass-exit1"),
            ),
            step(
                "run", "fictional-unknown", DIPPY_ASKPASS=str(BIN / "askpass-invalid")
            ),
            step("run", "fictional-unknown", DIPPY_ASKPASS=str(BIN / "askpass-silent")),
            step("run", "fictional-unknown", DIPPY_ASKPASS=str(BIN / "missing")),
            step("run", "exit 5"),
        ],
    ),
    (
        "askpass from config, timeout, wait message",
        {
            "project/.dippy": config(
                f"set askpass {BIN}/askpass-slow\nset askpass-timeout 1\n"
                "set approval-wait-message Ask the supervisor.\n"
            )
        },
        [step("run", "fictional-unknown")],
    ),
    (
        "command from stdin, empty command, cwd and config options",
        {
            "project/.dippy": config(),
            "project/sub/.keep": "",
            "other.dippy": "allow fictional-other\n",
        },
        [
            step("run", stdin="printf 'one\\n'\nprintf 'two\\n'\n"),
            step("run", stdin="  \n"),
            step("run", ""),
            step("--cwd", "sub", "run", "printf '%s\\n' \"$PWD\""),
            step("--config", str(BOX / "other.dippy"), "run", "fictional-other"),
            step("--config-only", str(BOX / "other.dippy"), "run", "printf x"),
            step("--config-only", str(BOX / "missing.dippy"), "run", "printf x"),
        ],
    ),
    (
        "run-on-server validation and decisions",
        {
            "project/.dippy": config(
                "server other\ndeny [run-on-server] fictional-erase *\n"
                "ask [srv] fictional-ok --careful\n"
            )
        },
        [
            step("run-on-server", "user@host", "printf x"),
            step("run-on-server", "nope", "printf x"),
            step("run-on-server", "srv", "fictional-erase /x"),
            step("run-on-server", "srv", "fictional-ok --careful"),
            step(
                "run-on-server",
                "srv",
                "fictional-ok --careful",
                DIPPY_ASKPASS=str(BIN / "askpass-allow"),
            ),
            step("run-on-server", "srv", stdin="printf 'remote %s\\n' \"$0\"\n"),
            step("run-on-server", "srv", "printf x; exit 7"),
            step("run-on-server", "other", "printf other"),
        ],
    ),
    (
        "ssh backend: lost connection is indeterminate until recovered",
        {"project/.dippy": config()},
        [
            step("run-on-server", "srv", "printf lost", FAKE_SSH_EXIT="255"),
            step("run-on-server", "srv", "printf blocked"),
            step("recover", "srv"),
            step("recover", "srv", "--clear"),
            step("recover", "srv"),
            step("run-on-server", "srv", "printf again"),
        ],
    ),
    (
        "tmux backend",
        {"project/.dippy": config("set run-on-server-backend tmux\n")},
        [
            step("run-on-server", "srv", "printf 'remote\\n'; exit 4"),
            step("run-on-server", "srv", "printf 'no newline'"),
            step("recover", "srv"),
        ],
    ),
    (
        "tmux failures",
        {
            "project/.dippy": config(
                "set run-on-server-backend tmux\n"
                "set run-on-server-timeout 1\nset run-on-server-poll-interval 0.05\n"
            )
        },
        [
            step("run-on-server", "srv", "printf x", FAKE_SSH_EXIT="255"),
            step("recover", "srv", "--clear"),
            # No tmux at all: a PATH with no programs, never the real tmux.
            step("run-on-server", "srv", "printf x", PATH=str(BOX / "empty")),
            step("recover", "srv", "--clear"),
        ],
    ),
    (
        "invalid session and unsupported state",
        {"project/.dippy": config("set run-on-server-backend tmux\n")},
        [
            step(
                "--config",
                str(BOX / "session.dippy"),
                "run-on-server",
                "srv",
                "printf x",
            ),
            step("recover", "srv"),
        ],
    ),
    (
        "herdr backend",
        {"project/.dippy": config("set run-on-server-backend herdr\n")},
        [
            step("run-on-server", "srv", "printf 'herdr remote\\n'; exit 6"),
            step("recover", "srv"),
        ],
    ),
    (
        "recover tmux and herdr markers",
        {
            "project/.dippy": config("set run-on-server-backend tmux\n"),
            "herdr.dippy": "set run-on-server-backend herdr\n",
            KEY_STATE: PENDING_TMUX,
            "tmux/panes/%7.log": PANE_LOG,
        },
        [
            step("--config", str(BOX / "herdr.dippy"), "recover", "srv"),
            step("recover", "srv"),
            step("recover", "srv"),
        ],
    ),
    (
        "recover herdr without output",
        {
            "project/.dippy": config("set run-on-server-backend herdr\n"),
            KEY_STATE: PENDING_HERDR,
        },
        [step("recover", "srv"), step("recover", "user@x"), step("recover", "nope")],
    ),
    (
        "legacy alias state",
        {
            "project/.dippy": config(),
            f"home/{STATE}/srv.json": '{"status": "running"}',
        },
        [
            step("run-on-server", "srv", "printf x"),
            step("recover", "srv"),
            step("recover", "srv", "--clear"),
            step("run-on-server", "srv", "printf x"),
        ],
    ),
    (
        "invalid state file",
        {
            "project/.dippy": config(),
            KEY_STATE: '{"status": "odd"}',
        },
        [step("run-on-server", "srv", "printf x"), step("recover", "srv")],
    ),
    (
        "isolated ssh profile",
        {
            "project/.dippy": config(
                "set run-on-server-ssh-config ssh-profile\n"
                "set run-on-server-ssh-auth-sock none\n"
            ),
            "project/ssh-profile": "Host *\n IdentityFile /project/key.pub\n",
        },
        [
            step(
                "run-on-server",
                "srv",
                "printf profile",
                FAKE_SSH_G="identityfile none\nidentityfile /project/key.pub",
                SSH_AUTH_SOCK="/user/agent",
            ),
            step("run-on-server", "srv", "printf x", FAKE_SSH_G="identityfile none"),
            step(
                "run-on-server",
                "srv",
                "printf x",
                FAKE_SSH_G="identityfile /k\nproxyjump bastion",
            ),
            step("run-on-server", "srv", "printf x", FAKE_SSH_G_EXIT="1"),
        ],
    ),
    (
        "incomplete or missing profile",
        {
            "project/.dippy": config("set run-on-server-ssh-config missing\n"),
            "both.dippy": "set run-on-server-ssh-auth-sock none\n",
            "sock.dippy": f"set run-on-server-ssh-config {PROJECT}/ssh-profile\n"
            f"set run-on-server-ssh-auth-sock {BOX}/nosock\n",
            "project/ssh-profile": "Host *\n",
        },
        [
            step("run-on-server", "srv", "printf x"),
            step(
                "--config", str(BOX / "both.dippy"), "run-on-server", "srv", "printf x"
            ),
            step(
                "--config", str(BOX / "sock.dippy"), "run-on-server", "srv", "printf x"
            ),
        ],
    ),
]

# Written after SCENARIOS reference it by path only.
EXTRA_FILES = {"session.dippy": "set run-on-server-session bad:name\n"}


def env(extra: dict[str, str]) -> dict[str, str]:
    home = str(HOME)
    assert home.startswith(str(BOX)), "sandbox HOME must stay inside the box"
    result = {
        "HOME": home,
        "PATH": f"{BIN}:" + os.environ.get("PATH", "/usr/bin:/bin"),
        "LANG": "C.UTF-8",
        "TMPDIR": str(BOX / "tmp"),
        # A real tmux, if ever reached, must not touch the user's server.
        "TMUX_TMPDIR": str(BOX / "tmp"),
    }
    result.update(extra)
    return result


def normalize(text: str) -> str:
    text = ENCODED.sub("printf %s <BASE64>", text)
    return TS.sub('"ts": "TS"', MARKER.sub("<MARKER>", text))


def run(binary: str, s: dict) -> tuple[int, str, str]:
    proc = subprocess.run(
        [binary, *s["args"]],
        cwd=PROJECT,
        env=env(s["env"]),
        input=s["stdin"] or "",
        capture_output=True,
        text=True,
        timeout=30,
    )
    return proc.returncode, normalize(proc.stdout), normalize(proc.stderr)


def askpass_input(binary: str, text: str) -> str:
    """The askpass payload without the keys only dippy-rs sends."""
    payload = json.loads(text)
    extra = {key: payload.pop(key, None) for key in RUST_ONLY}
    if binary == RS:
        assert None not in extra.values(), f"dippy-rs payload lacks {RUST_ONLY}"
    return json.dumps(payload)


def tree(binary: str) -> list[tuple[str, str, str]]:
    result = []
    for path in sorted(BOX.rglob("*")):
        rel = normalize(str(path.relative_to(BOX)))
        if rel.startswith(("bin", "tmp")):
            continue
        mode = oct(path.lstat().st_mode & 0o777)
        content = "<dir>" if path.is_dir() else normalize(path.read_text())
        if rel == "askpass-input":
            content = askpass_input(binary, content)
        result.append((rel, content, mode))
    return result


def play(binary: str, seed: dict[str, str], steps: list[dict]):
    if BOX.exists():
        shutil.rmtree(BOX)
    for directory in (PROJECT, HOME, BIN, BOX / "tmp"):
        directory.mkdir(parents=True)
    programs = {"ssh": FAKE_SSH, "tmux": FAKE_TMUX, "herdr": FAKE_HERDR}
    programs.update({name: f"#!/bin/bash\n{body}\n" for name, body in ASKPASS.items()})
    for name, text in programs.items():
        (BIN / name).write_text(text)
        (BIN / name).chmod(0o755)
    for rel, text in {**EXTRA_FILES, **seed}.items():
        path = BOX / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
    outputs = [run(binary, s) for s in steps]
    return outputs, tree(binary)


def main() -> int:
    differ = total = 0
    only = sys.argv[1] if len(sys.argv) > 1 else ""
    for name, seed, steps in SCENARIOS:
        if only not in name:
            continue
        py_out, py_tree = play(PY, seed, steps)
        rs_out, rs_tree = play(RS, seed, steps)
        for s, py, rs in zip(steps, py_out, rs_out):
            total += 1
            label = " ".join(s["args"])
            if only:
                print(f"  [{name}] {label}: {rs}")
            if "Traceback" in py[2]:
                differ += 1
                print(f"  [{name}] {label}: Python crashed\n{py[2]}")
            # Intended: dippy-rs words a config error as a request to stop.
            stop = py[2].replace("config error: ", "Dippy config error: ", 1).rstrip(
                "\n"
            ) + (
                ". Dippy blocks every tool call until the configuration is fixed: "
                "stop work and ask the user to fix it.\n"
            )
            if py != rs and (py[0], py[1], stop) != rs:
                differ += 1
                print(f"  [{name}] {label}\n     py: {py}\n     rs: {rs}")
        # Intended: without `set log` dippy-rs logs to ~/.dippy/audit.log.
        if not any(e[0].startswith("home/.dippy") for e in py_tree):
            rs_tree = [e for e in rs_tree if not e[0].startswith("home/.dippy")]
        if py_tree != rs_tree:
            differ += 1
            py_only = [e for e in py_tree if e not in rs_tree]
            rs_only = [e for e in rs_tree if e not in py_tree]
            print(f"  [{name}] files\n     py: {py_only}\n     rs: {rs_only}")
            if only:
                print(f"     py tree: {py_tree}\n     rs tree: {rs_tree}")
    shutil.rmtree(BOX, ignore_errors=True)
    print(f"run: {len(SCENARIOS)} scenarios, {total} steps, {differ} differ")
    return 1 if differ else 0


if __name__ == "__main__":
    sys.exit(main())
