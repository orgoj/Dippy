"""Dippy configuration system v1."""

import fnmatch
import logging
import os
import re
from dataclasses import dataclass, field, replace
from datetime import datetime, timedelta
from pathlib import Path

from dippy.core.parser import tokenize

# Cache home directory at module load - fails fast if HOME is unset
_HOME = Path.home()

USER_CONFIG = _HOME / ".dippy" / "config"
PROJECT_CONFIG_NAME = ".dippy"
ENV_CONFIG = "DIPPY_CONFIG"


class ConfigError(Exception):
    """Raised when config loading fails due to I/O or parse errors."""

    pass


# Config scopes in priority order (lowest to highest)
SCOPE_USER = "user"
SCOPE_PROJECT = "project"
SCOPE_ENV = "env"
SCOPE_FINAL = "final"


@dataclass
class Rule:
    """A single config rule with origin tracking."""

    decision: str  # 'allow' | 'ask' | 'deny'
    pattern: str
    message: str | None = None
    source: str | None = None  # file path
    scope: str | None = None  # user/project/env
    items: list[str] | None = None  # for option rules: list of items to match anywhere
    required_flags: frozenset[str] | None = None  # context flags that must all match
    negated_flags: frozenset[str] | None = (
        None  # context flags that must NOT be present
    )


@dataclass(frozen=True)
class WrapperInfo:
    """Configuration for a wrapper command."""

    name: str
    trigger: str | None = None
    """Subcommand that triggers inner command analysis (e.g. 'run', 'exec')."""
    target_flag: str | None = None
    """Flag that specifies the destination/target (e.g. '-t', '-h')."""


@dataclass
class Config:
    """Parsed configuration."""

    rules: list[Rule] = field(default_factory=list)
    """Command rules in load order."""

    redirect_rules: list[Rule] = field(default_factory=list)
    """Redirect rules in load order."""

    after_rules: list[Rule] = field(default_factory=list)
    """After rules for PostToolUse feedback."""

    mcp_rules: list[Rule] = field(default_factory=list)
    """MCP tool rules in load order."""

    after_mcp_rules: list[Rule] = field(default_factory=list)
    """After-MCP rules for PostToolUse feedback on MCP tools."""

    edit_rules: list[Rule] = field(default_factory=list)
    """Edit rules for Write/Edit/MultiEdit tools."""

    read_rules: list[Rule] = field(default_factory=list)
    """Read rules for Read tool."""

    web_rules: list[Rule] = field(default_factory=list)
    """WebSearch tool rules in load order."""

    after_web_rules: list[Rule] = field(default_factory=list)
    """After-web rules for PostToolUse feedback on WebSearch."""

    wrappers: dict[str, WrapperInfo] = field(default_factory=dict)
    """Custom wrapper commands mapping name to info."""

    aliases: dict[str, str] = field(default_factory=dict)
    """Command aliases mapping source to target (e.g., ~/bin/gh -> gh)."""

    default: str = "ask"  # 'allow' | 'ask' | 'pass'
    log: Path | None = None  # None = no logging
    log_full: bool = False  # log full command (requires log path)
    log_rotate_max_days: int = 30  # days to keep rotated logs (0 = disabled)
    log_hook_approvals: bool = True  # log to hook-approvals.log
    final: Path | None = None  # path to final config (loaded last)
    askpass: Path | None = None  # external approval program (SSH_ASKPASS style)
    askpass_timeout: int = 60  # seconds to wait for askpass response
    notifier_command: str | None = None  # external notification command
    notifier_include: frozenset[str] | None = None  # tools/commands to trigger notifier


@dataclass
class Match:
    """Result of matching against config rules."""

    decision: str  # 'allow' | 'ask' | 'deny'
    pattern: str  # the glob pattern that matched
    message: str | None = None  # shown to AI on ask/deny
    source: str | None = None  # file path where rule was defined
    scope: str | None = None  # user/project/env


@dataclass
class SimpleCommand:
    """A simple command extracted from parsed bash.

    This is the intermediate representation passed to the rule engine.
    Dippy parses raw bash with Parable, walks the AST, and constructs
    SimpleCommand instances for each command node.
    """

    words: list[str]
    """Command words, e.g. ["git", "add", "."]."""

    redirects: list[str] = field(default_factory=list)
    """Redirect target paths, e.g. ["/tmp/log.txt", "~/.cache/out"]."""


# === Config Loading ===


def _find_project_config(cwd: Path) -> Path | None:
    """Walk up from cwd to find .dippy file."""
    current = cwd.resolve()
    while True:
        candidate = current / PROJECT_CONFIG_NAME
        if candidate.is_file():
            return candidate
        parent = current.parent
        if parent == current:  # reached root
            return None
        current = parent


def _merge_configs(base: Config, overlay: Config) -> Config:
    """Merge overlay config into base. Rules accumulate in order, settings override."""
    return replace(
        base,
        # Rules accumulate in load order (like git)
        rules=base.rules + overlay.rules,
        redirect_rules=base.redirect_rules + overlay.redirect_rules,
        after_rules=base.after_rules + overlay.after_rules,
        mcp_rules=base.mcp_rules + overlay.mcp_rules,
        after_mcp_rules=base.after_mcp_rules + overlay.after_mcp_rules,
        edit_rules=base.edit_rules + overlay.edit_rules,
        read_rules=base.read_rules + overlay.read_rules,
        web_rules=base.web_rules + overlay.web_rules,
        after_web_rules=base.after_web_rules + overlay.after_web_rules,
        # Wrappers accumulate (merge dicts)
        wrappers={**base.wrappers, **overlay.wrappers},
        # Settings: overlay wins if set
        default=overlay.default if overlay.default != "ask" else base.default,
        log=overlay.log if overlay.log is not None else base.log,
        log_full=overlay.log_full if overlay.log_full else base.log_full,
        final=overlay.final if overlay.final is not None else base.final,
        askpass=overlay.askpass if overlay.askpass is not None else base.askpass,
        askpass_timeout=(
            overlay.askpass_timeout
            if overlay.askpass_timeout != 60
            else base.askpass_timeout
        ),
    )


