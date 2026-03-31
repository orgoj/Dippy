# Dippy

Shell command approval hook for AI coding assistants.

## **CRITICAL: ENGLISH-ONLY CODE (NON-NEGOTIABLE!)**

**ALL source code, documentation, comments, test messages, and user-facing strings MUST be in English ONLY!**

**FORBIDDEN:**
- ❌ Czech text in ANY file (src/, tests/, docs/, *.md)
- ❌ Czech words like "rekurzivně", "příkazy", "konfigurace", etc.
- ❌ Czech comments in code
- ❌ Czech diacritics: č, ř, ž, š, ň, ě, ť, ď, Ě, Š, Č, Ř, Ž, Ý, Á, Í, É

**MANDATORY:**
- ✅ Write EVERYTHING in English - code, docs, comments, tests, logs
- ✅ If user speaks Czech, respond in Czech BUT write code in English
- ✅ Check files for Czech text before committing: `rg "[čřžšňěťďĚŠČŘŽÝÁÍÉ]"`

**This applies to:** .py files, .md files, test files, documentation, comments, EVERYTHING!

## Commands

```bash
just test          # Run tests (Python 3.12, quiet mode - errors only)
just test-parallel # Run tests in parallel (with xdist -n auto)
just test-py312    # Run tests (Python 3.12, explicit)
just lint          # Lint (ruff check)
just fmt           # Format (ruff format)
just check         # All of the above in parallel — MUST PASS before committing

## Debugging Tools

- `scripts/debug/check-path.py` — Verifies a specific file path against the active Dippy configuration. Useful for diagnosing why a `Read` or `Edit` operation is being blocked or asked.
  - **Usage:** Edit the `path` variable in the script and run: `export PYTHONPATH=$PYTHONPATH:$(pwd)/src && python3 scripts/debug/check-path.py`
```

**Test output:**
- Default `just test`: quiet mode, shows only summary and errors (no progress dots)
- Use `just test-parallel` for faster parallel execution with xdist
- `just check` uses `test-parallel` for speed

## Project Context

- **Config**: `~/.dippy/config` (global), `.dippy/` (project-local)
- **Audit log**: `~/.dippy/audit.log` (JSONL format with cwd, decision, cmd, agent, ts fields)
- **Entry point**: `log_decision()` in `src/dippy/core/config.py`
- **Standard logging**: goes to `~/.claude/hook-approvals.log`

## Process Rules

