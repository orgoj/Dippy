"""
Tests for lazy handler loading optimization (bead bd-mwh).

_discover_handlers() must build the command→module map without
importing every handler module at startup.
"""

from __future__ import annotations

import sys


def test_discover_handlers_does_not_import_handler_modules():
    """_discover_handlers must build mapping via AST scan, not module imports."""
    from dippy.cli import _discover_handlers

    # Save and clear all dippy.cli.* handler modules (keep only the package)
    saved = {k: v for k, v in sys.modules.items() if k.startswith("dippy.cli.")}
    for k in list(saved):
        del sys.modules[k]

    try:
        # At this point: no dippy.cli.* modules in sys.modules (verified below)
        pre_modules = {k for k in sys.modules if k.startswith("dippy.cli.")}
        assert not pre_modules, f"Expected clean slate, found: {pre_modules}"

        handlers = _discover_handlers()

        # After call: no handler modules should have been imported
        post_modules = {k for k in sys.modules if k.startswith("dippy.cli.")}
        assert not post_modules, (
            f"_discover_handlers should use AST scan, not module imports.\n"
            f"Unexpectedly imported: {post_modules}"
        )
        # Mapping must still be populated
        assert len(handlers) > 20, "Handler mapping should have >20 entries"
    finally:
        sys.modules.update(saved)


def test_known_handlers_maps_common_commands():
    """KNOWN_HANDLERS must map well-known commands to their modules."""
    from dippy.cli import KNOWN_HANDLERS

    assert KNOWN_HANDLERS.get("python") == "python"
    assert KNOWN_HANDLERS.get("python3") == "python"
    assert KNOWN_HANDLERS.get("git") == "git"
    assert KNOWN_HANDLERS.get("docker") == "docker"
    assert KNOWN_HANDLERS.get("aws") == "aws"


def test_get_handler_still_works_after_lazy_loading():
    """get_handler must still return a callable handler after the refactor."""
    from dippy.cli import HandlerContext, get_handler

    handler = get_handler("git")
    assert handler is not None
    ctx = HandlerContext(tokens=["git", "status"])
    result = handler.classify(ctx)
    assert result.action in ("allow", "ask", "delegate")