def _tag_rules(config: Config, source: str, scope: str) -> Config:
    """Tag all rules in config with source file and scope."""
    return replace(
        config,
        rules=[replace(r, source=source, scope=scope) for r in config.rules],
        redirect_rules=[
            replace(r, source=source, scope=scope) for r in config.redirect_rules
        ],
        after_rules=[
            replace(r, source=source, scope=scope) for r in config.after_rules
        ],
        mcp_rules=[replace(r, source=source, scope=scope) for r in config.mcp_rules],
        after_mcp_rules=[
            replace(r, source=source, scope=scope) for r in config.after_mcp_rules
        ],
        edit_rules=[replace(r, source=source, scope=scope) for r in config.edit_rules],
        read_rules=[replace(r, source=source, scope=scope) for r in config.read_rules],
        web_rules=[replace(r, source=source, scope=scope) for r in config.web_rules],
        after_web_rules=[
            replace(r, source=source, scope=scope) for r in config.after_web_rules
        ],
    )


def _expand_includes(
    text: str,
    base_dir: Path,
    current_file: Path,
    included_files: set[Path],
) -> str:
    """Recursively expand include directives.

    Args:
        text: Config text to process
        base_dir: Directory to resolve relative paths from
        current_file: Current config file (for circular detection)
        included_files: Set of already included files (circular detection)

    Returns:
        Text with all includes expanded inline

    Raises:
        ConfigError: On circular includes or I/O errors
    """
    import glob

    # Track this file
    included_files.add(current_file.resolve())

    result_lines = []
    for lineno, line in enumerate(text.splitlines(), 1):
        stripped = line.strip()

        # Check if this is an include directive (with or without pattern)
        if not stripped.startswith("include"):
            result_lines.append(line)
            continue

        # Must be "include" followed by whitespace or EOL
        if len(stripped) > 7 and not stripped[7].isspace():
            # Not an include directive, just a line starting with "include"
            result_lines.append(line)
            continue

        # Parse include directive
        pattern = stripped[7:].strip() if len(stripped) > 7 else ""
        if not pattern:
            logging.warning(f"{current_file}:{lineno}: empty include pattern (skipped)")
            continue

        # Expand ~ and resolve relative to base_dir
        pattern_path = Path(pattern).expanduser()
        if not pattern_path.is_absolute():
            pattern_path = base_dir / pattern_path

        # Expand glob pattern
        matches = sorted(glob.glob(str(pattern_path)))

        if not matches:
            logging.warning(
                f"{current_file}:{lineno}: no files match '{pattern}' (skipped)"
            )
            continue

        # Process each matched file
        for match_str in matches:
            match_path = Path(match_str).resolve()

            # Circular include detection
            if match_path in included_files:
                raise ConfigError(f"circular include: {current_file} -> {match_path}")

            # Read and recursively expand
            try:
                included_text = match_path.read_text()
            except PermissionError:
                raise ConfigError(
                    f"permission denied reading included file: {match_path}"
                ) from None
            except OSError as e:
                raise ConfigError(
                    f"cannot read included file {match_path}: {e}"
                ) from None

            # Recursive expansion (included file can have includes)
            expanded = _expand_includes(
                included_text,
                match_path.parent,  # Relative paths in included file resolve from its dir
                match_path,
                included_files,
            )

            # Add expanded content with comment marker
            result_lines.append(f"# included from: {match_path}")
            result_lines.append(expanded)

    return "\n".join(result_lines)


def _load_config_file(path: Path) -> Config:
    """Read and parse a config file. Raises ConfigError on I/O failure."""
    try:
        text = path.read_text()
    except PermissionError:
        raise ConfigError(f"permission denied reading config: {path}") from None
    except OSError as e:
        raise ConfigError(f"cannot read config {path}: {e}") from None

    # Preprocess includes
    included_files: set[Path] = set()
    text = _expand_includes(text, path.parent, path, included_files)

    return parse_config(text, source=str(path))


def _rotate_logs(config: Config) -> None:
    """Rotate audit log daily and clean up old logs.

    Only rotates once per day (first run after midnight).
    Safe to call multiple times - checks if already rotated today.
    """
    # Skip if logging disabled or rotation disabled
    if config.log is None or config.log_rotate_max_days <= 0:
        return

    # Check if we already rotated today
    yesterday = (datetime.now() - timedelta(days=1)).strftime("%Y-%m-%d")
    rotated_path = config.log.parent / f"audit-{yesterday}.log"

    if rotated_path.exists():
        return  # Already rotated today, nothing to do

    # Rotate: rename current log to yesterday's date
    if config.log.exists():
        config.log.rename(rotated_path)

    # Clean up old logs
    cutoff = (datetime.now() - timedelta(days=config.log_rotate_max_days)).strftime(
        "%Y-%m-%d"
    )
    for old_log in config.log.parent.glob("audit-*.log"):
        # Extract date from filename: "audit-YYYY-MM-DD.log"
        parts = old_log.stem.split("-")
        if len(parts) >= 4:
            date_str = "-".join(parts[1:4])  # Extract YYYY-MM-DD
            if date_str < cutoff:
                old_log.unlink()


