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


READ_TOOLS = {
    "view_file",
    "read_file",
    "Read",
    "read",
    "read_many_files",
    "LS",
    "Glob",
    "Grep",
    "Search",
}
EDIT_TOOLS = {
    "write_to_file",
    "replace_file_content",
    "Write",
    "Edit",
    "MultiEdit",
    "write_file",
    "replace",
}
WEB_TOOLS = {
    "search_web",
    "read_url_content",
    "WebSearch",
    "WebFetch",
    "google_web_search",
    "web_fetch",
}
MCP_TOOLS = {"call_mcp_tool"}


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
    value.setdefault("tool", os.environ.get("DIPPY_TOOL") or None)
    value.setdefault("rule", os.environ.get("DIPPY_RULE") or None)
    value.setdefault("message", os.environ.get("DIPPY_MESSAGE") or None)
    value.setdefault("file_path", os.environ.get("DIPPY_FILE_PATH") or None)
    if "reason" not in value:
        value["reason"] = value.get("message") or value.get("rule") or ""
    return value


def _get_monitors() -> list[dict[str, int]]:
    """Detect monitor geometries via xrandr."""
    import re
    import subprocess

    try:
        out = subprocess.check_output(
            ["xrandr", "--listmonitors"],
            text=True,
            timeout=1,
            stderr=subprocess.DEVNULL,
        )
        monitors = []
        for line in out.splitlines():
            m = re.search(r"(\d+)/\d+x(\d+)/\d+\+(\d+)\+(\d+)", line)
            if m:
                w, h, x, y = map(int, m.groups())
                monitors.append({"w": w, "h": h, "x": x, "y": y})
        return monitors
    except Exception:
        return []


def _center_window(root: tk.Tk) -> None:
    """Center window in the middle of the active monitor where the pointer is."""
    try:
        if hasattr(root, "update_idletasks"):
            root.update_idletasks()
        w = root.winfo_reqwidth() if hasattr(root, "winfo_reqwidth") else 600
        h = root.winfo_reqheight() if hasattr(root, "winfo_reqheight") else 350

        px = root.winfo_pointerx() if hasattr(root, "winfo_pointerx") else 0
        py = root.winfo_pointery() if hasattr(root, "winfo_pointery") else 0

        monitors = _get_monitors()
        active_mon = None
        for mon in monitors:
            if (
                mon["x"] <= px < mon["x"] + mon["w"]
                and mon["y"] <= py < mon["y"] + mon["h"]
            ):
                active_mon = mon
                break

        if active_mon:
            x = active_mon["x"] + max(0, (active_mon["w"] - w) // 2)
            y = active_mon["y"] + max(0, (active_mon["h"] - h) // 2)
        else:
            sw = (
                root.winfo_screenwidth() if hasattr(root, "winfo_screenwidth") else 1920
            )
            sh = (
                root.winfo_screenheight()
                if hasattr(root, "winfo_screenheight")
                else 1080
            )
            x = max(0, (sw - w) // 2)
            y = max(0, (sh - h) // 2)

        if hasattr(root, "geometry"):
            root.geometry(f"+{x}+{y}")
        if hasattr(root, "lift"):
            root.lift()
        if hasattr(root, "focus_force"):
            root.focus_force()
    except Exception:
        pass


def show_dialog(payload: dict[str, object]) -> tuple[str, str | None]:
    """Show the modal dialog. Closing the window or timing out means deny."""
    if tk is None or ttk is None:
        raise RuntimeError("Tk is not available in this Python installation")
    result: list[str | None] = ["deny", None]

    tool = str(payload.get("tool") or "").strip()
    file_path = str(payload.get("file_path") or "").strip()
    command = str(payload.get("command") or "").strip()
    reason = str(payload.get("reason") or "").strip()

    if tool in READ_TOOLS or (
        not command and file_path and ("read" in tool.lower() or "view" in tool.lower())
    ):
        op_title = "Read File"
        target_label = "File path"
        target_value = file_path or command
    elif tool in EDIT_TOOLS or (
        not command
        and file_path
        and (
            "write" in tool.lower()
            or "edit" in tool.lower()
            or "replace" in tool.lower()
        )
    ):
        op_title = "Edit File"
        target_label = "File path"
        target_value = file_path or command
    elif tool in WEB_TOOLS:
        op_title = "Web Request"
        target_label = "Query / URL"
        target_value = command
    elif tool in MCP_TOOLS or tool.startswith("mcp__"):
        op_title = "MCP Tool Call"
        target_label = "MCP Tool"
        target_value = command or tool
    else:
        op_title = "Command"
        target_label = "Command"
        target_value = command or file_path

    root = tk.Tk()
    root.title(f"Dippy approval: {op_title}")
    root.attributes("-topmost", True)
    frame = ttk.Frame(root, padding=16)
    frame.grid(sticky="nsew")

    rows_data = [
        ("Operation", op_title),
        (target_label, target_value),
        ("Working directory", payload.get("cwd")),
        ("Server", payload.get("server")),
        ("Rule / Reason", reason),
    ]

    row = 0
    for title, val in rows_data:
        if val:
            ttk.Label(frame, text=title).grid(row=row, column=0, sticky="nw", pady=4)
            is_multiline = title in ("Command", "File path", "Query / URL")
            field = tk.Text(frame, width=72, height=3 if is_multiline else 1)
            field.insert("1.0", str(val))
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
    _center_window(root)
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
