"""Python module allow/deny lists must survive config merging."""

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