def load_config(cwd: Path, config_path: str | None = None) -> Config:
    """Load config from ~/.dippy/config, .dippy, and $DIPPY_CONFIG.

    Args:
        cwd: Current working directory (used to find project config).
        config_path: Optional explicit config file path (highest priority,
                     overrides $DIPPY_CONFIG).

    Raises ConfigError if any config file exists but cannot be read or parsed.
    Missing files are silently skipped.
    """
    config = Config()

    # 1. User config (lowest priority)
    try:
        if USER_CONFIG.is_file():
            user_config = _load_config_file(USER_CONFIG)
            user_config = _tag_rules(user_config, str(USER_CONFIG), SCOPE_USER)
            config = _merge_configs(config, user_config)
    except PermissionError:
        raise ConfigError(f"permission denied accessing {USER_CONFIG}") from None

    # 2. Project config (walk up from cwd)
    project_path = _find_project_config(cwd)
    if project_path is not None:
        project_config = _load_config_file(project_path)
        project_config = _tag_rules(project_config, str(project_path), SCOPE_PROJECT)
        config = _merge_configs(config, project_config)

    # 3. Env override or explicit config_path (highest priority)
    override_path = config_path or os.environ.get(ENV_CONFIG)
    if override_path:
        override_config_path = Path(override_path).expanduser()
        try:
            if override_config_path.is_file():
                override_config = _load_config_file(override_config_path)
                override_config = _tag_rules(
                    override_config, str(override_config_path), SCOPE_ENV
                )
                config = _merge_configs(config, override_config)
            elif config_path:
                # Explicit --config path must exist
                raise ConfigError(f"config file not found: {override_config_path}")
        except PermissionError:
            raise ConfigError(
                f"permission denied accessing {override_config_path}"
            ) from None

    # 4. Final config (if configured via 'set final')
    if config.final:
        try:
            if config.final.is_file():
                final_config = _load_config_file(config.final)
                final_config = _tag_rules(final_config, str(config.final), SCOPE_FINAL)
                config = _merge_configs(config, final_config)
            else:
                logging.warning(f"Final config not found: {config.final}")
        except PermissionError:
            raise ConfigError(f"permission denied accessing {config.final}") from None

    # Rotate logs at the end of config loading
    _rotate_logs(config)

    return config


def _extract_context_flags(
    s: str,
) -> tuple[str, frozenset[str] | None, frozenset[str] | None]:
    """Extract context flags from a pattern string.

    Syntax: [flag1,!flag2,...] pattern
    - AST flags start with @ (e.g., @subshell)
    - Wrapper flags have no prefix (e.g., ssh)
    - Negated flags start with ! (e.g., !@subshell, !ssh)

    Returns (remaining_pattern, required_flags, negated_flags).
    """
    s = s.strip()
    if not s.startswith("["):
        return s, None, None

    # Find closing bracket
    end = s.find("]")
    if end == -1:
        return s, None, None  # Malformed, treat as pattern

    flags_str = s[1:end].strip()
    remaining = s[end + 1 :].strip()

    if not flags_str:
        return remaining, None, None

    # Parse comma-separated flags, separating required from negated
    required: set[str] = set()
    negated: set[str] = set()

    for f in flags_str.split(","):
        f = f.strip()
        if not f:
            continue
        if f.startswith("!"):
            # Negated flag - strip the ! prefix
            negated.add(f[1:])
        else:
            required.add(f)

    return (
        remaining,
        frozenset(required) if required else None,
        frozenset(negated) if negated else None,
    )


def _strip_exact_anchor(pattern: str) -> tuple[str, bool]:
    """Strip | anchor from pattern, return (pattern, is_exact)."""
    if pattern.endswith("|"):
        return pattern[:-1].rstrip(), True
    return pattern, False


def _parse_option_rule(decision: str, rest: str) -> Rule:
    """Parse an option rule: <prefix> <item1> <item2>...

    The prefix can be quoted (e.g., "git commit") or a single word (e.g., git).
    Items are subcommands or flags to match anywhere in the command.
    """
    # Extract message first if present
    pattern, message = _extract_message(rest)

    # Parse prefix and items
    # Prefix can be quoted or single word
    parts = tokenize(pattern)
    if not parts:
        # Fallback to simple split if tokenize fails
        parts = pattern.split()

    if not parts:
        raise ValueError("option rule requires prefix and items")

    # First part is prefix, rest are items
    prefix = parts[0]
    items = parts[1:] if len(parts) > 1 else []

    if not items:
        raise ValueError("option rule requires at least one item to match")

    return Rule(decision, prefix, message=message, items=items)


