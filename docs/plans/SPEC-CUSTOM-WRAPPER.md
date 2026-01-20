# Custom Wrapper Command System

## Ralph Loop Instructions

**Read this spec completely at the start of each iteration.**

**Iteration workflow:**
1. Read this spec (`docs/plans/SPEC-CUSTOM-WRAPPER.md`)
2. Find the next uncompleted phase in Implementation Plan
3. Implement that phase
4. Run tests: `just test`
5. Fix any failures
6. If all acceptance criteria met and tests pass: output `<promise>CUSTOM_WRAPPERS_COMPLETE</promise>`
7. Otherwise, continue to next iteration

**Do not skip phases.** Work through them sequentially (1 → 2 → 3 → 4 → 5 → 6).

**After each code change:**
- Run `just test` to verify
- If tests fail, fix them before moving to next phase
- Check existing test files to understand patterns

## Overview

Enable project-specific wrapper commands (like `ssh`, but configurable) with automatic context flag extraction. Wrappers delegate to inner commands while adding wrapper name and destination as context flags for rule matching.

**Example use case:**
```
wrap server1 "free -h"          # wrapper=wrap, dest=server1, cmd=free -h
wrap server1 free -h            # same command without quotes
wrap server1 <<EOF
free -h
EOF                              # heredoc syntax

# Config rules:
allow [wrap,server1] free *     # allow free on server1 via wrap
allow [server2] ls *            # allow ls on server2 (any wrapper)
deny [server1] rm *             # deny rm on server1 (any wrapper)
```

## Requirements

### 1. Config Directive

Add `wrapper` directive to config parser:

```
wrapper <command_name>
```

**Examples:**
```
wrapper wrap
wrapper ssh
wrapper tmux-cli
```

**Validation:**
- Command name must be non-empty
- Warning on duplicate definition
- Store in `Config.wrappers: set[str]`

### 2. Wrapper Command Syntax

Wrapper commands follow this pattern:

```
<wrapper> [options] <destination> [inner_command]
```

**Components:**
- `wrapper`: Command name defined in config
- `options`: Optional flags (must skip option arguments like `-p 22`)
- `destination`: First non-option token (becomes context flag)
- `inner_command`: Everything after destination (can be empty for interactive)

**Examples:**
```
wrap server1 "free -h"           # inner: "free -h"
wrap server1 free -h             # inner: "free -h"
wrap -p 2222 server1 ls         # skips -p 2222, inner: "ls"
wrap server1                     # no inner → ask (interactive)
```

### 3. Context Flags

Wrapper extraction produces **two flags**:
- Wrapper name: `["wrap"]`
- Destination: `["server1"]`

Both added to `context_flags` set when analyzing inner command.

**Existing behavior:** `wrapper_context` is currently a string (e.g., `"ssh"`). Must change to **list of strings** to support multiple flags:
- Old: `wrapper_context="ssh"`
- New: `wrapper_context=["ssh", "server1"]`

**Analyzer integration:** In `_analyze_simple_command`, when handling delegate with `wrapper_context`, add all items to `context_flags`:
```python
if result.wrapper_context:
    context_flags.update(result.wrapper_context)
```

### 4. CLI Handler vs Analyzer

**Decision:** Implement wrapper extraction in **analyzer**, not as CLI handler.

**Reason:** CLI handlers are discovered at import time (before config loads). Wrappers are defined in config, so we can't register them as CLI handlers dynamically.

**Location:** `src/dippy/core/analyzer.py`, function `_analyze_simple_command`.

**Flow:**
```python
# Before calling get_handler()
if tokens[0] in config.wrappers:
    wrapper_name = tokens[0]
    dest, inner_cmd = extract_wrapper_args(tokens, config.wrappers)

    context_flags = {wrapper_name}
    if dest:
        context_flags.add(dest)

    # Delegate to inner command
    return analyze(inner_cmd, config, cwd, context_flags)
```

### 5. Argument Extraction Logic

Same as existing `ssh.py` handler:

1. Skip options with arguments: `-b`, `-c`, `-D`, `-E`, `-e`, `-F`, `-I`, `-i`, `-J`, `-L`, `-l`, `-m`, `-O`, `-o`, `-p`, `-Q`, `-R`, `-S`, `-W`, `-w`
2. Skip flag options: anything starting with `-` not in above list
3. First non-option token → **destination**
4. Everything after destination → **inner_command**
5. Special case: `--` ends option parsing

**Edge cases:**
- `wrap server1` (no inner) → return `Classification("ask", description="wrap server1")`
- `wrap` (no dest) → return `Classification("ask", description="wrap")`
- Empty inner command → treat as interactive (ask)

## Acceptance Criteria

### Config Parser
- [ ] `wrapper` directive parsed correctly
- [ ] Multiple wrappers stored in `Config.wrappers`
- [ ] Duplicate wrapper definition logged as warning
- [ ] Invalid wrapper name (empty, starts with `-`) logged as error

### Wrapper Extraction
- [ ] `wrap server1 "free -h"` extracts dest="server1", inner="free -h"
- [ ] `wrap server1 free -h` extracts dest="server1", inner="free -h"
- [ ] `wrap -p 2222 server1 ls` skips `-p 2222`, extracts dest="server1", inner="ls"
- [ ] `wrap server1` (no inner) returns ask with description
- [ ] `wrap` (no dest) returns ask with description
- [ ] Context flags include both wrapper name and destination

