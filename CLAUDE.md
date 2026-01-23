# Dippy

Shell command approval hook for AI coding assistants.

## Commands

```bash
just test          # Run tests (Python 3.12, quiet mode - errors only)
just test-parallel # Run tests in parallel (with xdist -n auto)
just test-py312    # Run tests (Python 3.12, explicit)
just lint          # Lint (ruff check)
just fmt           # Format (ruff format)
just check         # All of the above in parallel — MUST PASS before committing
```

**Test output:**
- Default `just test`: quiet mode, shows only summary and errors (no progress dots)
- Use `just test-parallel` for faster parallel execution with xdist
- `just check` uses `test-parallel` for speed

## Project Context

- **Config**: `~/.dippy/config` (global), `.dippy/` (project-local)
- **Audit log**: `~/.dippy/audit.log` (JSONL format with cwd, decision, cmd, ts fields)
- **Entry point**: `log_decision()` in `src/dippy/core/config.py`
- **Standard logging**: goes to `~/.claude/hook-approvals.log`

## Process Rules

- research: ALWAYS web search for current best practices before implementing unfamiliar configs/patterns - user has no patience for trial-and-error experiments
- testing: use existing test suite (`just test`), never write adhoc tests
- testing: always isolate tests from live config/system using tmp_path, monkeypatch, and explicit isolation verification tests
- testing: TDD is mandatory for ALL changes including "small" bug fixes - write failing test FIRST, then implement fix
- verification: check actual running system (real files, real logs, real config) - never verify with synthetic examples when real system is accessible
- documentation: always read README.md before making assumptions about config/log locations
- documentation: README.md has priority for user-facing features - docs/ is for technical reference only
- documentation: update docs/README when adding/changing features
- documentation: keep docs minimal and tool-specific - don't explain technologies users already know
- backlog: use filters with `backlog task list` (e.g., `-p high -s todo`), never bare listing
- development: prefer simple KISS solutions over clever features - don't add overhead on every operation when once-per-day is sufficient
- development: avoid duplicate list maintenance - discover from code, never maintain separate constant lists (e.g., BUILTIN_COMMANDS)
- development: avoid imports inside functions - ugly pattern that violates code cleanliness
- background tasks: daily cleanup/rotation tasks should run once per relevant period, not on every startup/write
- log rotation: use yesterday's date for rotated files (active file always has current name)

## Technical Patterns

- context_flags: when delegating analysis, preserve outer context by combining with inner context_flags
- context_flags: when creating Decision objects, explicitly pass context_flags parameter (don't rely on defaults)
- type changes: when changing field types in dataclasses, update ALL consumers systematically (handlers, analyzer, tests)
- optional sets: use truthiness (`if value`) not identity (`if value is not None`) for optional frozensets - empty set is falsy but not None

## Communication

- communication: Czech phrases signal STOP - "kurva" (fundamental error requiring correction), "musi byt" (non-negotiable requirement), "nemam cas na pokusy" (no experiments allowed, research first)

## Git

- remotes: `original`=upstream (ldayton/Dippy), `origin`=fork (orgoj/Dippy), `tony`=contributor (tony-nekola-silk)
- attribution: when documenting fork features, run `git remote -v` first, use `git log --all --source` for commit origins
- operations: always check `git status` first to detect interrupted states
- merges: use worktrees for large upstream merges (see skill: safe-upstream-merge)
- commits: run `just test` BEFORE committing - NON-NEGOTIABLE, never commit failing tests or skip this step
- commits: use conventional format (feat:, fix:, chore:, docs:) with Co-Authored-By trailer
