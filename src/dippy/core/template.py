"""Safe template expansion for Dippy."""

# Whitelist of allowed placeholder keys for template expansion
# This prevents injection attacks via str.format() or similar
ALLOWED_PLACEHOLDERS = {
    "title",
    "message",
    "cwd",
    "notification_type",
    # Additional placeholders for deny_format compatibility
    "command",
    "reason",
    "pattern",
}


def expand_template(template: str, **kwargs) -> str:
    """Safely expand {key} placeholders in template.

    Only whitelisted keys are expanded. Unknown placeholders are left unchanged.
    Values are sanitized (newlines replaced, length limited, shell-escaped).

    Args:
        template: Template string with {key} placeholders.
        **kwargs: Values to expand in the template.

    Returns:
        Expanded template string with sanitized values.
    """
    result = template
    for key, value in kwargs.items():
        if key not in ALLOWED_PLACEHOLDERS:
            continue
        value_str = _sanitize_value(str(value))
        result = result.replace(f"{{{key}}}", value_str)
    return result


def _sanitize_value(value: str, max_length: int = 500) -> str:
    """Sanitize value for safe template expansion in double-quoted shell context.

    Escapes characters that could break out of double quotes or execute commands:
    - Backslashes (must be first to avoid double-escaping)
    - Double quotes
    - Dollar signs (command substitution)
    - Backticks (command substitution)
    - Newlines/carriage returns (break commands)
    - Null bytes (corruption/injection)

    Args:
        value: String to sanitize.
        max_length: Maximum length after sanitization.

    Returns:
        Sanitized string safe for use inside double quotes in shell commands.
    """
    # Remove null bytes first (potential injection vector)
    value = value.replace("\x00", "")
    # Escape backslashes FIRST (before adding more)
    value = value.replace("\\", "\\\\")
    # Escape characters that are special inside double quotes
    value = value.replace('"', '\\"')
    value = value.replace("$", "\\$")
    value = value.replace("`", "\\`")
    # Replace newlines and carriage returns with spaces (prevent line breaks)
    value = value.replace("\n", " ").replace("\r", " ")
    # Limit length
    value = value[:max_length].strip()
    return value
