"""Tests for logging enhancements - context flags in audit log."""

import json
import os
import tempfile
from pathlib import Path

from dippy.core.config import parse_config, configure_logging, log_decision


def test_log_decision_with_context_flags():
    """Test that log_decision includes context_flags in audit log."""
    config = parse_config("wrapper wrap")

    # Create temp file for logging
    with tempfile.NamedTemporaryFile(mode='w', delete=False, suffix='.log') as f:
        log_path = Path(f.name)

    try:
        # Enable logging for tests
        old_val = os.environ.get("DIPPY_TEST_NO_LOG")
        os.environ.pop("DIPPY_TEST_NO_LOG", None)

        try:
            # Configure logging to temp file
            from dippy.core.config import Config
            test_config = Config(log=log_path, log_full=True)
            configure_logging(test_config)

            # Log decision with context flags
            log_decision(
                "allow",
                "free",
                rule="free *",
                command="wrap server1 free -h",
                context_flags=frozenset(["wrap", "server1"]),
            )

            # Read and verify log file
            log_content = log_path.read_text()
            entries = [json.loads(line) for line in log_content.strip().split('\n') if line.strip()]

            assert len(entries) == 1
            entry = entries[0]

            assert entry["decision"] == "allow"
            assert entry["cmd"] == "free"
            assert entry["rule"] == "free *"
            assert entry["command"] == "wrap server1 free -h"
            assert "context_flags" in entry
            assert sorted(entry["context_flags"]) == ["server1", "wrap"]

        finally:
            # Restore old value
            if old_val is not None:
                os.environ["DIPPY_TEST_NO_LOG"] = old_val

    finally:
        # Clean up temp file
        log_path.unlink(missing_ok=True)


def test_log_decision_without_context_flags():
    """Test that log_decision works without context_flags."""
    with tempfile.NamedTemporaryFile(mode='w', delete=False, suffix='.log') as f:
        log_path = Path(f.name)

    try:
        old_val = os.environ.get("DIPPY_TEST_NO_LOG")
        os.environ.pop("DIPPY_TEST_NO_LOG", None)

        try:
            from dippy.core.config import Config
            test_config = Config(log=log_path)
            configure_logging(test_config)

            # Log decision without context flags
            log_decision(
                "allow",
                "ls",
                rule="ls *",
            )

            # Read and verify log file
            log_content = log_path.read_text()
            entries = [json.loads(line) for line in log_content.strip().split('\n') if line.strip()]

            assert len(entries) == 1
            entry = entries[0]

            assert entry["decision"] == "allow"
            assert entry["cmd"] == "ls"
            assert entry["rule"] == "ls *"
            # context_flags should not be present when None
            assert "context_flags" not in entry

        finally:
            if old_val is not None:
                os.environ["DIPPY_TEST_NO_LOG"] = old_val

    finally:
        log_path.unlink(missing_ok=True)


def test_log_decision_with_empty_context_flags():
    """Test that empty context_flags are not logged."""
    with tempfile.NamedTemporaryFile(mode='w', delete=False, suffix='.log') as f:
        log_path = Path(f.name)

    try:
        old_val = os.environ.get("DIPPY_TEST_NO_LOG")
        os.environ.pop("DIPPY_TEST_NO_LOG", None)

        try:
            from dippy.core.config import Config
            test_config = Config(log=log_path)
            configure_logging(test_config)

            # Log decision with empty context flags
            log_decision(
                "allow",
                "ls",
                rule="ls *",
                context_flags=frozenset(),  # Empty set
            )

            # Read and verify log file
            log_content = log_path.read_text()
            entries = [json.loads(line) for line in log_content.strip().split('\n') if line.strip()]

            assert len(entries) == 1
            entry = entries[0]

            # Empty context_flags should not be in log
            assert "context_flags" not in entry

        finally:
            if old_val is not None:
                os.environ["DIPPY_TEST_NO_LOG"] = old_val

    finally:
        log_path.unlink(missing_ok=True)