def parse_config(text: str, source: str | None = None) -> Config:
    """Parse config text into Config object. Logs and skips invalid lines."""

    rules: list[Rule] = []
    redirect_rules: list[Rule] = []
    after_rules: list[Rule] = []
    mcp_rules: list[Rule] = []
    after_mcp_rules: list[Rule] = []
    edit_rules: list[Rule] = []
    read_rules: list[Rule] = []
    web_rules: list[Rule] = []
    after_web_rules: list[Rule] = []
    wrappers: dict[str, WrapperInfo] = {}
    settings: dict[str, bool | int | str | Path] = {}
    prefix = f"{source}: " if source else ""

    for lineno, raw_line in enumerate(text.splitlines(), 1):
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue

        parts = line.split(None, 1)
        directive = parts[0].lower()
        rest = parts[1].strip() if len(parts) > 1 else ""

        try:
            if directive == "allow":
                if not rest:
                    raise ValueError("requires a pattern")
                pattern_part, flags, neg_flags = _extract_context_flags(rest)
                if not pattern_part:
                    raise ValueError("requires a pattern after flags")
                rules.append(
                    Rule(
                        "allow",
                        _expand_pattern_tildes(pattern_part),
                        required_flags=flags,
                        negated_flags=neg_flags,
                    )
                )

            elif directive == "ask":
                if not rest:
                    raise ValueError("requires a pattern")
                pattern_part, flags, neg_flags = _extract_context_flags(rest)
                if not pattern_part:
                    raise ValueError("requires a pattern after flags")
                pattern, message = _extract_message(pattern_part)
                rules.append(
                    Rule(
                        "ask",
                        _expand_pattern_tildes(pattern),
                        message=message,
                        required_flags=flags,
                        negated_flags=neg_flags,
                    )
                )

            elif directive == "deny":
                if not rest:
                    raise ValueError("requires a pattern")
                pattern_part, flags, neg_flags = _extract_context_flags(rest)
                if not pattern_part:
                    raise ValueError("requires a pattern after flags")
                pattern, message = _extract_message(pattern_part)
                rules.append(
                    Rule(
                        "deny",
                        _expand_pattern_tildes(pattern),
                        message=message,
                        required_flags=flags,
                        negated_flags=neg_flags,
                    )
                )

            elif directive == "allow-redirect":
                if not rest:
                    raise ValueError("requires a pattern")
                redirect_rules.append(Rule("allow", _expand_pattern_tildes(rest)))

            elif directive == "ask-redirect":
                if not rest:
                    raise ValueError("requires a pattern")
                pattern, message = _extract_message(rest)
                redirect_rules.append(
                    Rule("ask", _expand_pattern_tildes(pattern), message=message)
                )

            elif directive == "deny-redirect":
                if not rest:
                    raise ValueError("requires a pattern")
                pattern, message = _extract_message(rest)
                redirect_rules.append(
                    Rule("deny", _expand_pattern_tildes(pattern), message=message)
                )

            elif directive == "after":
                if not rest:
                    raise ValueError("requires a pattern")
                pattern, message = _extract_message(rest)
                after_rules.append(Rule("after", pattern, message=message))

            elif directive == "allow-opt":
                if not rest:
                    raise ValueError("requires a prefix and items")
                rules.append(_parse_option_rule("allow", rest))

            elif directive == "ask-opt":
                if not rest:
                    raise ValueError("requires a prefix and items")
                rules.append(_parse_option_rule("ask", rest))

            elif directive == "deny-opt":
                if not rest:
                    raise ValueError("requires a prefix and items")
                rules.append(_parse_option_rule("deny", rest))

            elif directive == "allow-mcp":
                if not rest:
                    raise ValueError("requires a pattern")
                mcp_rules.append(Rule("allow", rest))

            elif directive == "ask-mcp":
                if not rest:
                    raise ValueError("requires a pattern")
                pattern, message = _extract_message(rest)
                mcp_rules.append(Rule("ask", pattern, message=message))

            elif directive == "deny-mcp":
                if not rest:
                    raise ValueError("requires a pattern")
                pattern, message = _extract_message(rest)
                mcp_rules.append(Rule("deny", pattern, message=message))

            elif directive == "after-mcp":
                if not rest:
                    raise ValueError("requires a pattern")
                pattern, message = _extract_message(rest)
                after_mcp_rules.append(Rule("after", pattern, message=message))

            elif directive == "allow-edit":
                if not rest:
                    raise ValueError("requires a pattern")
                edit_rules.append(Rule("allow", _expand_pattern_tildes(rest)))

            elif directive == "ask-edit":
                if not rest:
                    raise ValueError("requires a pattern")
                pattern, message = _extract_message(rest)
                edit_rules.append(
                    Rule("ask", _expand_pattern_tildes(pattern), message=message)
                )

            elif directive == "deny-edit":
                if not rest:
                    raise ValueError("requires a pattern")
                pattern, message = _extract_message(rest)
                edit_rules.append(
                    Rule("deny", _expand_pattern_tildes(pattern), message=message)
                )

            elif directive == "allow-read":
                if not rest:
                    raise ValueError("requires a pattern")
                read_rules.append(Rule("allow", _expand_pattern_tildes(rest)))

            elif directive == "ask-read":
                if not rest:
                    raise ValueError("requires a pattern")
                pattern, message = _extract_message(rest)
                read_rules.append(
                    Rule("ask", _expand_pattern_tildes(pattern), message=message)
                )

            elif directive == "deny-read":
                if not rest:
                    raise ValueError("requires a pattern")
                pattern, message = _extract_message(rest)
                read_rules.append(
                    Rule("deny", _expand_pattern_tildes(pattern), message=message)
                )

            elif directive == "allow-web":
                pattern = rest if rest else "*"
                web_rules.append(Rule("allow", pattern))

            elif directive == "ask-web":
                if not rest:
                    raise ValueError("requires a pattern")
                pattern, message = _extract_message(rest)
                web_rules.append(Rule("ask", pattern, message=message))

            elif directive == "deny-web":
                if not rest:
                    raise ValueError("requires a pattern")
                pattern, message = _extract_message(rest)
                web_rules.append(Rule("deny", pattern, message=message))

            elif directive == "after-web":
                if not rest:
                    raise ValueError("requires a pattern")
                pattern, message = _extract_message(rest)
                after_web_rules.append(Rule("after", pattern, message=message))

            elif directive == "wrapper":
                if not rest:
                    raise ValueError("requires a command name")
                parts = rest.split()
                wrapper_name = parts[0]
                trigger = parts[1] if len(parts) > 1 else None
                target_flag = parts[2] if len(parts) > 2 else None

                if wrapper_name.startswith("-"):
                    raise ValueError(
                        f"wrapper name cannot start with '-': {wrapper_name}"
                    )
                if wrapper_name in wrappers:
                    logging.warning(
                        f"{prefix}line {lineno}: duplicate wrapper definition: {wrapper_name}"
                    )
                wrappers[wrapper_name] = WrapperInfo(
                    name=wrapper_name, trigger=trigger, target_flag=target_flag
                )

            elif directive == "set":
                _apply_setting(settings, rest)

            else:
                raise ValueError(f"unknown directive '{directive}'")

        except ValueError as e:
            logging.warning(f"{prefix}line {lineno}: {e} (skipped)")

    return Config(
        rules=rules,
        redirect_rules=redirect_rules,
        after_rules=after_rules,
        mcp_rules=mcp_rules,
        after_mcp_rules=after_mcp_rules,
        edit_rules=edit_rules,
        read_rules=read_rules,
        web_rules=web_rules,
        after_web_rules=after_web_rules,
        wrappers=wrappers,
        default=settings.get("default", "ask"),
        log=settings.get("log"),
        log_full=settings.get("log_full", False),
        log_rotate_max_days=settings.get("log_rotate_max_days", 30),
        log_hook_approvals=settings.get("log_hook_approvals", True),
        final=settings.get("final"),
        askpass=settings.get("askpass"),
        askpass_timeout=settings.get("askpass_timeout", 60),
        notifier_command=settings.get("notifier_command"),
        notifier_include=settings.get("notifier_include"),
    )


