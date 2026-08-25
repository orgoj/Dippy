"""Tests for fail-closed GUI approval behavior without a display server."""

from types import SimpleNamespace

import pytest

from dippy import askpass_gui


class FakeWidget:
    def __init__(self, *args, **kwargs):
        self.value = ""

    def grid(self, *args, **kwargs):
        return None

    def insert(self, index, value):
        self.value = value

    def configure(self, **kwargs):
        return None

    def get(self):
        return self.value


class FakeRoot(FakeWidget):
    def __init__(self):
        super().__init__()
        self.close = None

    def title(self, value):
        return None

    def attributes(self, *args):
        return None

    def protocol(self, name, callback):
        self.close = callback

    def after(self, milliseconds, callback):
        pytest.fail("GUI must not own the approval timeout")

    def mainloop(self):
        self.close()

    def destroy(self):
        return None


def test_close_denies_without_scheduling_timeout(monkeypatch):
    root = FakeRoot()
    monkeypatch.setattr(
        askpass_gui, "tk", SimpleNamespace(Tk=lambda: root, Text=FakeWidget)
    )
    monkeypatch.setattr(
        askpass_gui,
        "ttk",
        SimpleNamespace(
            Frame=FakeWidget,
            Label=FakeWidget,
            Entry=FakeWidget,
            Button=FakeWidget,
        ),
    )
    assert askpass_gui.show_dialog({"command": "frob"}) == ("deny", None)


def test_dialog_displays_working_directory(monkeypatch):
    root = FakeRoot()
    fields = []

    class RecordingWidget(FakeWidget):
        def insert(self, index, value):
            super().insert(index, value)
            fields.append(value)

    monkeypatch.setattr(
        askpass_gui, "tk", SimpleNamespace(Tk=lambda: root, Text=RecordingWidget)
    )
    monkeypatch.setattr(
        askpass_gui,
        "ttk",
        SimpleNamespace(
            Frame=FakeWidget,
            Label=FakeWidget,
            Entry=FakeWidget,
            Button=FakeWidget,
        ),
    )

    askpass_gui.show_dialog({"command": "frob", "cwd": "/work/project"})

    assert "/work/project" in fields
