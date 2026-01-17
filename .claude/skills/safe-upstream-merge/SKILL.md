---
name: safe-upstream-merge
description: Safely merge changes from upstream remote into active repository using git worktrees. Use when merging would disrupt the working repo that must remain functional at all times.
---

# Safe Upstream Merge with Worktrees

Use this skill when:
- Merging upstream changes that might break tests or introduce conflicts
- The repository is actively used and must remain in working state
- Large changes that would disrupt normal work if done in main worktree

## Pre-Merge Checklist

1. **Verify clean state**
   ```bash
   git status  # Must be clean, no interrupted operations
   ```

2. **Identify remotes correctly**
   - `original` = upstream (ldayton/Dippy)
   - `origin` = your fork
   - READ user instructions carefully - "origin" vs "original" matters!

3. **Fetch latest from upstream**
   ```bash
   git fetch original
   ```

## Worktree Merge Process

### Step 1: Create isolated worktree

```bash
git worktree add .worktrees/merge-upstream original/main
cd .worktrees/merge-upstream
```

Or for specific branch:
```bash
git worktree add .worktrees/merge-feature -b merge-feature origin/your-branch
cd .worktrees/merge-feature
git merge original/branch-to-merge
```

### Step 2: Resolve conflicts carefully

When conflicts occur:
1. **Identify what each side contributes** - don't blindly accept one side
2. **Preserve local features** - your customizations matter
3. **Preserve upstream features** - that's why you're merging
4. **Combine code paths** when both add different functionality

Common conflict patterns in this repo:
- CLAUDE.md - keep local (upstream may delete)
- README.md - combine documentation sections
- analyzer.py - preserve `default=pass` logic + upstream additions
- config.py - combine parsing for different rule types
- tests - combine test classes from both sides

### Step 3: Run tests BEFORE committing

```bash
just test  # or: uv run pytest
```

**CRITICAL**: Never commit merge until ALL tests pass!

If tests fail:
1. Check if upstream introduced bugs
2. Fix issues in worktree before commit
3. Document fixes in commit message

### Step 4: Complete merge in worktree

```bash
git add -A
git commit -m "Merge original/branch: description of what's added

- Preserved local features: list them
- Added upstream features: list them
- Fixed conflicts in: list files

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

### Step 5: Bring changes to main worktree

```bash
# Return to main worktree
cd /path/to/main/repo

# Merge from the worktree branch (fast-forward or merge)
git merge merge-upstream

# Verify tests still pass in main worktree
just test
```

### Step 6: Cleanup worktree

```bash
git worktree remove .worktrees/merge-upstream
```

## Recovery Procedures

### Interrupted rebase detected

```bash
git status  # Shows "rebase in progress"
git rebase --abort  # Recover cleanly
```

### Worktree merge went wrong

The main worktree is untouched! Simply:
```bash
git worktree remove --force .worktrees/merge-upstream
```

### Tests fail after merge

Fix in worktree, don't commit broken state:
```bash
# Stay in worktree, fix issues
# Re-run tests
just test
# Only then commit
```

## Anti-Patterns

- **NEVER** merge directly in main worktree for large changes
- **NEVER** commit merge before tests pass
- **NEVER** use `git reset --hard` during active rebase
- **NEVER** assume remote names - verify with `git remote -v`
- **NEVER** rush through conflict resolution

## Project-Specific Notes

This repo (dippy) has:
- Remote `original` = ldayton/Dippy (upstream)
- Remote `origin` = orgoj/Dippy (fork)
- Local features to preserve: `allow-opt/ask-opt/deny-opt`, `set default pass`
- Test command: `just test` or `uv run pytest`
- SIMPLE_SAFE vs CLI handlers: commands with unsafe flag variants need handlers, not SIMPLE_SAFE
