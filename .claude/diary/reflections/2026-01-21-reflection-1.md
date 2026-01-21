# Reflection: Last 2 Entries (Jan 19, 2026)

**Generated**: 2026-01-21
**Entries Analyzed**: 2
**Date Range**: 2026-01-19 to 2026-01-19

## Summary

Two diary entries from January 19, 2026 reveal consistent patterns around research-first development, test discipline, and user verification expectations. Both sessions involved technical implementations (pytest configuration, include directive) where mistakes occurred specifically from violating existing CLAUDE.md rules: implementing without research, using wrong tools (grep instead of sed), and verifying against synthetic examples instead of real config.

A critical pattern emerged: **violations of existing CLAUDE.md rules correlate directly with mistakes**. Entry 1 violated "RESEARCH-FIRST DEVELOPMENT" and made trial-and-error changes. Entry 2 violated "verification against actual system" and created synthetic test data. Both caused user frustration and rework.

The user's communication style provides clear signals: Czech phrases ("kurva", "musi byt", "nemam cas na pokusy") indicate fundamental misunderstandings requiring immediate attention. These aren't casual frustrations—they're correction signals.

## Patterns Identified

### Strong Patterns (2/2 entries)

1. **Research Before Implementation** (2/2 entries)
   - **Observation**: Both sessions had mistakes from skipping web research. Entry 1: pytest config trial-and-error. Entry 2: assumed relative paths work without verifying. User explicitly corrected: "najdi si to na webu jakto mas delat, nemam cas na pokusy"
   - **CLAUDE.md rule**: `- research: ALWAYS web search before implementing unfamiliar configs/patterns - user has no patience for trial-and-error`

2. **Verify Against Actual System** (2/2 entries)
   - **Observation**: Both sessions had verification errors from synthetic testing. Entry 1: assumed pytest flags work without testing. Entry 2: verified with synthetic config, user said "podivej se na ty soubory a zkontroluj to poradne". Real system beats synthetic tests.
   - **CLAUDE.md rule**: `- verification: check actual running system (real files, real logs, real config), never synthetic examples when real system is accessible`

3. **Test Isolation Is Critical** (2/2 entries)
   - **Observation**: Entry 2 emphasized test isolation extensively (monkeypatch HOME, tmp_path fixtures). User was explicitly worried about tests touching live config. Entry 1 had existing test suite that was trusted. Pattern: user prioritizes safety.
   - **CLAUDE.md rule**: `- testing: always isolate tests from live config/system - use tmp_path, monkeypatch, explicit verification tests`

4. **Czech Language = Correction Signal** (2/2 entries)
   - **Observation**: Both entries documented user's Czech phrases during frustration: "kurva" (fundamental misunderstanding), "musi byt" (non-negotiable), "nemam cas na pokusy" (no time for experiments). Pattern is consistent.
   - **CLAUDE.md rule**: `- communication: Czech phrases ("kurva", "musi byt") mean STOP - fundamental misunderstanding, requires immediate attention and correction`

5. **Commit Discipline** (2/2 entries)
   - **Observation**: Both entries emphasized running tests before committing. Entry 1: "run `just test` BEFORE committing". Entry 2: "MUST run `just test` BEFORE committing - never commit failing tests". Pattern: tests are gate, not suggestion.
   - **CLAUDE.md rule**: Already exists, should be strengthened to emphasize it's non-negotiable

### Emerging Patterns (single strong entry, worth noting)

1. **Tool Selection Pattern** (2/2 entries)
   - **Observation**: Entry 1 documented sed vs grep for filtering. Entry 2 documented Grep tool vs bash grep. Pattern: prefer tools that preserve exit codes and respect .gitignore.
   - **CLAUDE.md rule**: Already exists in global CLAUDE.md ("FORBIDDEN: NEVER use bash grep")

2. **Documentation Scope** (1/2 entries, but important)
   - **Observation**: Entry 2: Added docs to docs/config.md but user said "to musi byt v README.md". Pattern: user-facing features belong in README, not just technical docs.
   - **CLAUDE.md rule**: Already exists ("documentation: update docs/README when adding/changing features"), but should clarify README priority

## Proposed CLAUDE.md Updates

### Process Rules (new)
```
- research: ALWAYS web search for current best practices before implementing unfamiliar configs/patterns - user has no patience for trial-and-error experiments
- verification: check actual running system (real files, real logs, real config) - never verify with synthetic examples when real system is accessible
- testing: always isolate tests from live config/system using tmp_path, monkeypatch, and explicit isolation verification tests
```

### Communication (strengthen existing)
```
- communication: when user uses "kurva" repeatedly, pay attention - they're correcting fundamental misunderstandings
+ communication: Czech phrases signal STOP - "kurva" (fundamental error), "musi byt" (non-negotiable), "nemam cas na pokusy" (no experiments allowed)
```

### Git (strengthen existing)
```
- commits: run `just test` BEFORE committing, never commit failing tests
+ commits: run `just test` BEFORE committing - NON-NEGOTIABLE, never commit failing tests or skip this step
```

### Documentation (clarify existing)
```
- documentation: update docs/README when adding/changing features
+ documentation: README.md has priority for user-facing features - docs/ is for technical reference only
```

## Rule Violations Detected

Both entries show violations of **EXISTING** global CLAUDE.md rules that caused mistakes:

### Entry 1 Violations:
1. **RESEARCH-FIRST DEVELOPMENT** (global CLAUDE.md): "NEVER implement anything from memory or learned knowledge"
   - **Violation**: Started changing pytest config without web research
   - **User correction**: "najdi si to na webu jakto mas delat, nemam cas na pokusy"
   - **Action**: Strengthen this rule in project CLAUDE.md with local emphasis

2. **PYTHON OVER BASH** (global CLAUDE.md): "FOR COMPLEX FILE OPERATIONS: Use Python, NOT Bash"
   - **Violation**: Used grep for filtering instead of sed/python
   - **Result**: Pipeline broke due to exit code
   - **Action**: Already learned, sed pattern documented

### Entry 2 Violations:
1. **Verification discipline**: "check actual running system (real log files) instead of writing adhoc tests"
   - **Violation**: Verified with synthetic test data instead of user's actual ~/.dippy/config
   - **User correction**: "podivej se na ty soubory a zkontroluj to poradne"
   - **Action**: Strengthen verification rule

## One-Off Observations

### Technical Notes:
- **pytest-xdist output**: xdist adds progress bars separate from pytest core - can't be disabled via config, requires separate targets
- **sed vs grep filtering**: sed `/pattern/d` preserves exit codes, grep doesn't when no matches
- **Path resolution context**: Include directives resolve relative paths from including file's directory, not main config (like C `#include`)
- **justfile pipeline**: User's preferred pattern: `command 2>&1 | sed -u | tee /tmp/log.txt`

### User Preferences:
- **Python execution**: Always `uv run python` (not bare `python`)
- **Just over make**: Uses `just` for task automation
- **Conventional commits**: Always with Co-Authored-By trailer
- **Branch workflow**: Working on `orgoj` branch

## Metadata

**Entries analyzed:**
- `.claude/diary/2026-01-19-14-30-4282427c.md` (pytest configuration)
- `.claude/diary/2026-01-19-09-54-910b9d48.md` (include directive)

**Processing notes:**
- Both entries from same day, single user session context
- No Skills/Commands feedback to process
- All patterns backed by direct user quotes from diary entries
