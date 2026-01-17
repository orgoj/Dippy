---
id: TASK-2
title: Jak povolit cd jen v subshell?
status: Done
assignee:
  - '@myself'
created_date: '2026-01-16 06:23'
updated_date: '2026-01-17 18:14'
labels: []
milestone: m-0
dependencies: []
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
je treba udelat brainstorm skill

- detekuje to AST ? existuje nejaky flag ?
- jak to definovat v allow, nejaka if flag?
- mozna nejaky allow [subshell] cd  - v [] by mohl byt seznam flagu ktere musi platit a command parser by nastavil flag pokud jsou programy v ()
- to by slou pouzit do budoucna treba pro prikasy pres ssh to by nastavilo flag ssh a allow [ssh] by matchovalo jen ssh prikazy
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Config parser recognizes [flags] syntax in rules
Analyzer tracks @subshell context during AST walk
Rule matching respects context flags (AND logic)
cd removed from SIMPLE_SAFE
Default config includes: allow [@subshell] cd *
Tests pass for subshell cd allowed, standalone cd denied
Backward compatible - rules without flags work as before

- [ ] #2 [x] Config parser recognizes [flags] syntax in rules
[x] Analyzer tracks @subshell context during AST walk
[x] Rule matching respects context flags (AND logic)
[x] cd removed from SIMPLE_SAFE
[ ] Tests pass for subshell cd allowed, standalone cd denied
[ ] Backward compatible - rules without flags work as before

- [ ] #3 [x] Config parser recognizes [flags] syntax in rules
[x] Analyzer tracks @subshell context during AST walk
[x] Rule matching respects context flags (AND logic)
[x] cd removed from SIMPLE_SAFE
[x] Tests pass for subshell cd allowed, standalone cd denied
[x] Backward compatible - rules without flags work as before
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Add flag parsing to config.py (detect [flags] syntax)
2. Extend analyzer.py to track @subshell context
3. Pass context to rule matching
4. Remove cd from SIMPLE_SAFE in allowlists.py
5. Add default rule: allow [@subshell] cd *
6. Add tests
7. Update documentation
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
## Implementation Summary

Implemented context-aware rules for allow/deny/ask directives.

### Syntax
```
allow [flags] pattern
deny [flags] pattern
ask [flags] pattern "message"
```

Flags in brackets, comma-separated, AND logic (all must match).

### Flag Types
- AST flags: `@subshell` (prefix with @)
- Wrapper flags: future - `ssh`, `sudo` (no prefix)

### Changes

**config.py:**
- Added `required_flags: frozenset[str] | None` to Rule dataclass
- Added `_extract_context_flags()` to parse [flags] syntax
- Updated `_match_words()` and `match_command()` to accept context_flags
- Rules with flags only match when all flags are present in context

**analyzer.py:**
- Added `context_flags` parameter to `analyze()` and all internal functions
- Subshell nodes set `@subshell` flag for their body
- Context flags flow through entire AST traversal

**allowlists.py:**
- Removed `cd` from SIMPLE_SAFE

### Documentation
- docs/config.md - Added "Context Flags" section with syntax, examples, how it works
- README.md - Added context-aware rules example in Configuration section
- docs/plans/2026-01-17-context-aware-rules-design.md - Design document

### Tests
- tests/test_config.py - TestContextFlags class (13 tests)
- tests/test_analyzer_bugs.py - TestSubshellContext class (8 tests)

### Files Modified
- src/dippy/core/config.py
- src/dippy/core/analyzer.py
- src/dippy/core/allowlists.py
- tests/test_config.py
- tests/test_analyzer_bugs.py
- docs/config.md
- README.md
- docs/plans/2026-01-17-context-aware-rules-design.md

### Usage Example
```
deny cd *
allow [@subshell] cd *
```

This denies standalone `cd /path` but allows `(cd /path && make)`.
<!-- SECTION:NOTES:END -->
