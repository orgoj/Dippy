# Subagent Hook Issues in Claude Code

**Last Updated:** January 23, 2026

## Summary

Claude Code subagents (spawned via the Task tool) have critical bugs that prevent PreToolUse hooks from controlling tool execution. These issues are confirmed, reproducible, and **closed as "not planned"** by Anthropic, meaning they will not be fixed.

---

## The Problem

### Issue 1: Subagents Bypass Tool Permissions

**GitHub Issue:** [#4740 - Sub-agents use tools without permission](https://github.com/anthropics/claude-code/issues/4740)

**Status:** Closed as NOT PLANNED (January 3, 2026)

**Problem:** Subagents can use tools even when:
- No tool permissions are granted in subagent config (`tools:` empty)
- Project-level permissions explicitly deny tools
- PreToolUse hooks return `deny` decisions

**Example:**
```yaml
# Subagent config with no tools permitted
---
name: prd
description: Transform information into PRD
tools:      # ← Empty - no tools should be allowed
color: yellow
---
```

**Result:** Subagent executes 7 tool calls anyway:
```
⏺ prd(Create PRD from ticket)
 ⎿  Done (7 tool uses · 23.8k tokens · 1m 55.9s)
```

**Related Issues:** #4799, #4801, #5406, #5438, #5448, #6413 (multiple duplicates)

---

### Issue 2: PreToolUse Hooks Ignored

**GitHub Issue:** [#4669 - permissionDecision: "deny" in PreToolUse hooks is ignored](https://github.com/anthropics/claude-code/issues/4669)

**Status:** Closed as NOT PLANNED (January 12, 2026)

**Problem:** PreToolUse hooks execute correctly and return proper JSON, but Claude Code **ignores the decision** and executes tools anyway.

**What Doesn't Work:**
- ✗ `"permissionDecision": "deny"` - Tool executes anyway
- ✗ `"permissionDecision": "ask"` - No user prompt shown
- ✗ `"continue": false` - Tool execution continues
- ✗ Non-zero exit codes - Tool proceeds regardless

**What Does Work:**
- ✓ Hook scripts execute
- ✓ JSON output is formatted correctly
- ✓ Hook can analyze tool input
- ✓ Hook can log decisions to audit files

**Affected Versions:** 1.0.62 through 1.0.69+ (all recent versions)

**Related Issues:** [#4362 - PreToolUse hooks cannot block tool execution](https://github.com/anthropics/claude-code/issues/4362)

---

## Impact on Dippy

### What Works

**Main Session:**
1. Dippy hook executes
2. Returns `"permissionDecision": "allow"` for safe commands
3. Claude respects the decision ✓
4. Command executes without prompt ✓

**Audit Log:**
```json
{"decision": "allow", "cmd": "cargo clippy", "rule": "cargo *", "ts": "..."}
```

### What's Broken

**Subagent Session:**
1. Dippy hook executes
2. Returns `"permissionDecision": "allow"` for safe commands
3. **Claude IGNORES the decision** ✗
4. **User is prompted anyway** ✗

**Audit Log (identical output):**
```json
{"decision": "allow", "cmd": "cargo clippy", "rule": "cargo *", "ts": "..."}
```

**Result:** Dippy works perfectly, but Claude Code subagents ignore the response.

---

## Why This Happens

### Context Isolation

**From Issue #5812:** [Allow Hooks to Bridge Context Between Sub-Agents](https://github.com/anthropics/claude-code/issues/5812)

Subagents run in isolated contexts with:
- Separate session IDs (but same `session_id` in hooks, making them indistinguishable)
- No access to parent session state
- No sharing of learned rules or permissions
- Independent permission evaluation

**Status:** Closed as NOT PLANNED

### Missing Parent Context

**From Issue #19448:** [Include parent_session_id in hook payloads](https://github.com/anthropics/claude-code/issues/19448)

Hook payloads don't include:
- `parent_session_id` - Can't identify parent-child relationships
- `agent_id` - Can't distinguish between subagents
- `parent_agent_id` - Can't track agent hierarchy

**Status:** Closed as duplicate of #16424

---

## Workarounds

### 1. Explicit Config Rules (Recommended)

Instead of relying on hooks, add explicit rules to `~/.dippy/config`:

```
# Cargo tools - safe development commands
allow cargo clippy *
allow cargo test *
allow cargo build *
allow cargo check *
allow cargo fmt *

# Add rules for any commands frequently used in subagents
```

**Why this helps:** Config rules are evaluated before hook decisions, providing a fallback when hooks are ignored.

**Limitation:** This only reduces prompts; subagents may still prompt for commands not explicitly allowed.

### 2. Project-Level Config

Add `.dippy` in project root with project-specific rules:

```
# Project: my-rust-app
allow cargo *
allow just *
```

**Limitation:** According to #4740, even project-level permissions are bypassed by subagents.

### 3. Session-Level Bypass (Not Recommended)

Use `mode: "bypassPermissions"` when spawning subagents:

```json
{
  "Task": {
    "mode": "bypassPermissions"
  }
}
```

**⚠️ WARNING:** This completely disables all permission checks, including Dippy. Only use in trusted, sandboxed environments.

### 4. Avoid Subagents for Tool-Heavy Work

Structure workflows to minimize subagent tool usage:
- Main session handles tool calls
- Subagents focus on planning, analysis, writing
- Parent agent executes subagent recommendations

**Trade-off:** Reduces parallelism and autonomous capabilities.

---

## Technical Details

### How Dippy Hook Executes

1. **PreToolUse event fires**
   - Claude Code calls dippy-hook with JSON input
   - Input contains: `tool_name`, `tool_input`, `cwd`, `session_id`

2. **Dippy analyzes command**
   - Parses bash with Parable
   - Matches against config rules
   - Makes decision: allow/ask/deny

3. **Dippy returns JSON**
   ```json
   {
     "hookSpecificOutput": {
       "hookEventName": "PreToolUse",
       "permissionDecision": "allow",
       "permissionDecisionReason": "🐤 cargo clippy (cargo *)"
     }
   }
   ```

4. **Claude Code behavior:**
   - **Main session:** Respects decision ✓
   - **Subagent:** Ignores decision, prompts user ✗

### Why Subagents Ignore Hooks

According to the issues, Claude Code has architectural problems:

1. **Permission inheritance:** Subagents don't inherit parent permissions
2. **Hook execution:** Hooks run but responses are not processed
3. **Security model:** Subagent permission system is separate and broken
4. **Design decision:** Anthropic closed issues as "not planned" - intentional?

---

## Verification

To verify this behavior in your environment:

1. Create a config rule: `allow cargo clippy *`
2. Run `cargo clippy` in main session → no prompt (works)
3. Launch subagent with Task tool
4. Subagent runs `cargo clippy` → prompted anyway (broken)
5. Check `~/.dippy/audit.log`:
   ```json
   {"decision": "allow", "cmd": "cargo clippy", ...}  // Main session
   {"decision": "allow", "cmd": "cargo clippy", ...}  // Subagent (identical!)
   ```

Both entries are identical, proving Dippy works correctly but subagents ignore the hook response.

---

## Timeline

| Date | Event |
|------|-------|
| 2025-07 | Issue #4740 filed: Subagents bypass tool permissions |
| 2025-09 | Issue #4669 filed: PreToolUse deny ignored |
| 2026-01-03 | Issue #4740 closed as NOT PLANNED |
| 2026-01-12 | Issue #4669 closed as NOT PLANNED |
| 2026-01-20 | Issue #19448 filed: Missing parent_session_id |
| 2026-01-23 | Issue #19448 closed as duplicate of #16424 |

---

## Recommendations

### For Dippy Users

1. **Add explicit config rules** for commands frequently used in subagents
2. **Don't rely on hooks alone** - treat them as main-session only
3. **Monitor audit log** - confirm Dippy is working (decisions logged)
4. **Report issues** to Anthropic (even though they're closed as "not planned")

### For Anthropic

This is a **critical security vulnerability**:
- Hooks are advertised as permission control mechanisms
- Documentation doesn't mention subagent limitations
- Multiple issues marked `area:security` but closed as "not planned"
- Workarounds are inadequate or dangerous (bypassPermissions)

**Request:** Reopen #4740 and #4669, prioritize fix, or update documentation to clearly state hooks don't work in subagents.

---

## References

- [Issue #4740 - Sub-agents use tools without permission](https://github.com/anthropics/claude-code/issues/4740)
- [Issue #4669 - permissionDecision: "deny" ignored](https://github.com/anthropics/claude-code/issues/4669)
- [Issue #4362 - PreToolUse hooks cannot block execution](https://github.com/anthropics/claude-code/issues/4362)
- [Issue #5812 - Allow Hooks to Bridge Context](https://github.com/anthropics/claude-code/issues/5812)
- [Issue #19448 - Include parent_session_id](https://github.com/anthropics/claude-code/issues/19448)
- [Issue #16424 - Expose Agent Context in Hook Event Payloads](https://github.com/anthropics/claude-code/issues/16424) (tracking issue)

---

**Bottom Line:** Dippy works correctly. Claude Code subagents are broken. Anthropic won't fix it. Use explicit config rules as workaround.
