# Context-Aware Allow Rules Design

## Overview

Add context flags to allow/deny/ask rules, enabling rules like "allow cd only in subshells".

## Syntax

```
allow [flags] pattern
deny [flags] pattern
ask [flags] pattern
```

Flags in brackets, comma-separated. All flags must match (AND logic).

### Flag Types

**AST Context Flags (@ prefix):**
- `@subshell` - inside `(...)`
- `@bracegroup` - inside `{ ...; }` (future)
- `@pipeline` - part of `cmd1 | cmd2` (future)
- `@compound` - any compound context (future)

**Wrapper Command Flags (no prefix):**
- `ssh` - argument to ssh command (future)
- `sudo` - argument to sudo command (future)
- Any command from `WRAPPER_COMMANDS` (future)

### Examples

```bash
# Allow cd only in subshells
deny cd *
allow [@subshell] cd *

# Allow rm, but restrict via ssh
allow rm *
deny [ssh] rm *
allow [ssh] rm /tmp/**

# Combine contexts
allow [@subshell,ssh] dangerous-cmd
```

## V1 Implementation (Minimal)

### Scope
- `@subshell` flag only
- Remove `cd` from `SIMPLE_SAFE`
- Add `allow [@subshell] cd *` to default config

### Changes

**1. Config Parser (`config.py`)**
- Detect `[...]` prefix on rule lines
- Parse comma-separated flags
- Store `required_flags: set[str]` with each rule

**2. Analyzer (`analyzer.py`)**
- Add `context_flags: set[str]` parameter to `_analyze_node()`
- When entering subshell node: `context_flags | {"@subshell"}`
- Pass context through recursive calls
- Pass context to rule matching

**3. Rule Matching**
- Rule has required_flags → all must be in current context_flags
- Rule has no flags → matches any context (backward compatible)

**4. Allowlists (`allowlists.py`)**
- Remove `cd` from `SIMPLE_SAFE`

**5. Default Config**
- Add: `allow [@subshell] cd *`

## Future Work

See backlog tasks for:
- Additional AST flags (@bracegroup, @pipeline, @compound)
- Wrapper command flags (ssh, sudo, pkexec, etc.)
- Negation syntax [!@subshell]
