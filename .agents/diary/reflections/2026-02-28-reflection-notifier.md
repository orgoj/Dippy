# Reflection: Notifier Feature & Process Refinement

**Generated**: 2026-02-28
**Entries Analyzed**: 3
**Date Range**: 2026-02-07 to 2026-02-09

## Summary
The recent sessions focused on implementing the "Universal Notifier" (Sidekick) feature, which transforms Dippy from a pure permission gatekeeper into a context-aware assistant. Key technical additions include idle long-polling hooks and filtering mechanisms. Process observations reinforced the importance of TDD, testing reliability preferences, and proper migration paths for configuration structures.

## Patterns Identified

### Strong Patterns (3/3 occurrences)
1. **Test before commit** (3/3 entries)
   - Observation: `just test` was run and passed before every single commit.
   - CLAUDE.md rule: `- commits: run `just test` BEFORE committing - NON-NEGOTIABLE.` (Already present, reinforced).

2. **Conventional commits** (3/3 entries)
   - Observation: Consistent use of `feat:`, `fix:`, `chore:`, `docs:` prefixes.
   - CLAUDE.md rule: `- commits: use conventional format (feat:, fix:, chore:, docs:)` (Already present).

3. **Co-authored-by trailers** (3/3 entries)
   - Observation: All commit messages included `Co-Authored-By` trailer.
   - CLAUDE.md rule: `- commits: use conventional format (feat:, fix:, chore:, docs:) with Co-Authored-By trailer` (Already present).

### Reinforced Patterns (2/3 occurrences)
1. **TDD is mandatory** (2/3 entries)
   - Observation: First two entries explicitly mentioned the need to write tests FIRST before implementation.
   - CLAUDE.md rule: `- testing: TDD is mandatory for ALL changes including "small" bug fixes - write failing test FIRST, then implement fix` (Already present, reinforced).

2. **KISS principle** (2/3 entries)
   - Observation: User preferred simple text/XML interface over complex JSON schemas.
   - CLAUDE.md rule: `- development: prefer simple KISS solutions over clever features` (Already present).

### Emerging Patterns (1/3 occurrences)
1. **Testing reliability preference**
   - Observation: First two entries mentioned that `uv run python -m pytest` is more reliable than `just` in restricted environments.
   - CLAUDE.md rule: `- testing: prefer `uv run python -m pytest` over `just` in restricted environments for reliability` (NEW).

2. **Repetitive loop anti-pattern**
   - Observation: First two entries mentioned getting stuck in verification loops before fixing root cause in config loading.
   - Root cause: Rushing to end-to-end testing before unit testing configuration plumbing.

## New Technical Information

### Notifier (Sidekick) Feature
- Configuration: `set notifier-command`, `set notifier-include`
- Purpose: External commands can inject context into agent sessions
- Implementation:
  - `pi-mono`: Uses `agent_end` event with `--idle` flag for long-polling
  - Uses `pi.sendUserMessage` with `deliverAs: "followUp"` for agent-initiated turns
  - Claude Code/Gemini CLI: `PostToolUse` and `Stop`/`AfterAgent` hooks

### Allowlist Override Mechanism
- **SIMPLE_SAFE/WRAPPER_COMMANDS**: Cannot be disabled via `set` directive (hard-coded in allowlists.py)
- **BUT**: Config rules have higher priority (checked at `analyzer.py:656` before allowlists at `analyzer.py:699`)
- This means config rules CAN effectively override allowlists

### Plan Mode Clarification
- Plan mode is **NOT a Dippy feature**
- It's a pi-mono feature (located in `examples/extensions/plan-mode/`)
- Exit via `/plan` command or Ctrl+Alt+P

### VSCode Extension Maintenance
- Syntax highlighting requires manual regex updates in `editors/vscode/syntaxes/dippy.tmLanguage.json`
- Each new directive must be added to the keyword matching pattern

## Proposed CLAUDE.md Updates

### Dippy Configuration
```markdown
- allowlists: SIMPLE_SAFE commands cannot be overridden by `set` directive, but CAN be overridden by config rules (rules have higher priority)
- notifier: `notifier-command` and `notifier-include` directives for sidekick context injection (v0.2.5+)
```

### Process Rules
```markdown
- testing: prefer `uv run python -m pytest` over `just` in restricted environments for reliability
- pi-mono: plan mode is a pi-mono feature, not Dippy (exit via `/plan` or Ctrl+Alt+P)
```

### Technical Patterns
```markdown
- pi-mono: use `deliverAs: "followUp"` for agent-initiated turns to prevent collisions with user input
- notifier: `agent_end` hook enables long-polling idle behaviors with `--idle` flag
```

### Documentation
```markdown
- documentation: VSCode extension requires manual regex updates in `editors/vscode/syntaxes/dippy.tmLanguage.json` when adding new directives
```

## One-Off Observations
- Version 0.2.5 marks a significant shift for Dippy from "permission gatekeeper" to "context-aware assistant sidekick"
- Quote stripping is necessary for custom `set` directives in Dippy's parser
- Using `pytest` directly is often more reliable than wrapper tools like `just` in restricted environments

## Anti-Patterns to Avoid
1. **Repetitive verification loops**: Fix root cause in config loading before attempting end-to-end tests
2. **Assuming tool availability**: Check `PATH` for tools like `just` before relying on them
3. **Rushing to verify**: Ensure "plumbing" (dataclass instantiation, config loading) is updated before testing features

## Metadata
- Entries analyzed:
    - 2026-02-07-23-25-NOTIFIER-FEAT.md
    - 2026-02-08-11-30-NOTIFIER-FILTER-FINAL.md
    - 2026-02-09-10-38-59667466.md
- CLAUDE.md updates applied: 5 edits (Dippy Configuration, Process Rules, Technical Patterns, Documentation)
