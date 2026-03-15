# Plan: Fix Dippy Hooks Installation - A1+ Quality

## Context

The previous implementation was BROKEN and created a mess:

1. **PreToolUse**: Still has OLD legacy hook `/home/michael/.../dippy-hook` (not replaced)
2. **PostToolUse**: Has NEW hook `dippy --claude` (added but PreToolUse wasn't updated)
3. **Duplicate/overlapping matchers**: New hooks added alongside old ones
4. **Missing hooks**: Only installed PreToolUse/PostToolUse, missing Notification/Stop hooks

### Root Cause

`_remove_dippy_hook()` only checks `command.lower().startswith("dippy")` which:
- ✗ Matches: `dippy --claude`
- ✗ Matches: `dippy-hook`
- ✗ FAILS: `/home/user/.../dippy-hook` (starts with `/`, not `dippy`)

## What Dippy Actually Supports

From `dippy.py` analysis, Dippy handles these hook events:

| Hook Event | Purpose | Currently Implemented? |
|------------|---------|------------------------|
| **PreToolUse** | Approve tools before execution | ✅ Yes |
| **PostToolUse** | Feedback after tool execution | ✅ Yes |
| **Notification** | Idle prompts | ✅ Partial (idle_prompt only) |
| **Stop** | Agent stop for idle notifier | ✅ Yes |
| **SubagentStop** | Subagent stop | ✅ Yes |
| **AfterAgent** | Alias for Stop | ✅ Yes |
| **PreCompact** | Before session compaction | ❌ MISSING |
| **SessionStart** | When session starts | ❌ MISSING |
| **UserPromptSubmit** | Before user prompt to LLM | ❌ MISSING |
| **PermissionRequest** | Permission approvals | ❌ MISSING |
| **SessionEnd** | When session ends | ❌ MISSING |

## Fix Plan

### 1. Fix `_remove_dippy_hook()` - CRITICAL

**File**: `src/dippy/cli/hooks.py`

```python
def _remove_dippy_hook(config: dict, agent: str) -> dict:
    result = copy.deepcopy(config)

    if agent in ("cursor", "windsurf"):
        # Cursor/Windsurf format
        if "hooks" in result:
            for hook_type, hooks_list in result["hooks"].items():
                result["hooks"][hook_type] = [
                    h for h in hooks_list
                    if not _is_dippy_hook(h)
                ]
    else:
        # Claude/Gemini/Windsurf format - check ALL supported hook types
        hook_types = ["PreToolUse", "PostToolUse", "Notification", "Stop", "SubagentStop", "AfterAgent"]

        if "hooks" in result:
            for hook_type in hook_types:
                if hook_type in result["hooks"]:
                    for entry in result["hooks"][hook_type]:
                        if "hooks" in entry:
                            entry["hooks"] = [
                                h for h in entry["hooks"]
                                if not _is_dippy_hook(h)
                            ]

    return result


def _is_dippy_hook(hook_obj: dict) -> bool:
    """Check if a hook object is a Dippy hook.

    Must distinguish between:
    - Legacy: /path/to/dippy-hook
    - New: dippy --claude
    - NOT a match: random command with "dippy" in path
    """
    if not isinstance(hook_obj, dict):
        return False

    command = hook_obj.get("command", "")
    if not command:
        return False

    cmd_lower = command.lower()

    # Match specific patterns:
    # 1. Starts with "dippy " (with space) or "dippy" at end (e.g., "dippy --claude")
    # 2. Ends with "dippy-hook" (legacy full path)
    # 3. Contains "/dippy" or "\\dippy" (legacy in path)
    # NOT: random command with "dippy" buried in middle

    return (
        # New style: "dippy --claude", "dippy --gemini"
        cmd_lower.startswith("dippy ") or cmd_lower == "dippy"
        # Legacy: any path ending with dippy-hook
        or cmd_lower.endswith("dippy-hook")
        # Legacy in path: /path/to/dippy, /path/to/dippy-hook
        or "/dippy" in cmd_lower or "\\dippy" in cmd_lower
    )
```

### 2. Support ALL Hook Types

**File**: `src/dippy/cli/hooks.py` - Update `HOOK_COMMANDS`

Define hook tiers for flexibility:

```python
# Minimal hooks (default when --all not specified)
MINIMAL_HOOKS = {
    "PreToolUse": [...],
    "PostToolUse": [...],
}

# Full hooks (when --all specified)
ALL_HOOKS = {
    "PreToolUse": [...],
    "PostToolUse": [...],
    "Notification": [{"matcher": "notification_type==idle_prompt", ...}],
    "Stop": [...],
    "SubagentStop": [...],
    "AfterAgent": [...],
}

# Future: Extended hooks (not yet implemented, for documentation)
EXTENDED_HOOKS = {
    "PreCompact": [...],        # TODO: implement
    "SessionStart": [...],      # TODO: implement
    "UserPromptSubmit": [...],  # TODO: implement
    "PermissionRequest": [...], # TODO: implement
    "SessionEnd": [...],        # TODO: implement
}
```

For Claude:

```python
"claude": {
    "config": "~/.claude/settings.json",
    "project_config": ".claude/settings.json",
    "hook_entry": {
        "hooks": {
            "PreToolUse": [
                {
                    "matcher": "Bash|Write|Edit|MultiEdit|Read|LS|Glob|Grep|Search|WebSearch|mcp__.*",
                    "hooks": [{"type": "command", "command": "dippy --claude"}]
                }
            ],
            "PostToolUse": [
                {
                    "matcher": "Bash|WebSearch|mcp__.*",
                    "hooks": [{"type": "command", "command": "dippy --claude"}]
                }
            ],
            # NEW: Notification hook for idle prompts
            "Notification": [
                {
                    "matcher": "notification_type==idle_prompt",
                    "hooks": [{"type": "command", "command": "dippy --claude"}]
                }
            ],
            # NEW: Stop hooks for idle notifier
            "Stop": [
                {
                    "hooks": [{"type": "command", "command": "dippy --claude"}]
                }
            ],
            "SubagentStop": [
                {
                    "hooks": [{"type": "command", "command": "dippy --claude"}]
                }
            ],
            "AfterAgent": [
                {
                    "hooks": [{"type": "command", "command": "dippy --claude"}]
                }
            ]
        }
    },
}
```

### 3. Add `--all` Flag for Complete Installation

**Usage**:
```bash
dippy hooks install claude --global --all    # Install ALL supported hooks (PreToolUse, PostToolUse, Notification, Stop, etc.)
dippy hooks install claude --global         # Install minimal hooks (PreToolUse, PostToolUse only - default)
```

**Implementation**:
```python
def install(agent: str, global_config: bool = False, cwd: str | None = None,
           force: bool = False, dry_run: bool = False, no_backup: bool = False,
           all_hooks: bool = False) -> int:
    """Install Dippy hooks.

    Args:
        all_hooks: If True, install ALL supported hooks. If False, install minimal set.
    """
    # Select hook entry based on --all flag
    if all_hooks:
        hook_entry = ALL_HOOKS[agent]["hook_entry"]
    else:
        hook_entry = MINIMAL_HOOKS[agent]["hook_entry"]
    ...
```

**File**: `src/dippy/dippy.py`

Add `--all` argument to install parser:
```python
install_parser.add_argument(
    "--all",
    action="store_true",
    help="Install ALL supported hooks (PreToolUse, PostToolUse, Notification, Stop, etc.)",
)
```

### 4. Fix Match Detection in `list_hooks`

The current code shows duplicated matchers because it extracts from config wrong. Need to:
1. Only show matchers for hooks that are actually Dippy hooks
2. Filter out memorix and other hooks

### 5. Update `uninstall` to Remove ALL Hooks

The uninstall command must also use `_is_dippy_hook()` to properly remove ALL Dippy hooks from ALL hook types, not just PreToolUse/PostToolUse.

```python
def uninstall(agent: str, global_config: bool = False, cwd: str | None = None, dry_run: bool = False) -> int:
    ...
    # Use same _is_dippy_hook() helper to find and remove ALL dippy hooks
    # from PreToolUse, PostToolUse, Notification, Stop, SubagentStop, AfterAgent
```

## Verification

1. `dippy hooks install claude --global --force` should:
   - Remove OLD `/path/to/dippy-hook` from PreToolUse
   - Add NEW `dippy --claude` to PreToolUse
   - NOT duplicate PostToolUse

2. `dippy hooks install claude --global --all --force` should:
   - Install hooks to: PreToolUse, PostToolUse, Notification, Stop, SubagentStop, AfterAgent
   - NOT touch other hooks (memorix, PreCompact, SessionStart, etc.)

3. `dippy hooks list --verbose` should show:
   - Correct matchers without duplicates
   - Only Dippy hooks, not memorix hooks

## Files to Modify

1. `src/dippy/cli/hooks.py` - Fix _remove_dippy_hook(), add _is_dippy_hook(), update HOOK_COMMANDS
2. `src/dippy/dippy.py` - Add --all flag, pass through to install()
3. `src/dippy/cli/doctor.py` - Update to show all hook types

## What NOT to Do (Lessons from Previous Failure)

❌ **WRONG**: `_remove_dippy_hook()` only checks `command.lower().startswith("dippy")`
- Fails to detect `/path/to/dippy-hook`
- Results in duplicate hooks

❌ **WRONG**: `_is_dippy_hook()` checks `"dippy" in command.lower()`
- Too broad - could match random commands with "dippy" in path
- Example: `/home/user/adippy-workspace/script.sh`

❌ **WRONG**: Merge logic that doesn't preserve non-Dippy hooks
- Other hooks (memorix, user-added) must be preserved

❌ **WRONG**: Only installing PreToolUse/PostToolUse
- Missing Notification, Stop hooks breaks idle features

✅ **CORRECT**: Check specific patterns
- `"dippy "` (with space) or `"dippy"` alone
- `/dippy` or `dippy-hook` in path
- NOT: random "dippy" in middle of string

## Critical Tests

### Test 1: Current Config (Broken State)
Before fix, ~/.claude/settings.json has:
```json
{
  "hooks": {
    "PreToolUse": [
      {"hooks": [{"command": "/home/michael/.../dippy-hook"}], "matcher": "..."}  // WRONG
    ],
    "PostToolUse": [
      {"hooks": [{"command": "memorix hook", ...}]},
      {"hooks": [{"command": "dippy --claude"}], "matcher": "..."}  // Added but PreToolUse not fixed
    ]
  }
}
```

### Test 2: After `dippy hooks install claude --global --force`
```json
{
  "hooks": {
    "PreToolUse": [
      {"hooks": [{"command": "dippy --claude", "type": "command"}], "matcher": "Bash|Write|Edit|..."}
    ],
    "PostToolUse": [
      {"hooks": [{"command": "memorix hook", ...}]},  // PRESERVED
      {"hooks": [{"command": "dippy --claude", "type": "command"}], "matcher": "Bash|WebSearch|..."}
    ]
  }
}
```

### Test 3: After `dippy hooks install claude --global --all --force`
```json
{
  "hooks": {
    "PreToolUse": [{"hooks": [{"command": "dippy --claude"}], "matcher": "..."}],
    "PostToolUse": [
      {"hooks": [{"command": "memorix hook"}]},
      {"hooks": [{"command": "dippy --claude"}], "matcher": "..."}
    ],
    "Notification": [{"hooks": [{"command": "dippy --claude"}], "matcher": "notification_type==idle_prompt"}],
    "Stop": [{"hooks": [{"command": "dippy --claude"}]}],
    "SubagentStop": [{"hooks": [{"command": "dippy --claude"}]}],
    "AfterAgent": [{"hooks": [{"command": "dippy --claude"}]}]
    // PreCompact, SessionStart, UserPromptSubmit preserved (not touched)
  }
}
```

### Test 4: Edge Cases

1. **Empty config**: Create new hooks section from scratch
2. **No hooks key**: Create hooks key and add hooks
3. **Mixed hooks**: Dippy hooks + other hooks (memorix, custom) - preserve all non-Dippy
4. **Legacy path at different location**: `/opt/dippy-hook`, `~/.local/bin/dippy-hook`
5. **Multiple legacy entries**: Remove ALL dippy-hook variants

### Test 5: list-hooks --verbose Output
```
[+] Claude Code
    global:  installed (dippy --claude) ~/.claude/settings.json
    Matchers:
      - PreToolUse: Bash|Write|Edit|MultiEdit|Read|LS|Glob|Grep|Search|WebSearch|mcp__.*
      - PostToolUse: Bash|WebSearch|mcp__.*
      - Notification: notification_type==idle_prompt
      - Stop: (all)
      - SubagentStop: (all)
      - AfterAgent: (all)
```

## Implementation Order

1. **First**: Fix `_remove_dippy_hook()` and add `_is_dippy_hook()` - CRITICAL
2. **Second**: Update HOOK_COMMANDS with minimal and full hook sets
3. **Third**: Add `--all` flag support to install()
4. **Fourth**: Fix `_extract_matchers_from_config()` to only show Dippy hooks
5. **Fifth**: Update doctor.py to show all hook types in verbose mode

## Rollback Plan

If something goes wrong:
1. Backup created automatically at `.dippy-backup-TIMESTAMP`
2. Restore: `cp ~/.claude/settings.json.dippy-backup-* ~/.claude/settings.json`
3. Last 5 backups kept automatically
