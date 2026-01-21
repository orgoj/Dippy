# Session Diary - Implementation Lessons

**Date**: 2026-01-20
**Session ID**: f8a3c532-implementation
**Project**: /home/michael/work/ai/CLAUDE/TOOLS/dippy-dev
**Related Spec**: `docs/plans/SPEC-CUSTOM-WRAPPER.md`

## Task Summary
Implemented custom wrapper command system and context flags logging according to spec. Session involved significant debugging, learning from user feedback about KISS principles, and discovering multiple subtle bugs in context flags propagation.

## Critical Bug: _combine() Wrong Condition Check

### The Bug
```python
# WRONG - checks for "not None"
context_flags = next((d.context_flags for d in decisions if d.context_flags is not None), None)
```

### Why It Failed
- Empty `frozenset()` is not `None`, but it's falsy
- When some decisions had empty context_flags, the selector skipped them
- Led to wrong context_flags being selected

### The Fix
```python
# CORRECT - checks for truthiness
context_flags = next((d.context_flags for d in decisions if d.context_flags), None)
```

**Lesson**: When filtering optional sets, use truthiness (`if value`) not identity (`if value is not None`).

## Critical Bug: Context Flags Lost During Delegation

### The Bug
```python
# WRONG - returns inner_decision directly, loses wrapper flags
inner_decision = analyze(result.inner_command, config, cwd, inner_flags)
return inner_decision
```

### Why It Failed
- `inner_flags` was computed correctly (wrapper flags + outer flags)
- But returned `inner_decision.context_flags` only had flags from inner analysis
- Wrapper flags from `inner_flags` were discarded

### The Fix
```python
# CORRECT - preserve wrapper flags in final decision
inner_decision = analyze(result.inner_command, config, cwd, inner_flags)
if inner_decision.context_flags:
    inner_decision.context_flags = inner_decision.context_flags | inner_flags
else:
    inner_decision.context_flags = inner_flags
return inner_decision
```

**Lesson**: When delegating analysis, always preserve outer context by combining with inner context_flags.

## Critical Bug: Missing context_flags in Decision Creations

### The Bug
```python
# WRONG - Decision without context_flags
return Decision("ask", base)
return Decision("allow", base)
return Decision("deny", f"{cmd} not in allowlist")
```

### Why It Failed
- After adding `context_flags` field to Decision dataclass, many creation sites didn't pass it
- Led to decisions having `context_flags=None` when they should have flags

### The Fix
Added `context_flags=context_flags` to ALL Decision creations in `_analyze_simple_command`:
```python
# CORRECT - always pass context_flags
return Decision("ask", base, context_flags=context_flags)
return Decision("allow", base, context_flags=context_flags)
return Decision("deny", f"{cmd} not in allowlist", context_flags=context_flags)
```

**Lesson**: When adding new fields to dataclasses, update ALL creation sites systematically.

## User Feedback: KISS Principle Violation

### The Error
I initially created:
1. `BUILTIN_COMMANDS` constant - manual list of wrapper names
2. `_get_builtin_commands()` function - imports inside function to discover builtins
3. Wrapper conflict validation - checks if user-defined wrappers conflict with builtins

### User's Response (translated)
- "why are you making BUILTIN_COMMANDS, it must be discoverable from code, we won't maintain extra list"
- "what the fuck is imports in function? you don't need to make builtin list"
- "don't make it complicated, maintain code cleanliness and KISS principles"

### What I Did Wrong
1. **Duplicate list maintenance**: `BUILTIN_COMMANDS` duplicates what's already in code (ssh.py, sudo.py)
2. **Imports inside function**: `_get_builtin_commands()` imported handlers inside function - ugly pattern
3. **Over-engineering**: Added validation complexity when users can define wrappers however they want

### The Fix
**Completely removed all validation logic**:
- No BUILTIN_COMMANDS constant
- No _get_builtin_commands() function
- No wrapper conflict validation
- KISS approach: users define wrappers in config, analyzer checks `tokens[0] in config.wrappers`