def _unescape(s: str) -> str:
    """Unescape backslash sequences in a message string."""
    result = []
    i = 0
    while i < len(s):
        if s[i] == "\\" and i + 1 < len(s):
            next_char = s[i + 1]
            if next_char in ('"', "\\"):
                result.append(next_char)
                i += 2
                continue
        result.append(s[i])
        i += 1
    return "".join(result)


def _extract_message(s: str) -> tuple[str, str | None]:
    """Extract pattern and optional quoted message from string.

    Message is extracted only if:
    - String ends with unescaped "
    - There's an opening " preceded by whitespace

    Returns (pattern, message) where message may be None.
    """
    s = s.rstrip()
    if not s.endswith('"'):
        return s, None

    # Count trailing backslashes to check if quote is escaped
    j = len(s) - 2
    num_bs = 0
    while j >= 0 and s[j] == "\\":
        num_bs += 1
        j -= 1
    if num_bs % 2 == 1:
        return s, None  # Trailing quote is escaped

    # Find opening quote (must be preceded by whitespace)
    i = len(s) - 2
    while i >= 0:
        if s[i] == '"' and (i == 0 or s[i - 1].isspace()):
            message = _unescape(s[i + 1 : -1])
            pattern = s[:i].rstrip()
            if not pattern:
                raise ValueError("pattern required before message")
            return pattern, message
        i -= 1

    return s, None  # No valid opening quote, treat as pattern


def _strip_quotes(value: str) -> str:
    """Strip surrounding quotes from a value."""
    if len(value) >= 2:
        if (value[0] == '"' and value[-1] == '"') or (
            value[0] == "'" and value[-1] == "'"
        ):
            return value[1:-1]
    return value


def _apply_setting(settings: dict[str, bool | int | str | Path], rest: str) -> None:
    """Parse and apply a 'set' directive. Raises ValueError on invalid setting."""
    if not rest:
        raise ValueError("'set' requires a setting name")

    parts = rest.split(None, 1)
    key = parts[0].lower()
    value = parts[1] if len(parts) > 1 else None
    key_normalized = key.replace("-", "_")

    # Boolean settings (no value required)
    if key_normalized in ("log_full",):
        if value is not None:
            raise ValueError(f"'{key}' takes no value")
        settings[key_normalized] = True

    # Boolean settings with on/off value
    elif key_normalized == "log_hook_approvals":
        if value is None:
            raise ValueError(f"'{key}' requires 'on' or 'off'")
        value_lower = value.lower()
        if value_lower == "on":
            settings[key_normalized] = True
        elif value_lower == "off":
            settings[key_normalized] = False
        else:
            raise ValueError(f"'{key}' must be 'on' or 'off', got '{value}'")

    # Choice settings
    elif key_normalized == "default":
        if value not in ("allow", "ask", "pass"):
            raise ValueError(
                f"'default' must be 'allow', 'ask' or 'pass', got '{value}'"
            )
        settings[key_normalized] = value

    # Path settings
    elif key_normalized == "log":
        if value is None:
            raise ValueError("'log' requires a path")
        settings[key_normalized] = Path(value).expanduser()

    elif key_normalized == "final":
        if value is None:
            raise ValueError("'final' requires a path")
        settings[key_normalized] = Path(value).expanduser()

    elif key_normalized == "askpass":
        if value is None:
            raise ValueError("'askpass' requires a path")
        settings[key_normalized] = Path(value).expanduser()

    # Integer settings
    elif key_normalized == "log_rotate_max_days":
        if value is None:
            raise ValueError("'log-rotate-max-days' requires a number")
        try:
            settings[key_normalized] = int(value)
        except ValueError:
            raise ValueError(f"'log-rotate-max-days' must be an integer, got '{value}'")

    elif key_normalized == "askpass_timeout":
        if value is None:
            raise ValueError("'askpass-timeout' requires a number")
        try:
            settings[key_normalized] = int(value)
        except ValueError:
            raise ValueError(f"'askpass-timeout' must be an integer, got '{value}'")

    elif key_normalized == "notifier_command":
        if value is None:
            raise ValueError("'notifier-command' requires a command string")
        settings[key_normalized] = _strip_quotes(value)

    elif key_normalized == "notifier_include":
        if value is None:
            raise ValueError("'notifier-include' requires a list of tools or commands")
        # Split by comma and strip whitespace and quotes
        stripped_value = _strip_quotes(value)
        items = [i.strip() for i in stripped_value.split(",") if i.strip()]
        settings[key_normalized] = frozenset(items)

    else:
        raise ValueError(f"unknown setting '{key}'")


# === Path Classification & Expansion ===

# Token kinds (order matters for classification precedence)
_URL = "url"  # https://example.com, ftp://...
_VARIABLE = "variable"  # $HOME, ${VAR}, $0
_ABSOLUTE = "absolute"  # /foo/bar
_HOME = "home"  # ~ or ~/foo
_USER_HOME = "user_home"  # ~bob or ~bob/foo
_RELATIVE = "relative"  # ./foo, ../foo, ., .., or contains /
_BARE = "bare"  # everything else (command names, flags, args)


def _classify_token(token: str) -> str:
    """Classify a token into one of the path kinds.

    Classification is pure - no side effects, no cwd needed.
    Order matters: earlier checks take precedence.
    """
    if "://" in token:
        return _URL
    if token.startswith("$"):
        return _VARIABLE
    if token.startswith("/"):
        return _ABSOLUTE
    if token == "~" or token.startswith("~/"):
        return _HOME
    if token.startswith("~"):
        return _USER_HOME
    if (
        token in (".", "..")
        or token.startswith("./")
        or token.startswith("../")
        or "/" in token
    ):
        return _RELATIVE
    return _BARE


