# Plan: Complete Moltbot Extension for Dippy

## Status Summary

**Done:**
- ✅ `moltbot-extension/dippy-extension.ts` - main extension code (valid)
- ✅ `moltbot-extension/index.ts` - re-export
- ✅ `moltbot-extension/README.md` - user documentation
- ✅ `moltbot-extension/IMPLEMENTATION_PLAN.md` - design doc

**Issues found:**
1. `pi_wrapper.py` logs `agent="pi"` - should be `agent="moltbot"` for proper audit
2. No `configSchema` defined - moltbot plugins should define their config schema
3. No testing performed - moltbot not installed, cannot verify functionality

## Changes Required

### 1. Add Moltbot Support to `pi_wrapper.py`

**File:** `src/dippy/pi_wrapper.py`

Add support for `agent="moltbot"` parameter and log accordingly:

```python
# Add new CLI or env var to specify agent type
# Default: "pi" for backward compatibility
# When called from moltbot: pass agent="moltbot"
```

Change the `log_decision()` calls to use the passed agent parameter instead of hardcoded `"pi"`.

### 2. Add `configSchema` to Moltbot Extension

**File:** `moltbot-extension/dippy-extension.ts`

Add proper moltbot config schema (zod-based or plain object):

```typescript
const dippyPlugin = {
  id: "dippy",
  name: "Dippy Command Validator",
  // ...
  configSchema: {
    safeParse: (value: unknown) => {
      // Validate: enabled?, askBehavior?
    }
  }
};
```

### 3. Pass Agent ID from Extension

**File:** `moltbot-extension/dippy-extension.ts`

Pass `agent="moltbot"` to `pi_wrapper.py` via stdin input:

```typescript
interface DippyInput {
  type: "bash" | "read" | "edit";
  command?: string;
  path?: string;
  cwd: string;
  agent?: string;  // NEW
}
```

### 4. Update `pi_wrapper.py` Agent Handling

**File:** `src/dippy/pi_wrapper.py`

- Read `agent` from input JSON
- Default to `"pi"` if not provided (backward compat)
- Pass to `log_decision()` calls

### 5. Testing

Since moltbot is not installed, testing will be manual after installation:

1. Install moltbot: `cd /home/michael/work/ai/MOLTBOT/moltbot && pnpm install`
2. Symlink extension: `ln -s $(pwd)/moltbot-extension ~/.moltbot/extensions/dippy`
3. Test basic commands

## Files to Modify

| File | Change |
|------|--------|
| `src/dippy/pi_wrapper.py` | Add agent parameter support |
| `moltbot-extension/dippy-extension.ts` | Add agent to DippyInput, add configSchema |
| `moltbot-extension/README.md` | Update if needed (verify) |

## Verification Steps

1. **Test wrapper directly:**
   ```bash
   echo '{"type":"bash","command":"ls","cwd":".","agent":"moltbot"}' | \
     python3 src/dippy/pi_wrapper.py
   ```

2. **Check audit log:**
   ```bash
   tail -1 ~/.dippy/audit.log | jq .
   # Should show "agent": "moltbot"
   ```

3. **Test with moltbot** (after install):
   ```bash
   moltbot agent --message "run ls -la"    # should allow
   moltbot agent --message "run rm -rf /"  # should deny
   ```