### Context Flags Integration
- [ ] `wrapper_context` changed from `str | None` to `list[str] | None`
- [ ] Existing ssh.py and sudo.py handlers updated to return list: `["ssh"]`, `["sudo"]`
- [ ] `_analyze_simple_command` adds all wrapper_context items to context_flags
- [ ] Existing tests for `[ssh]` and `[sudo]` flags still pass

### Rule Matching
- [ ] `allow [wrap,server1] free *` matches `wrap server1 free -h`
- [ ] `allow [server1] free *` matches (dest flag only)
- [ ] `allow [wrap] free *` matches (wrapper flag only)
- [ ] `deny [server1] rm *` blocks `wrap server1 rm /tmp/x`
- [ ] Flag matching works with negation: `[!ssh]` or `[!server1]`

### Tests
- [ ] Unit tests for wrapper extraction logic (various option combinations)
- [ ] Unit tests for context flags generation
- [ ] Integration tests with config parsing
- [ ] Integration tests with rule matching (allow/deny/ask)
- [ ] All existing tests pass (`just test`)
- [ ] Test coverage for heredoc syntax (if supported)

## Implementation Plan

### Phase 1: Config Parser
1. Add `wrappers: set[str]` field to `Config` dataclass in `src/dippy/core/config.py`
2. Add `wrapper` directive parser (similar to existing `allow`, `deny`, `set`)
3. Add validation and warning logs

### Phase 2: Classification Type Change
1. Change `wrapper_context` field in `Classification` dataclass (`src/dippy/cli/__init__.py`)
2. From: `wrapper_context: str | None = None`
3. To: `wrapper_context: list[str] | None = None`

### Phase 3: Update Existing Handlers
1. Update `src/dippy/cli/ssh.py`: `wrapper_context=["ssh"]` (was `"ssh"`)
2. Update `src/dippy/cli/sudo.py`: `wrapper_context=["sudo"]` (was `"sudo"`)
3. Run tests: `just test` - ensure nothing breaks

### Phase 4: Analyzer Integration
1. Update `_analyze_simple_command` in `src/dippy/core/analyzer.py`
2. When handling delegate with `wrapper_context`:
   ```python
   if result.wrapper_context:
       if isinstance(result.wrapper_context, str):
           # Backward compat (shouldn't happen after Phase 3)
           context_flags.add(result.wrapper_context)
       else:
           context_flags.update(result.wrapper_context)
   ```
3. Add wrapper extraction check before CLI handler call:
   ```python
   if tokens[0] in config.wrappers:
       # Extract wrapper args and delegate
   ```

### Phase 5: Wrapper Extraction Logic
1. Create `_extract_wrapper_args(tokens: list[str]) -> tuple[str | None, str]`
2. Implement option skipping (reuse logic from ssh.py)
3. Return (destination, inner_command)
4. Handle edge cases (no dest, no inner, interactive)

### Phase 6: Tests
1. Add `tests/test_custom_wrappers.py`
2. Test wrapper extraction with various option combinations
3. Test context flags generation
4. Test rule matching with wrapper flags
5. Integration tests with config files
6. Run `just test` - all tests pass

## Test Examples

```python
def test_wrapper_extraction_basic():
    config = parse_config("wrapper wrap")
    tokens = ["wrap", "server1", "free", "-h"]
    dest, inner = _extract_wrapper_args(tokens)
    assert dest == "server1"
    assert inner == "free -h"

def test_wrapper_extraction_with_options():
    tokens = ["wrap", "-p", "2222", "-l", "user", "server1", "ls"]
    dest, inner = _extract_wrapper_args(tokens)
    assert dest == "server1"
    assert inner == "ls"

def test_wrapper_context_flags():
    config = parse_config("wrapper wrap")
    result = analyze('wrap server1 free -h', config)
    # Inner command "free -h" analyzed with context_flags={"wrap", "server1"}

def test_wrapper_rule_matching():
    config = parse_config('''
    wrapper wrap
    allow [wrap,server1] free *
    deny [server1] rm *
    ''')

    assert analyze('wrap server1 free -h', config).decision == "allow"
    assert analyze('wrap server1 rm /tmp/x', config).decision == "deny"

def test_wrapper_without_inner():
    tokens = ["wrap", "server1"]
    result = _extract_wrapper_args(tokens)
    assert result == ("server1", "")  # or handle as ask

def test_wrapper_no_destination():
    tokens = ["wrap"]
    result = _extract_wrapper_args(tokens)
    assert result == (None, "")
```

## Success Criteria

**Promise tag for Ralph:**
```
<promise>CUSTOM_WRAPPERS_COMPLETE</promise>
```

**Definition of done:**
1. All acceptance criteria checked
2. All tests pass: `just test` (no failures)
3. Can define wrapper in config
4. Wrapper commands extract dest and inner correctly
5. Context flags work with allow/deny/ask rules
6. Existing ssh/sudo wrapper flags still work
7. Code follows project style (ruff format, ruff check passes)

## Debugging Commands

```bash
# Run tests
just test

# Run specific test file
python -m pytest tests/test_custom_wrappers.py -v

# Check parsing
echo 'wrapper wrap' | python -m dippy.core.config -

# Check with example config
cat .dippy-test | python -m dippy.core.config -
```

## Notes

- **Backward compatibility:** Existing ssh/sudo handlers must continue working
- **Type safety:** Update type hints for `wrapper_context: list[str] | None`
- **Logging:** Add debug logs for wrapper extraction (can be enabled with `DEBUG=1`)
- **Config priority:** Wrappers defined in project `.dippy` override global `~/.dippy/config`
- **Heredoc support:** Not required for initial implementation (shell expands before Dippy sees it)
