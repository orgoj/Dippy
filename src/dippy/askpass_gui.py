"""Tk approval provider for ``dippy run`` and ``run-on-server``."""

from __future__ import annotations

import json
import os
import sys

try:
    import tkinter as tk
    from tkinter import ttk
except ModuleNotFoundError:
    tk = None
    ttk = None


def _payload() -> dict[str, object]:
    try:
        value = json.load(sys.stdin)
    except (json.JSONDecodeError, OSError):
        value = {}
    if not isinstance(value, dict):
        value = {}
    value.setdefault("command", os.environ.get("DIPPY_COMMAND", ""))
    value.setdefault("cwd", os.environ.get("DIPPY_CWD") or None)
    value.setdefault("server", os.environ.get("DIPPY_SERVER") or None)
    value.setdefault("reason", os.environ.get("DIPPY_RULE", ""))
    return value


def show_dialog(payload: dict[str, object]) -> tuple[str, str | None]:
    """Show the modal dialog. Closing the window or timing out means deny."""
    if tk is None or ttk is None:
        raise RuntimeError("Tk is not available in this Python installation")
    result: list[str | None] = ["deny", None]
    root = tk.Tk()
    root.title("Dippy command approval")
    root.attributes("-topmost", True)
    frame = ttk.Frame(root, padding=16)
    frame.grid(sticky="nsew")

    row = 0
    for title, key in (
        ("Command", "command"),
        ("Working directory", "cwd"),
        ("Server", "server"),
        ("Rule", "reason"),
    ):
        value = payload.get(key)
        if value:
            ttk.Label(frame, text=title).grid(row=row, column=0, sticky="nw", pady=4)
            field = tk.Text(frame, width=72, height=4 if key == "command" else 1)
            field.insert("1.0", str(value))
            field.configure(state="disabled")
            field.grid(row=row, column=1, sticky="ew", pady=4)
            row += 1

    ttk.Label(frame, text="Note").grid(row=row, column=0, sticky="w", pady=4)
    note = ttk.Entry(frame, width=72)
    note.grid(row=row, column=1, sticky="ew", pady=4)
    row += 1

    def finish(decision: str) -> None:
        result[0] = decision
        result[1] = note.get().strip() or None
        root.destroy()

    buttons = ttk.Frame(frame)
    buttons.grid(row=row, column=1, sticky="e", pady=(12, 0))
    ttk.Button(buttons, text="Deny", command=lambda: finish("deny")).grid(
        row=0, column=0, padx=4
    )
    ttk.Button(buttons, text="Allow", command=lambda: finish("allow")).grid(
        row=0, column=1, padx=4
    )
    root.protocol("WM_DELETE_WINDOW", lambda: finish("deny"))
    root.mainloop()
    return str(result[0]), result[1]


def main() -> None:
    if tk is None and os.path.exists("/usr/bin/python3"):
        current = os.path.realpath(sys.executable)
        fallback = os.path.realpath("/usr/bin/python3")
        if current != fallback:
            os.execv(fallback, [fallback, os.path.realpath(__file__), *sys.argv[1:]])
    try:
        decision, note = show_dialog(_payload())
    except Exception as error:
        print(json.dumps({"decision": "deny", "note": f"GUI error: {error}"}))
        raise SystemExit(1) from None
    response = {"decision": decision}
    if note:
        response["note"] = note
    print(json.dumps(response))
    raise SystemExit(0 if decision == "allow" else 1)


if __name__ == "__main__":
    main()
