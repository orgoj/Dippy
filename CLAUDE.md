# Dippy

Shell command approval hook for AI coding assistants.

## Commands

```bash
just test        # Run tests (Python 3.14)
just test-all    # All Python versions (3.11-3.14)
just lint        # Lint (ruff check)
just fmt         # Format (ruff format)
just check       # All of the above in parallel — MUST PASS before committing
```

## Project Context

- **Config**: `~/.dippy/config` (global), `.dippy/` (project-local)
- **Audit log**: `~/.dippy/audit.log` (JSONL format with cwd, decision, cmd, ts fields)
- **Entry point**: `log_decision()` in `src/dippy/core/config.py`
- **Standard logging**: goes to `~/.claude/hook-approvals.log`

## Process Rules

- testing: use existing test suite (`just test`), never write adhoc tests
- verification: check actual running system (real log files) instead of writing adhoc tests
- documentation: always read README.md before making assumptions about config/log locations
- documentation: update docs/README when adding/changing features
- documentation: keep docs minimal and tool-specific - don't explain technologies users already know
- backlog: use filters with `backlog task list` (e.g., `-p high -s todo`), never bare listing
- development: prefer simple KISS solutions over clever features - don't add overhead on every operation when once-per-day is sufficient
- background tasks: daily cleanup/rotation tasks should run once per relevant period, not on every startup/write
- log rotation: use yesterday's date for rotated files (active file always has current name)

## Communication

- communication: when user uses "kurva" repeatedly, pay attention - they're correcting fundamental misunderstandings

## Git

- remotes: `original`=upstream (ldayton/Dippy), `origin`=fork (orgoj/Dippy)
- operations: always check `git status` first to detect interrupted states
- merges: use worktrees for large upstream merges (see skill: safe-upstream-merge)
- commits: run `just test` BEFORE committing, never commit failing tests
- commits: use conventional format (feat:, fix:, chore:, docs:) with Co-Authored-By trailer
