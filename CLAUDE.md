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
- backlog: use filters with `backlog task list` (e.g., `-p high -s todo`), never bare listing