**Lesson**: **Always ask: is this necessary?** If users can define wrappers however they want, validation adds complexity without value. KISS > clever features.

## Bug: Logging Tests Failed - DIPPY_TEST_NO_LOG

### The Error
Tests in `test_logging_enhancements.py` failed with empty log files.

### Root Cause
`DIPPY_TEST_NO_LOG` environment variable was set, disabling logging in tests.

### The Fix
Temporarily unset `DIPPY_TEST_NO_LOG` in tests, then restore:
```python
old_val = os.environ.get("DIPPY_TEST_NO_LOG")
os.environ.pop("DIPPY_TEST_NO_LOG", None)
try:
    # ... test code with logging enabled ...
finally:
    if old_val is not None:
        os.environ["DIPPY_TEST_NO_LOG"] = old_val
```

**Lesson**: When testing features that disable logging during normal test runs, temporarily re-enable them in specific tests.

## Bug: Test Expectation Wrong - Rule Matching Order

### The Error
Test `test_wrapper_specific_over_generic` expected deny rule to override allow rule.

### Root Cause
Misunderstanding of "last-match-wins" - assumed more specific flags would win, but spec says later rules always win regardless of flag specificity.

### The Fix
Renamed test to `test_wrapper_last_match_wins_with_flags` and corrected expectation to "allow" since it's the last matching rule.

**Lesson**: Rule matching is purely order-dependent, not specificity-dependent. Last match wins, always.

## Type Change Ripple Effect: str → list[str]

### The Change
Changed `wrapper_context` from `str | None` to `list[str] | None`.

### Impact
Had to update ALL consumers:
1. `src/dippy/cli/__init__.py` - Classification dataclass definition
2. `src/dippy/cli/ssh.py` - Return `["ssh"]` instead of `"ssh"`
3. `src/dippy/cli/sudo.py` - Return `["sudo"]` instead of `"sudo"`
4. `tests/cli/test_ssh.py` - Test assertions from `== "ssh"` to `== ["ssh"]`
5. `tests/cli/test_sudo.py` - Test assertions from `== "sudo"` to `== ["sudo"]`

**Lesson**: Type changes have ripple effects. Update ALL consumers systematically, not just definition.

## Code Formatting Discipline

### The Process
After implementing changes, ran:
```bash
just fmt --fix  # Format 4 files with ruff
```

### Files Formatted
- `src/dippy/core/analyzer.py`
- `src/dippy/core/config.py`
- `tests/test_custom_wrappers.py`
- `tests/test_custom_wrappers_rule_matching.py`

**Lesson**: Always format code before completing work. Ruff formatting is part of project discipline.

## Test Results

### Before Implementation
- 9757 tests passing

### After Implementation
- 9793 tests passing (36 new tests added)
- All existing tests still pass - backward compatibility confirmed

### New Test Files
1. `tests/test_custom_wrappers.py` - 25 tests for wrapper extraction and analysis
2. `tests/test_custom_wrappers_rule_matching.py` - 8 tests for rule matching
3. `tests/test_logging_enhancements.py` - 3 tests for audit logging

**Lesson**: Comprehensive test coverage catches bugs early. 36 tests for one feature is appropriate for complex logic.

## Backward Compatibility Verification

### What Changed
- Added `context_flags` parameter to `log_decision()`
- Added `context_flags` field to `Decision` dataclass
- Added context flags to audit log JSON

### What Stayed Same
- Existing configs work unchanged
- Normal commands (direct "ls") have `context_flags=None`
- Audit logs only include `context_flags` field when non-None/empty

### Verification
```python
# Direct command: no context flags
analyze("ls", config, cwd)  # context_flags=None

# Subshell: has context flags
analyze("$(ls)", config, cwd)  # context_flags=frozenset({"@subshell"})

# Wrapper: has context flags
analyze("wrap server1 free -h", config, cwd)  # context_flags=frozenset({"wrap", "server1"})
```