- plan mode: wait for explicit user confirmation before calling ExitPlanMode - never assume readiness
- sources: check local repositories (~/work/ai/) before web searches or GitHub API calls
- research: ALWAYS web search for current best practices before implementing unfamiliar configs/patterns - user has no patience for trial-and-error experiments
- testing: use existing test suite (`just test`), never write adhoc tests
- testing: use fictional commands in config rule tests to avoid SIMPLE_SAFE allowlist interference
- testing: always isolate tests from live config/system using tmp_path, monkeypatch, and explicit isolation verification tests
- testing: TDD is mandatory for ALL changes including "small" bug fixes - write failing test FIRST, then implement fix
- testing: run expensive commands once to /tmp/file.txt 2>&1, analyze with grep/wc/head/tail - never re-run for different views
- testing: prefer `uv run python -m pytest` over `just` in restricted environments for reliability
- verification: check actual running system (real files, real logs, real config) - never verify with synthetic examples when real system is accessible
- verification: never assume systematic issues across multiple files - use grep/rg to find actual errors, fix ONLY confirmed bugs
- documentation: always read README.md before making assumptions about config/log locations
- documentation: README.md has priority for user-facing features - docs/ is for technical reference only
- documentation: update docs/README when adding/changing features
- documentation: keep docs minimal and tool-specific - don't explain technologies users already know
- documentation: main docs in `docs/config.md`, README for overview only
- documentation: when adding tool support, update core docs, pi-extension/README.md, and docs/hook-systems/
- documentation: VSCode extension requires manual regex updates in `editors/vscode/syntaxes/dippy.tmLanguage.json` when adding new directives
- backlog: use filters with `backlog task list` (e.g., `-p high -s todo`), never bare listing
- development: prefer simple KISS solutions over clever features - don't add overhead on every operation when once-per-day is sufficient
- development: prefer native Dippy tool matchers (match_read, match_edit) over synthetic bash command simulation
- development: prefer surgical edit over full write for configuration files to prevent accidental regressions (e.g., reverting user's manual changes)
- development: ensure pi_wrapper.py remains synchronized with dippy.py for logging, agent identification, and tool handling
- development: if changing >5 files, you're probably wrong - verify with actual error data before making mass changes
- development: don't simplify user's exact requirements without asking - implement literally
- development: ask before acting when user requests explanation - don't make edits when user says "vysvetli mi" or "nic dalsiho nedelej"
- development: use code review subagent for significant changes (>100 lines or new features)
- development: avoid duplicate list maintenance - discover from code, never maintain separate constant lists (e.g., BUILTIN_COMMANDS)
- development: avoid imports inside functions - ugly pattern that violates code cleanliness
- background tasks: daily cleanup/rotation tasks should run once per relevant period, not on every startup/write
- log rotation: use yesterday's date for rotated files (active file always has current name)
- pi-mono: plan mode is a pi-mono feature, not Dippy (exit via `/plan` or Ctrl+Alt+P)

## Dippy Configuration

- rules: follow "last match wins" behavior
- allowlists: SIMPLE_SAFE commands cannot be overridden by `set` directive, but CAN be overridden by config rules (rules have higher priority)
- notifier: `notifier-command` and `notifier-include` directives for sidekick context injection (v0.2.5+)
- pattern matching: happens after quote stripping by Parable parser
- directives: only `ask` and `deny` support messages, `allow` does not
- tool directives: `allow-edit` is the universal directive for all modification tools (Write, Edit, MultiEdit). Also supports `allow-read`, `ask-read`, `deny-read` for file access.

## Technical Patterns

- context_flags: when delegating analysis, preserve outer context by combining with inner context_flags
- context_flags: when creating Decision objects, explicitly pass context_flags parameter (don't rely on defaults)
- context_flags: when handlers delegate via Classification, preserve remote flag by returning Classification(..., remote=ctx.remote)
- remote mode: container/ssh commands should NOT expand paths against host cwd - use literal paths when remote=True
- type changes: when changing field types in dataclasses, update ALL consumers systematically (handlers, analyzer, tests)
- optional sets: use truthiness (`if value`) not identity (`if value is not None`) for optional frozensets - empty set is falsy but not None
- banned modules: avoid `shlex`; use `dippy.core.parser.tokenize` for bash-compatible tokenization
- suggestion field: use `" ".join(tokens)` for output formatting (not `shlex.join` — shlex is banned; space-join is sufficient for allow-rule suggestions where fnmatch handles patterns)
- pi-mono: use `deliverAs: "followUp"` for agent-initiated turns to prevent collisions with user input
- notifier: `agent_end` hook enables long-polling idle behaviors with `--idle` flag

## Communication

- communication: ALWAYS explain the intended change and the reasoning behind it FIRST. Wait for user approval before modifying any files.
- communication: Czech phrases signal STOP - "kurva" (fundamental error requiring correction), "musi byt" (non-negotiable requirement), "nemam cas na pokusy" (no experiments allowed, research first)

## Git

- remotes: `original`=upstream (ldayton/Dippy), `origin`=fork (orgoj/Dippy), `tony`=contributor (tony-nekola-silk)
- attribution: when documenting fork features, run `git remote -v` first, use `git log --all --source` for commit origins
- operations: always check `git status` first to detect interrupted states
- merges: use worktrees for large upstream merges (see skill: safe-upstream-merge)
- merges: use fast-forward (`--ff-only`) for branch synchronization
- workflow: uses git worktrees for isolated development (e.g., `orgoj-dev` worktree for development)
- commits: run `just test` BEFORE committing - NON-NEGOTIABLE, never commit failing tests or skip this step
- commits: use conventional format (feat:, fix:, chore:, docs:) with Co-Authored-By trailer
- commits: push immediately after commit when user requests

## Merge Process

- upstream merges: run tests IMMEDIATELY after merge to baseline - don't assume systematic issues
- verification: create merge report with git hash/date in filename (docs/orgoj/merge_report_YYYY-MM-DD.md)
- verification: verify README claims against actual code using git diff and grep - don't claim features without evidence
- context: use git log --all --source to verify commit origins before attributing features

## Upstream

Read [README.md](README.md) for an overview.

Configuration docs: [../Dippy.wiki/Configuration.md](../Dippy.wiki/Configuration.md)

<!-- bv-agent-instructions-v1 -->

---

## Beads Workflow Integration

This project uses [beads_viewer](https://github.com/Dicklesworthstone/beads_viewer) for issue tracking. Issues are stored in `.beads/` and tracked in git.

### Essential Commands

```bash
# View issues (launches TUI - avoid in automated sessions)
bv

# CLI commands for agents (use these instead)
br ready              # Show issues ready to work (no blockers)
br list --status=open # All open issues
br show <id>          # Full issue details with dependencies
br create --title="..." --type=task --priority=2
br update <id> --status=in_progress
br close <id> --reason="Completed"
br close <id1> <id2>  # Close multiple issues at once
br sync               # Commit and push changes
```

### Workflow Pattern

1. **Start**: Run `br ready` to find actionable work
2. **Claim**: Use `br update <id> --status=in_progress`
3. **Work**: Implement the task
4. **Complete**: Use `br close <id>`
5. **Sync**: Always run `br sync` at session end

### Key Concepts

- **Dependencies**: Issues can block other issues. `br ready` shows only unblocked work.
- **Priority**: P0=critical, P1=high, P2=medium, P3=low, P4=backlog (use numbers, not words)
- **Types**: task, bug, feature, epic, question, docs
- **Blocking**: `br dep add <issue> <depends-on>` to add dependencies

### Session Protocol

**Before ending any session, run this checklist:**

```bash
git status              # Check what changed
git add <files>         # Stage code changes
br sync                 # Commit beads changes
git commit -m "..."     # Commit code
br sync                 # Commit any new beads changes
git push                # Push to remote
```

### Best Practices

- Check `br ready` at session start to find available work
- Update status as you work (in_progress → closed)
- Create new issues with `br create` when you discover tasks
- Use descriptive titles and set appropriate priority/type
- Always `br sync` before ending session

<!-- end-bv-agent-instructions -->