def _expand_token(token: str, cwd: Path, *, force_path: bool = False) -> str:
    """Expand a token based on its classification.

    Args:
        token: The token to expand
        cwd: Working directory for resolving relative paths
        force_path: If True, treat BARE tokens as paths (for redirects)

    Returns:
        Expanded token string
    """
    kind = _classify_token(token)
    home = Path.home()
    if kind == _URL:
        return token
    if kind == _VARIABLE:
        return token
    if kind == _ABSOLUTE:
        return token
    if kind == _HOME:
        # ~ → /home/user, ~/foo → /home/user/foo
        return str(home) + token[1:] if len(token) > 1 else str(home)
    if kind == _USER_HOME:
        return token
    if kind == _RELATIVE:
        return str((cwd / token).resolve())
    # BARE
    if force_path:
        return str((cwd / token).resolve())
    return token


def _expand_home_only(token: str) -> str:
    """Expand only HOME kind tokens (~ and ~/...) at parse time.

    Used for pattern tilde expansion to match settings behavior.
    Other token kinds pass through unchanged.
    """
    if _classify_token(token) == _HOME:
        home = Path.home()
        return str(home) + token[1:] if len(token) > 1 else str(home)
    return token


def _expand_pattern_tildes(pattern: str) -> str:
    """Expand tildes in pattern tokens at parse time."""
    return " ".join(_expand_home_only(t) for t in pattern.split())


def _normalize_token(token: str, cwd: Path) -> str:
    """Normalize a single token in a command."""
    return _expand_token(token, cwd, force_path=False)


def _normalize_words(words: list[str], cwd: Path) -> str:
    """Normalize paths in command words and join into a string for matching."""
    return " ".join(_normalize_token(w, cwd) for w in words)


def _normalize_pattern(pattern: str, cwd: Path) -> str:
    """Normalize paths in a pattern against cwd.

    Splits pattern on spaces (preserving glob chars), normalizes path-like
    tokens, rejoins. This allows patterns like 'node bin/*' to expand to
    'node /cwd/bin/*'.
    """
    tokens = pattern.split()
    return " ".join(_normalize_token(t, cwd) for t in tokens)


def _normalize_path(path: str, cwd: Path) -> str:
    """Normalize a redirect target path (strip trailing /, force as path)."""
    return _expand_token(path.rstrip("/"), cwd, force_path=True)


def _glob_to_regex(pattern: str) -> re.Pattern:
    """Convert a glob pattern with ** support to a regex.

    ** matches zero or more path components (including /)
    * matches anything except /
    ? matches any single character except /
    [abc] matches character class
    """
    regex = []
    i = 0
    n = len(pattern)
    while i < n:
        c = pattern[i]
        if c == "*":
            if i + 1 < n and pattern[i + 1] == "*":
                # ** - matches anything including /
                regex.append(".*")
                i += 2
                # Skip trailing / after **
                if i < n and pattern[i] == "/":
                    regex.append("/?")
                    i += 1
            else:
                # * - matches anything except /
                regex.append("[^/]*")
                i += 1
        elif c == "?":
            regex.append("[^/]")
            i += 1
        elif c == "[":
            # Character class - find the closing ]
            j = i + 1
            if j < n and pattern[j] == "!":
                j += 1
            if j < n and pattern[j] == "]":
                j += 1
            while j < n and pattern[j] != "]":
                j += 1
            if j >= n:
                # Unclosed bracket, treat as literal
                regex.append(re.escape(c))
                i += 1
            else:
                # Convert [!...] to [^...]
                cls = pattern[i + 1 : j]
                if cls.startswith("!"):
                    cls = "^" + cls[1:]
                regex.append(f"[{cls}]")
                i = j + 1
        else:
            regex.append(re.escape(c))
            i += 1
    return re.compile("^" + "".join(regex) + "$")


def _glob_match(text: str, pattern: str) -> bool:
    """Match text against a glob pattern with ** support.

    For patterns without **, uses fnmatch (faster).
    For patterns with **, converts to regex for proper recursive matching:
    - ** matches zero or more directories
    - foo/**/bar matches foo/bar, foo/x/bar, foo/x/y/bar
    """
    if "**" not in pattern:
        return fnmatch.fnmatch(text, pattern)
    if pattern == "**":
        return True
    try:
        regex = _glob_to_regex(pattern)
        return regex.match(text) is not None
    except re.error:
        return False


def _match_option_rule(rule: Rule, words: list[str]) -> bool:
    """Check if an option rule matches the command words.

    Matches if:
    1. Command words start with the rule's prefix
    2. Any item from rule.items matches any word exactly (word-boundary)

    Flags like --foo match --foo exactly, not --foo-bar.
    Use glob patterns like --foo* in normal rules for prefix matching.
    """
    if not rule.items:
        return False

    # Check prefix match
    prefix_words = rule.pattern.split()
    if len(words) < len(prefix_words):
        return False
    for i, pw in enumerate(prefix_words):
        if words[i] != pw:
            return False

    # Check if any item matches any remaining word exactly
    remaining_words = words[len(prefix_words) :]
    items_set = set(rule.items)
    return bool(items_set.intersection(remaining_words))


def _match_words(
    words: list[str],
    config: Config,
    cwd: Path,
    context_flags: frozenset[str] | None = None,
    *,
    remote: bool = False,
) -> Match | None:
    """Match command words against rules. Returns last matching rule.

    Args:
        words: Command words to match.
        config: Configuration with rules.
        cwd: Current working directory for path resolution.
        context_flags: Optional set of active context flags (e.g., {"@subshell"}).
            Rules with required_flags only match if all flags are present.
            Rules with negated_flags only match if NONE of those flags are present.
        remote: If True, paths are NOT expanded against cwd (container/remote context).
    """
    # When remote, don't expand paths - match against literal words
    normalized_cmd = " ".join(words) if remote else _normalize_words(words, cwd)
    result: Match | None = None
    active_flags = context_flags or frozenset()

    for rule in config.rules:
        # Check context flags first - rule only applies if all required flags are present
        if rule.required_flags is not None:
            if not rule.required_flags.issubset(active_flags):
                continue

        # Check negated flags - rule only applies if NONE of negated flags are present
        if rule.negated_flags is not None:
            if rule.negated_flags & active_flags:  # intersection is non-empty
                continue

        # Option rules use different matching logic
        if rule.items is not None:
            if _match_option_rule(rule, words):
                result = Match(
                    decision=rule.decision,
                    pattern=rule.pattern,
                    message=rule.message,
                    source=rule.source,
                    scope=rule.scope,
                )
                continue

        normalized_pattern = _normalize_pattern(rule.pattern, cwd)
        matched = fnmatch.fnmatch(normalized_cmd, normalized_pattern)
        # Trailing ' *' also matches bare command (no args)
        if not matched and normalized_pattern.endswith(" *"):
            matched = normalized_cmd == normalized_pattern[:-2]
        if matched:
            result = Match(
                decision=rule.decision,
                pattern=rule.pattern,
                message=rule.message,
                source=rule.source,
                scope=rule.scope,
            )
    return result


