"""Python module allow/deny lists must survive config merging."""

import pytest

from dippy.core.config import _merge_configs, parse_config


class TestPythonModuleMerge:
    """A project config extends the global module lists instead of dropping them."""

    def test_allow_modules_accumulate(self):
        base = parse_config("python-allow-module numpy")
        overlay = parse_config("python-allow-module pandas")
        merged = _merge_configs(base, overlay)
        assert merged.python_allow_modules == ["numpy", "pandas"]

    def test_deny_modules_accumulate(self):
        base = parse_config("python-deny-module requests")
        overlay = parse_config("python-deny-module socket")
        merged = _merge_configs(base, overlay)
        assert merged.python_deny_modules == ["requests", "socket"]

    def test_global_allow_survives_silent_overlay(self):
        """A project config without module directives must not erase the global list."""
        base = parse_config("python-allow-module numpy")
        overlay = parse_config("allow echo *")
        merged = _merge_configs(base, overlay)
        assert merged.python_allow_modules == ["numpy"]

    def test_global_deny_survives_silent_overlay(self):
        """Dropping a deny list on merge would be a security regression."""
        base = parse_config("python-deny-module requests")
        overlay = parse_config("allow echo *")
        merged = _merge_configs(base, overlay)
        assert merged.python_deny_modules == ["requests"]

    def test_empty_base_takes_overlay(self):
        base = parse_config("allow echo *")
        overlay = parse_config("python-allow-module numpy\npython-deny-module socket")
        merged = _merge_configs(base, overlay)
        assert merged.python_allow_modules == ["numpy"]
        assert merged.python_deny_modules == ["socket"]


class TestPythonAllowSymbolDirective:
    """`python-allow-symbol module.symbol` parsing and merging."""

    def test_parses_a_dotted_symbol(self):
        cfg = parse_config("python-allow-symbol sys.stdin")
        assert cfg.python_allow_symbols == ["sys.stdin"]

    def test_parses_a_symbol_from_a_dotted_module(self):
        cfg = parse_config("python-allow-symbol http.client.HTTPResponse")
        assert cfg.python_allow_symbols == ["http.client.HTTPResponse"]

    def test_trailing_comment_is_stripped(self):
        cfg = parse_config("python-allow-symbol sys.stdin  # piping input")
        assert cfg.python_allow_symbols == ["sys.stdin"]

    @pytest.mark.parametrize(
        "line",
        [
            "python-allow-symbol",
            "python-allow-symbol sys",  # no module part
            "python-allow-symbol sys.stdin extra",
            "python-allow-symbol sys.",
            "python-allow-symbol .stdin",
            "python-allow-symbol sys.1stdin",
        ],
    )
    def test_rejects_malformed_symbols(self, line):
        """A malformed line is skipped with a warning, never silently allowed."""
        assert parse_config(line).python_allow_symbols == []

    def test_symbols_accumulate(self):
        base = parse_config("python-allow-symbol sys.stdin")
        overlay = parse_config("python-allow-symbol sys.stdout")
        merged = _merge_configs(base, overlay)
        assert merged.python_allow_symbols == ["sys.stdin", "sys.stdout"]

    def test_global_symbols_survive_silent_overlay(self):
        base = parse_config("python-allow-symbol sys.stdin")
        overlay = parse_config("allow echo *")
        merged = _merge_configs(base, overlay)
        assert merged.python_allow_symbols == ["sys.stdin"]
