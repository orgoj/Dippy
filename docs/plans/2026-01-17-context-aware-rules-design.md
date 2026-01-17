# Context-Aware Rules Design

## Overview

Add context flags to allow/deny/ask rules, enabling rules like "allow cd only in subshells" and "deny rm via ssh".

## Syntax

```
allow [flags] pattern
deny [flags] pattern
ask [flags] pattern
```

Flags in brackets, comma-separated. All flags must match (AND logic).

### Negation

Prefix with `!` to negate - flag must NOT be present:

```
deny [!@subshell] cd *         # deny cd when NOT in subshell
allow [ssh,!@pipeline] rm *    # ssh required, pipeline forbidden
```

### Flag Types

**AST Context Flags (@ prefix):**
- `@subshell` - inside `(...)`
- `@bracegroup` - inside `{ ...; }`
- `@pipeline` - part of `cmd1 | cmd2`
- `@compound` - any compound context (subshell, bracegroup, pipeline, list)

**Wrapper Command Flags (no prefix):**
- `ssh` - argument to ssh command
- `sudo` - argument to sudo/doas/pkexec command

### Examples

```bash
# Allow cd only in subshells (using negation)
deny [!@subshell] cd *

# Allow rm, but restrict via ssh
allow rm *
deny [ssh] rm *
allow [ssh] rm /tmp/**

# Combine contexts - ssh AND subshell required
allow [@subshell,ssh] dangerous-cmd

# Mixed required and negated - ssh but NOT pipeline
allow [ssh,!@pipeline] volatile-cmd
```

## Implementation Status

### Completed (v1)

**Scope:**
- `@subshell` flag
- Remove `cd` from `SIMPLE_SAFE`
- Add `allow [@subshell] cd *` to default config

**Changes:**

1. **Config Parser (`config.py`)**
   - Detect `[...]` prefix on rule lines
   - Parse comma-separated flags
   - Support `!` negation prefix
   - Store `required_flags: frozenset[str]` with each rule
   - Store `negated_flags: frozenset[str]` with each rule

2. **Analyzer (`analyzer.py`)**
   - Add `context_flags: set[str]` parameter to `_analyze_node()`
   - When entering subshell node: `context_flags | {"@subshell", "@compound"}`
   - Pass context through recursive calls
   - Pass context to rule matching

3. **Rule Matching**
   - Rule has required_flags → all must be in current context_flags
   - Rule has negated_flags → NONE must be in current context_flags
   - Rule has no flags → matches any context (backward compatible)

4. **Allowlists (`allowlists.py`)**
   - Remove `cd` from `SIMPLE_SAFE`

5. **Default Config**
   - Add: `allow [@subshell] cd *`

### Completed (v2) - Additional AST Flags

- `@bracegroup` flag for `{ ...; }` brace groups
- `@pipeline` flag for `cmd1 | cmd2` pipelines
- `@compound` meta-flag for any compound context

### Completed (v3) - Wrapper Command Flags

- `ssh` flag for commands like `ssh host "rm /tmp/x"`
- `sudo` flag for commands like `sudo rm /etc/passwd` (also covers `doas`, `pkexec`)

### Completed (v4) - Negation Syntax

- `!` prefix for negated flags
- Mixed required and negated flags in same rule

## Future Work

- Additional wrapper command flags as needed
- Per-command configuration in CLI handlers