def _normalize_redirect_pattern(pattern: str, cwd: Path) -> str:
    """Normalize a redirect pattern, handling ** specially.

    For patterns with **, normalize the prefix before ** and keep the rest.
    For example: 'src/**' -> '/abs/path/to/src/**'
    """
    if "**" not in pattern:
        return _normalize_path(pattern, cwd)
    # Split at first **, normalize prefix, rejoin
    idx = pattern.index("**")
    prefix = pattern[:idx].rstrip("/")
    suffix = pattern[idx:]
    if prefix:
        normalized_prefix = _normalize_path(prefix, cwd)
        return f"{normalized_prefix}/{suffix}"
    # Pattern starts with ** (e.g., "**/foo") - no prefix to normalize
    return pattern


def _match_redirect(
    target: str, config: Config, cwd: Path, *, remote: bool = False
) -> Match | None:
    """Match redirect target against rules. Returns last matching rule.

    Args:
        target: Redirect target to match.
        config: Configuration with redirect rules.
        cwd: Current working directory for path resolution.
        remote: If True, paths are NOT expanded against cwd (container/remote context).
    """
    # When remote, don't expand paths - match against literal target
    normalized_target = target if remote else _normalize_path(target, cwd)
    result: Match | None = None
    for rule in config.redirect_rules:
        # Patterns are always normalized as host paths (user's intent)
        normalized_pattern = _normalize_redirect_pattern(rule.pattern, cwd)
        if _glob_match(normalized_target, normalized_pattern):
            result = Match(
                decision=rule.decision,
                pattern=rule.pattern,
                message=rule.message,
                source=rule.source,
                scope=rule.scope,
            )
    return result


def match_command(
    cmd: SimpleCommand,
    config: Config,
    cwd: Path,
    context_flags: frozenset[str] | None = None,
    *,
    remote: bool = False,
) -> Match | None:
    """Match command and its redirects against config rules.

    Args:
        cmd: SimpleCommand with words and redirects from parsed bash.
        config: Loaded configuration.
        cwd: Current working directory for path resolution.
        context_flags: Optional set of active context flags (e.g., {"@subshell"}).
        remote: If True, command runs in remote context (container, ssh).
                Paths are NOT expanded against host cwd.

    Returns:
        Match object for the deciding rule, or None if no rules matched.
        Priority when combining command + redirect matches: deny > ask > allow.
        Returns the first match of the most restrictive decision type.
    """
    matches: list[Match] = []

    # Match command words
    cmd_match = _match_words(cmd.words, config, cwd, context_flags, remote=remote)
    if cmd_match:
        matches.append(cmd_match)

    # Match each redirect
    for target in cmd.redirects:
        redirect_match = _match_redirect(target, config, cwd, remote=remote)
        if redirect_match:
            matches.append(redirect_match)

    if not matches:
        return None

    # Priority: deny > ask > allow (most restrictive wins)
    for m in matches:
        if m.decision == "deny":
            return m
    for m in matches:
        if m.decision == "ask":
            return m
    return matches[0]


def match_redirect(
    target: str, config: Config, cwd: Path, *, remote: bool = False
) -> Match | None:
    """Match a redirect target against redirect rules.

    This is a convenience function for testing and for cases where you
    need to match a redirect target in isolation. Normally, redirects
    are matched as part of match_command() via SimpleCommand.redirects.

    Args:
        target: Redirect target path.
        config: Loaded configuration.
        cwd: Current working directory for path resolution.
        remote: If True, paths are NOT expanded against cwd (container/remote context).

    Returns:
        Match object for the last matching rule, or None if no match.
    """
    return _match_redirect(target, config, cwd, remote=remote)


def match_after(words: list[str], config: Config, cwd: Path) -> str | None:
    """Match command against after rules for PostToolUse feedback.

    Last matching rule wins. Empty string message means silent (no output).

    Args:
        words: Command words (e.g., ["git", "push", "origin", "main"]).
        config: Loaded configuration.
        cwd: Current working directory for path resolution.

    Returns:
        Message string if a rule with message matches, empty string if silent
        rule matches, None if no rule matches.
    """
    normalized_cmd = _normalize_words(words, cwd)
    result: str | None = None
    for rule in config.after_rules:
        normalized_pattern = _normalize_pattern(rule.pattern, cwd)
        matched = fnmatch.fnmatch(normalized_cmd, normalized_pattern)
        # Trailing ' *' also matches bare command (no args)
        if not matched and normalized_pattern.endswith(" *"):
            matched = normalized_cmd == normalized_pattern[:-2]
        if matched:
            # message is None for pattern-only rules, "" for explicit empty
            result = rule.message if rule.message is not None else ""
    return result


def match_mcp(tool_name: str, config: Config) -> Match | None:
    """Match MCP tool name against mcp rules.

    Simpler than command matching - just fnmatch against tool name.
    Last match wins.

    Args:
        tool_name: MCP tool name (e.g., "mcp__github__get_issue").
        config: Loaded configuration.

    Returns:
        Match object for the last matching rule, or None if no match.
    """
    result: Match | None = None
    for rule in config.mcp_rules:
        if fnmatch.fnmatch(tool_name, rule.pattern):
            result = Match(
                decision=rule.decision,
                pattern=rule.pattern,
                message=rule.message,
                source=rule.source,
                scope=rule.scope,
            )
    return result