**Lesson**: Additive changes (new fields that default to None) are backward compatible. Logs only include new fields when they exist.

## Files Modified

### Core Implementation
- `src/dippy/core/config.py` - Added wrappers field, wrapper parser, context_flags logging
- `src/dippy/core/analyzer.py` - Added wrapper extraction, context_flags to Decision, debug logging
- `src/dippy/dippy.py` - Pass context_flags to log_decision()

### Type Change Impact
- `src/dippy/cli/__init__.py` - Changed wrapper_context to list[str]
- `src/dippy/cli/ssh.py` - Return list instead of string
- `src/dippy/cli/sudo.py` - Return list instead of string

### Tests
- `tests/cli/test_ssh.py` - Updated assertions for list type
- `tests/cli/test_sudo.py` - Updated assertions for list type
- `tests/test_custom_wrappers.py` - NEW (25 tests)
- `tests/test_custom_wrappers_rule_matching.py` - NEW (8 tests)
- `tests/test_logging_enhancements.py` - NEW (3 tests)

### Documentation
- `docs/config.md` - Added "Custom Wrappers" section
- `README.md` - Added custom wrapper example
- `docs/plans/SPEC-CUSTOM-WRAPPER.md` - Implementation spec (created earlier)

### Diary
- `.claude/diary/2026-01-20-09-23-49dfa926.md` - First diary entry
- `.claude/diary/2026-01-20-14-45-f8a3c532.md` - Brainstorming session diary

## Commit

### Commit Message
```
feat: add custom wrapper system and context flags logging

Implement user-defined wrapper commands that extract destination and
inner command, adding both as context flags for rule matching.

- Add `wrapper` directive to config for custom wrapper definitions
- Change wrapper_context from str to list[str] to support multiple flags
- Add context_flags to Decision dataclass and audit log entries
- Implement wrapper extraction logic with SSH-style option parsing
- Add 33 new tests covering wrapper extraction and rule matching
- Update documentation in README.md and docs/config.md

Backward compatible: existing configs work unchanged, context_flags
only added to logs when commands run in specific contexts.

Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>
```

### Commit Hash
`6e89d71` on branch `orgoj-dev`

## Key Lessons for Future Sessions

### Technical Lessons
1. **Context flags propagation is tricky** - must preserve through delegation and combining
2. **Type changes ripple everywhere** - update ALL consumers, not just definition
3. **Truthiness vs identity** - use `if value` for optional sets, not `if value is not None`
4. **Dataclass fields** - add defaults to maintain backward compatibility

### Process Lessons
1. **KISS is paramount** - don't add complexity unless absolutely necessary
2. **User feedback signals** - repeated "kurva" means fundamental misunderstanding
3. **Test coverage** - comprehensive tests catch subtle bugs in complex logic
4. **Run tests before commit** - never commit failing tests

### Code Quality Lessons
1. **No duplicate lists** - discover from code, don't maintain manually
2. **No imports in functions** - ugly pattern, avoid it
3. **Format before committing** - use `just fmt --fix`
4. **Ask before adding complexity** - validation may not be needed

### Communication Lessons
1. **"Kurva" as signal** - user correcting fundamental mistakes
2. **User knows KISS** - prefer simple solutions over clever ones
3. **Czech language** - user communicates in Czech, respond accordingly

## Integration Test Results

```
wrap server1 free -h: allow ✓
wrap server1 ls -la: allow ✓
wrap server1 rm /tmp/x: deny ✓
wrap server2 anything: allow ✓
wrap -p 2222 server1 free -h: allow ✓
```

All integration tests passed, confirming the feature works end-to-end.

## Next Steps

Feature is complete and ready for release:
- All acceptance criteria met
- All tests passing (9793 tests)
- Backward compatibility verified
- Documentation updated
- Commit created (6e89d71)

Release when ready - no blockers.
