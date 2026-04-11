"""Tests for pytest command handler."""

from __future__ import annotations

import pytest

from conftest import is_approved, needs_confirmation


class TestPytestSafeFlags:
    """Tests for pytest flags that don't execute tests."""

    @pytest.mark.parametrize(
        "cmd",
        [
            "pytest --version",
            "pytest -V",
            "pytest --help",
            "pytest -h",
            "pytest --collect-only",
            "pytest --co",
        ],
    )
    def test_safe_flags_approved(self, check, cmd):
        result = check(cmd)
        assert is_approved(result), f"Expected approve: {cmd}"


class TestPytestExecution:
    """Tests for pytest commands that execute test code."""

    @pytest.mark.parametrize(
        "cmd",
        [
            "pytest",
            "pytest tests/",
            "pytest tests/test_foo.py",
            "pytest -x tests/",
            "pytest -v tests/",
            "pytest -k test_foo",
        ],
    )
    def test_execution_needs_confirmation(self, check, cmd):
        result = check(cmd)
        assert needs_confirmation(result), f"Expected ask: {cmd}"