def match_after_mcp(tool_name: str, config: Config) -> str | None:
    """Match MCP tool against after-mcp rules for PostToolUse feedback.

    Last matching rule wins. Empty string message means silent (no output).

    Args:
        tool_name: MCP tool name (e.g., "mcp__github__create_pr").
        config: Loaded configuration.

    Returns:
        Message string if a rule with message matches, empty string if silent
        rule matches, None if no rule matches.
    """
    result: str | None = None
    for rule in config.after_mcp_rules:
        if fnmatch.fnmatch(tool_name, rule.pattern):
            result = rule.message if rule.message is not None else ""
    return result


def match_web(query: str, config: Config) -> Match | None:
    """Match WebSearch query against web rules.

    Simpler than command matching - just fnmatch against query string.
    Last match wins.

    Args:
        query: WebSearch query string.
        config: Loaded configuration.

    Returns:
        Match object for the last matching rule, or None if no match.
    """
    result: Match | None = None
    for rule in config.web_rules:
        if fnmatch.fnmatch(query, rule.pattern):
            result = Match(
                decision=rule.decision,
                pattern=rule.pattern,
                message=rule.message,
                source=rule.source,
                scope=rule.scope,
            )
    return result


def match_after_web(query: str, config: Config) -> str | None:
    """Match WebSearch query against after-web rules for PostToolUse feedback.

    Last matching rule wins. Empty string message means silent (no output).

    Args:
        query: WebSearch query string.
        config: Loaded configuration.

    Returns:
        Message string if a rule with message matches, empty string if silent
        rule matches, None if no rule matches.
    """
    result: str | None = None
    for rule in config.after_web_rules:
        if fnmatch.fnmatch(query, rule.pattern):
            result = rule.message if rule.message is not None else ""
    return result


def match_edit(file_path: str, config: Config, cwd: Path) -> Match | None:
    """Match file path against edit rules for Write/Edit/MultiEdit tools.

    Uses same glob matching as redirect rules. Last matching rule wins.

    Args:
        file_path: Absolute path to the file being edited.
        config: Loaded configuration.
        cwd: Current working directory for path resolution.

    Returns:
        Match object for the last matching rule, or None if no match.
    """
    normalized_path = _normalize_path(file_path, cwd)
    result: Match | None = None
    for rule in config.edit_rules:
        normalized_pattern = _normalize_redirect_pattern(rule.pattern, cwd)
        if _glob_match(normalized_path, normalized_pattern):
            result = Match(
                decision=rule.decision,
                pattern=rule.pattern,
                message=rule.message,
                source=rule.source,
                scope=rule.scope,
            )
    return result


def match_read(file_path: str, config: Config, cwd: Path) -> Match | None:
    """Match file path against read rules for Read tool.

    Uses same glob matching as redirect rules. Last matching rule wins.

    Args:
        file_path: Absolute path to the file being read.
        config: Loaded configuration.
        cwd: Current working directory for path resolution.

    Returns:
        Match object for the last matching rule, or None if no match.
    """
    normalized_path = _normalize_path(file_path, cwd)
    result: Match | None = None
    for rule in config.read_rules:
        normalized_pattern = _normalize_redirect_pattern(rule.pattern, cwd)
        if _glob_match(normalized_path, normalized_pattern):
            result = Match(
                decision=rule.decision,
                pattern=rule.pattern,
                message=rule.message,
                source=rule.source,
                scope=rule.scope,
            )
    return result


# === Logging ===


@dataclass(frozen=True, slots=True)
class _LogConfig:
    """Internal log configuration."""

    path: Path
    full: bool = False


_log_config: _LogConfig | None = None
_log_disabled: bool = False  # Set on first failure, prevents repeated attempts


def configure_logging(config: Config) -> None:
    """Configure logging based on config settings. Call once at startup.

    Set DIPPY_TEST_NO_LOG=1 to disable logging (used in tests).
    """
    global _log_config, _log_disabled
    _log_disabled = False

    # Allow tests to disable logging via environment variable
    if os.environ.get("DIPPY_TEST_NO_LOG"):
        _log_config = None
        return

    # Disable hook-approvals.log if disabled
    if not config.log_hook_approvals:
        # Disable all logging by removing handlers and setting level to CRITICAL
        root_logger = logging.getLogger()
        root_logger.handlers.clear()
        root_logger.setLevel(logging.CRITICAL + 1)

    if config.log is None:
        _log_config = None
        return

    try:
        config.log.parent.mkdir(parents=True, exist_ok=True)
    except OSError:
        _log_config = None
        _log_disabled = True
        return

    _log_config = _LogConfig(path=config.log, full=config.log_full)


def log_decision(
    decision: str,
    cmd: str | None = None,
    rule: str | None = None,
    message: str | None = None,
    command: str | None = None,
    cwd: Path | None = None,
    tool: str | None = None,
    file_path: str | None = None,
    context_flags: frozenset[str] | None = None,
    agent: str | None = None,
) -> None:
    """Log a decision. No-op if logging not configured or disabled."""
    global _log_disabled
    import json
    from datetime import datetime, timezone

    if _log_config is None or _log_disabled:
        return

    entry: dict[str, str | None | list[str]] = {"decision": decision}
    if cmd is not None:
        entry["cmd"] = cmd
    if rule is not None:
        entry["rule"] = rule
    if message is not None:
        entry["message"] = message
    if _log_config.full and command is not None:
        entry["command"] = command
    if cwd is not None:
        entry["cwd"] = str(cwd)
    if tool is not None:
        entry["tool"] = tool
    if file_path is not None:
        entry["file_path"] = file_path
    if context_flags is not None and context_flags:
        entry["context_flags"] = sorted(context_flags)
    if agent is not None:
        entry["agent"] = agent
    entry["ts"] = datetime.now(timezone.utc).isoformat()

    try:
        with open(_log_config.path, "a") as f:
            f.write(json.dumps(entry) + "\n")
    except OSError:
        _log_disabled = True
