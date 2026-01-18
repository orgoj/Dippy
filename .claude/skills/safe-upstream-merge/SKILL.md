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
# Use subshell for all worktree operations:
(cd .worktrees/merge-upstream && git log -1 --oneline)
```

Or for specific branch:
```bash
git worktree add .worktrees/merge-feature -b merge-feature origin/your-branch
(cd .worktrees/merge-feature && git merge original/branch-to-merge)
```

**CRITICAL**: Always use subshells `(cd DIR && ...)` - never bare `cd` commands!

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
(cd .worktrees/merge-upstream && just test)  # or: uv run pytest
```

**CRITICAL**: Never commit merge until ALL tests pass!

If tests fail:
1. Check if upstream introduced bugs
2. Fix issues in worktree before commit
3. Document fixes in commit message

### Step 4: Complete merge in worktree

```bash
(cd .worktrees/merge-upstream && git add -A && git commit -m "Merge original/branch: description of what's added

- Preserved local features: list them
- Added upstream features: list them
- Fixed conflicts in: list files

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>")
```

### Step 5: Bring changes to main worktree

```bash
# You should already be in main worktree (project root)
# Merge from the worktree branch (fast-forward or merge)
git merge merge-upstream

# Verify tests still pass in main worktree
just test
```

### Step 6: Cleanup worktree

```bash
git worktree remove .worktrees/merge-upstream
```

## Cherry-Pick from External Forks

When cherry-picking commits from another user's fork (not upstream):

### Step 1: Add remote and fetch

```bash
git remote add <name> git@github.com:user/repo.git
git fetch <name>
```

### Step 2: Create worktree for cherry-pick

```bash
git worktree add .worktrees/cherry-pick-<feature> HEAD
(cd .worktrees/cherry-pick-<feature> && git status)
```

### Step 3: Cherry-pick with strategy

For new feature additions where both sides should coexist:
```bash
# Use theirs strategy to accept incoming changes
(cd .worktrees/cherry-pick-<feature> && git cherry-pick -X theirs <commit-hash>)
```

**CRITICAL**: After `-X theirs`, manually restore any local features that were removed:
1. Check diff to see what's missing
2. Manually add back local features (fields, functions, directives)
3. Test to ensure both features work together

### Step 4: Commit merge of features

If conflicts occurred and you manually merged features:
```bash
(cd .worktrees/cherry-pick-<feature> && git add -A && git commit -m "Merge external feature with local features

- Preserved local features: list them
- Added external features: list them
- Both coexist: explanation

Co-Authored-By: <Original Author> <email>
Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>")
```

### Step 5: Cherry-pick additional commits

For second and subsequent commits:
```bash
# Must commit previous changes first!
(cd .worktrees/cherry-pick-<feature> && git cherry-pick -X theirs <next-commit-hash>)
```

### Step 6: Run tests in worktree

```bash
(cd .worktrees/cherry-pick-<feature> && just test)
```

**CRITICAL**: All tests must pass before proceeding!

### Step 7: Merge to main worktree

```bash
# Get worktree HEAD commit
(cd .worktrees/cherry-pick-<feature> && git log -1 --oneline)

# Merge that commit to main
git merge <worktree-commit-hash>  # Fast-forward works best
```

### Step 8: Verify in main worktree

```bash
just test  # Must pass in main worktree too
git status  # Verify clean state
```

### Step 9: Cleanup

```bash
git worktree remove .worktrees/cherry-pick-<feature>
```

**Key differences from upstream merge:**
- Use `-X theirs` to accept new functionality
- Must manually preserve local customizations
- Use commit hash for merge, not worktree path
- Document both external author and your merge work

### Tracking Cherry-Picks

**IMPORTANT**: Always reference original commits to avoid duplication:

```bash
# Check what's already cherry-picked
git log --oneline --grep="Cherry-pick from"

# When cherry-picking, document original commit:
git cherry-pick -X theirs <commit-hash>
# Then amend commit message to include:
# Cherry-pick from: <remote>/<commit-hash>
# Original: <author> "<original-message>"
```

**Example commit message format:**
```
feat: add WebSearch auto-approval support

Cherry-pick from: tony/9ce9cfe
Original: Tony Nekola "Add WebSearch auto-approval support"
- Merged with local edit_rules feature
- Both coexist without conflicts

Co-Authored-By: Tony Nekola <tony.nekola@silk.us>
Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>
```

**Check if already merged:**
```bash
# Compare commit content (same changes = different hashes)
git log --all --source --oneline --grep="<commit-title>"
git show <original-hash> | grep -q "<unique-code>" && git log -S"<unique-code>" --oneline
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
# Fix issues in worktree using subshells
(cd .worktrees/merge-upstream && just test)
# Only then commit
(cd .worktrees/merge-upstream && git add -A && git commit --amend)
```

## Anti-Patterns

- **NEVER** merge directly in main worktree for large changes
- **NEVER** commit merge before tests pass
- **NEVER** use `git reset --hard` during active rebase
- **NEVER** assume remote names - verify with `git remote -v`
- **NEVER** rush through conflict resolution
- **NEVER** use bare `cd` commands - always use subshells `(cd DIR && ...)`
- **NEVER** merge worktree path directly - use commit hash: `git merge <hash>` not `git merge .worktrees/name`
- **NEVER** forget to manually restore local features after `-X theirs` cherry-pick

## Project-Specific Notes

This repo (dippy) has:
- Remote `original` = ldayton/Dippy (upstream)
- Remote `origin` = orgoj/Dippy (fork)
- Local features to preserve: `allow-opt/ask-opt/deny-opt`, `edit_rules` (Write/Edit/MultiEdit), `set default pass`
- Test command: `just test` or `uv run pytest`
- SIMPLE_SAFE vs CLI handlers: commands with unsafe flag variants need handlers, not SIMPLE_SAFE

When merging external WebSearch or edit rule features:
- Both `web_rules` and `edit_rules` use similar patterns - preserve both
- Config structure: add new rule fields to dataclass, merge/tag/parse functions
- Add new directives in parse_config after existing directive blocks
- Both check functions and handlers follow same pattern as MCP tools
