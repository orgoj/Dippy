"""Tests for safe template expansion."""

from dippy.core.template import expand_template, ALLOWED_PLACEHOLDERS


class TestTemplateExpansion:
    """Test safe template expansion."""

    def test_basic_expansion(self):
        """Test basic placeholder expansion."""
        result = expand_template("{title}: {message}", title="Test", message="Hello")
        assert result == "Test: Hello"

    def test_cwd_expansion(self):
        """Test cwd placeholder expansion."""
        result = expand_template("In {cwd}", cwd="/home/user/project")
        assert result == "In /home/user/project"

    def test_unknown_placeholder_ignored(self):
        """Test unknown placeholders are left unchanged."""
        result = expand_template("{title} - {unknown}", title="Test")
        assert result == "Test - {unknown}"

    def test_newline_sanitization(self):
        """Test newlines are replaced with spaces."""
        result = expand_template("{message}", message="Line1\nLine2")
        assert result == "Line1 Line2"

    def test_carriage_return_sanitization(self):
        """Test carriage returns are replaced with spaces."""
        result = expand_template("{message}", message="Line1\rLine2")
        assert result == "Line1 Line2"

    def test_length_limit(self):
        """Test long values are truncated."""
        long = "x" * 1000
        result = expand_template("{message}", message=long)
        assert len(result) <= 500

    def test_null_byte_removal(self):
        """Test null bytes are removed."""
        result = expand_template("{message}", message="test\x00evil")
        assert "\x00" not in result

    def test_empty_value(self):
        """Test empty values are handled."""
        result = expand_template("{title}", title="")
        assert result == ""

    def test_all_allowed_placeholders(self):
        """Test all whitelisted placeholders can be expanded."""
        for key in ALLOWED_PLACEHOLDERS:
            result = expand_template(f"{{{key}}}", **{key: "value"})
            assert result == "value"

    def test_multiple_placeholders(self):
        """Test multiple placeholders in one template."""
        result = expand_template(
            "{title} in {cwd}: {message}",
            title="Build",
            cwd="/project",
            message="Compiling...",
        )
        assert result == "Build in /project: Compiling..."

    def test_notification_type_expansion(self):
        """Test notification_type placeholder."""
        result = expand_template(
            "Type: {notification_type}",
            notification_type="idle_prompt",
        )
        assert result == "Type: idle_prompt"

    def test_placeholder_not_replaced_if_missing(self):
        """Test placeholders with no value remain as-is."""
        result = expand_template("{title} - {message}", title="Test")
        assert result == "Test - {message}"

    def test_whitespace_in_value(self):
        """Test values with leading/trailing whitespace are trimmed."""
        result = expand_template("{message}", message="  hello  ")
        assert result == "hello"

    def test_mixed_placeholders_known_and_unknown(self):
        """Test mix of known and unknown placeholders."""
        result = expand_template(
            "{title} - {unknown1} - {message} - {unknown2}",
            title="Title",
            message="Message",
        )
        assert result == "Title - {unknown1} - Message - {unknown2}"

    def test_duplicate_placeholders(self):
        """Test duplicate placeholders are all replaced."""
        result = expand_template("{title} says {title}", title="Bot")
        assert result == "Bot says Bot"

    def test_nested_braces(self):
        """Test nested braces behavior (simple replacement, no escaping)."""
        result = expand_template("{{title}}", title="Test")
        # Simple string replacement: {title} inside {{title}} gets replaced
        assert result == "{Test}"

    def test_special_chars_in_value(self):
        """Test special characters in values are preserved."""
        result = expand_template("{message}", message="Hello, @user!")
        assert result == "Hello, @user!"

    def test_value_with_tabs(self):
        """Test tabs in values are preserved."""
        result = expand_template("{message}", message="hello\tworld")
        assert "\t" in result

    def test_deny_format_placeholders(self):
        """Test deny format specific placeholders work."""
        result = expand_template(
            "{command} -> {reason} ({pattern})",
            command="rm -rf /",
            reason="Too dangerous",
            pattern="rm -rf /*",
        )
        assert result == "rm -rf / -> Too dangerous (rm -rf /*)"

    def test_shell_escape_dollar_sign(self):
        """Test dollar signs are escaped for shell safety."""
        result = expand_template("{message}", message="Price: $100")
        assert r"\$" in result
        assert "$(evil)" not in result

    def test_shell_escape_double_quotes(self):
        """Test double quotes are escaped for shell safety."""
        result = expand_template("{message}", message='Say "hello"')
        # Quotes inside double quotes must be escaped
        assert '\\"' in result
        # Should not have unescaped quotes that would break out
        assert result.count('"') >= 2  # At least opening + closing + escaped quotes

    def test_shell_escape_backticks(self):
        """Test backticks are escaped for shell safety."""
        result = expand_template("{message}", message="run`evil`")
        assert "\\`" in result
        assert "`evil`" not in result

    def test_shell_escape_backslashes(self):
        """Test backslashes are escaped for shell safety."""
        result = expand_template("{message}", message="path\\to\\file")
        assert "\\\\" in result  # Backslashes should be escaped

    def test_shell_escape_combined(self):
        """Test multiple special chars are escaped together."""
        result = expand_template(
            "{message}",
            message='He said "pay $100" and ran `evil`\\command',
        )
        # All dangerous chars should be escaped
        assert "\\$" in result
        assert '\\"' in result
        assert "\\`" in result
        assert "\\\\" in result

    def test_command_subjection_blocked(self):
        """Test command substitution is blocked."""
        result = expand_template("{message}", message="$(rm -rf /)")
        assert "$(" not in result or result.startswith("\\$")

    def test_safe_for_shell_double_quotes(self):
        """Test expanded value is safe inside double quotes."""
        dangerous = 'Hello"; echo "PWNED"; echo "'
        result = expand_template('echo "{message}"', message=dangerous)
        # The escaped quotes should not break out of the shell context
        assert '\\"' in result or result.count('";') == 0
