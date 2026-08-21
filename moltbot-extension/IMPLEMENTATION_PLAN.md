# Dippy Moltbot Extension - Implementation Plan

## Key Takeaway

**Moltbot needs no modifications.** The solution is purely in dippy repo - we create `moltbot-extension/` similar to existing `pi-extension/`.

## Why This Works

Moltbot uses **the same tools from `@mariozechner/pi-coding-agent`** as pi-mono:
- `read` - file reading
- `write` - file writing
- `edit` - file editing
- `exec` - bash commands (moltbot-specific name for `bash`)

Moltbot hook API (`before_tool_call`) is nearly identical to pi-mono:

| Aspect | pi-mono | moltbot |
|--------|---------|---------|
| Hook | `pi.on("tool_call", ...)` | `api.on("before_tool_call", ...)` |
| Tool name | `event.toolName` | `event.toolName` |
| Parameters | `event.input` | `event.params` |
| Block | `{ block: true, reason }` | `{ block: true, blockReason }` |
| UI confirm | `ctx.ui.confirm()` | TBD |

## Tool Mapping

| Moltbot Tool | Dippy Type | Dippy Config Rules |
|--------------|------------|-------------------|
| `exec` | `bash` | `allow`, `ask`, `deny` |
| `read` | `read` | `allow-read`, `ask-read`, `deny-read` |
| `write` | `edit` | `allow-edit`, `ask-edit`, `deny-edit` |
| `edit` | `edit` | `allow-edit`, `ask-edit`, `deny-edit` |

---

## Implementation in dippy Repo

### File Structure (in dippy)

```
dippy/
├── pi-extension/              # Existing pi-mono extension
│   ├── dippy-extension.ts
│   └── README.md
├── moltbot-extension/         # NEW - moltbot extension
│   ├── dippy-extension.ts     # Main extension file
│   ├── README.md              # Installation guide
│   └── IMPLEMENTATION_PLAN.md # This file
└── src/dippy/
    └── pi_wrapper.py          # Existing - shared Python wrapper
```

### moltbot-extension/dippy-extension.ts

Adaptation of existing pi-extension for moltbot API:

```typescript
/**
 * Dippy Extension for Moltbot
 *
 * Validates bash commands and file operations via dippy's Python analyzer.
 *
 * Installation:
 *   1. Symlink to moltbot workspace extensions:
 *      ln -s /path/to/dippy/moltbot-extension \
 *            ~/.moltbot/extensions/dippy
 *
 *   2. Or reference in moltbot config:
 *      plugins:
 *        - path: /path/to/dippy/moltbot-extension
 *          config:
 *            enabled: true
 *            askBehavior: block
 */
import type {
  MoltbotPluginApi,
  PluginHookBeforeToolCallEvent,
  PluginHookToolContext,
  PluginHookBeforeToolCallResult,
} from "moltbot/plugin-sdk";
import { spawn } from "child_process";
import { dirname, join } from "path";
import { fileURLToPath } from "url";
import { existsSync, realpathSync } from "fs";

const extensionPath = realpathSync(fileURLToPath(import.meta.url));
const __dirname = dirname(extensionPath);

interface DippyInput {
  type: 'bash' | 'read' | 'edit';
  command?: string;
  path?: string;
  cwd: string;
}

interface DippyDecision {
  action: 'allow' | 'ask' | 'deny' | 'pass';
  reason: string;
  context_flags?: string[];
  error?: boolean;
}

interface DippyConfig {
  enabled?: boolean;
  askBehavior?: 'block' | 'ask' | 'allow';
}

async function validateDippy(
  wrapperScript: string,
  input: DippyInput
): Promise<DippyDecision> {
  return new Promise((resolve, reject) => {
    const python = spawn("python3", [wrapperScript], {
      cwd: input.cwd,
      env: { ...process.env, PYTHONUNBUFFERED: "1" },
    });

    let stdout = "";
    let stderr = "";

    python.stdout.on("data", (data) => { stdout += data.toString(); });
    python.stderr.on("data", (data) => { stderr += data.toString(); });

    python.on("close", (code) => {
      if (code !== 0) {
        reject(new Error(`dippy failed (exit ${code}): ${stderr}`));
        return;
      }
      try {
        resolve(JSON.parse(stdout));
      } catch {
        reject(new Error(`Failed to parse dippy output: ${stdout}`));
      }
    });

    python.on("error", (error) => {
      reject(new Error(`Failed to spawn Python: ${error.message}`));
    });

    python.stdin.write(JSON.stringify(input));
    python.stdin.end();
  });
}

function mapToolToDippyInput(
  toolName: string,
  params: Record<string, unknown>,
  cwd: string
): DippyInput | null {
  switch (toolName) {
    case "exec":
    case "bash":
      const command = params.command as string;
      if (command) return { type: 'bash', command, cwd };
      break;

    case "read":
      const readPath = params.path as string ?? params.file_path as string;
      if (readPath) return { type: 'read', path: readPath, cwd };
      break;

    case "write":
    case "edit":
      const editPath = params.path as string ?? params.file_path as string;
      if (editPath) return { type: 'edit', path: editPath, cwd };
      break;
  }
  return null;
}

export default function register(api: MoltbotPluginApi) {
  const wrapperScript = join(__dirname, "../src/dippy/pi_wrapper.py");
  const config = api.pluginConfig as DippyConfig | undefined;

  if (!existsSync(wrapperScript)) {
    api.logger.error(`Dippy wrapper not found: ${wrapperScript}`);
    return;
  }

  if (config?.enabled === false) {
    api.logger.info("Dippy validation disabled");
    return;
  }

  const askBehavior = config?.askBehavior ?? 'block';

  api.logger.info(`Dippy validation enabled (askBehavior: ${askBehavior})`);

  api.on("before_tool_call", async (
    event: PluginHookBeforeToolCallEvent,
    ctx: PluginHookToolContext
  ): Promise<PluginHookBeforeToolCallResult | undefined> => {

    const cwd = process.cwd(); // TODO: get from ctx when available
    const input = mapToolToDippyInput(event.toolName, event.params, cwd);

    if (!input) return undefined;

    try {
      const decision = await validateDippy(wrapperScript, input);

      api.logger.debug?.(
        `[dippy] ${decision.action} [${event.toolName}]: ${decision.reason}`
      );

      switch (decision.action) {
        case "allow":
        case "pass":
          return undefined;

        case "deny":
          return {
            block: true,
            blockReason: decision.reason || "Blocked by Dippy",
          };

        case "ask":
          // Handle based on config
          switch (askBehavior) {
            case 'allow':
              return undefined;
            case 'block':
            default:
              return {
                block: true,
                blockReason: `Dippy requires approval: ${decision.reason}`,
              };
            // TODO: 'ask' mode when moltbot has UI confirmation API
          }
      }
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      api.logger.error(`Dippy validation error: ${msg}`);
      return {
        block: true,
        blockReason: `Dippy validation error: ${msg}`,
      };
    }
  }, { priority: 100 });
}
```

### Changes to Existing Code

**No changes in moltbot repo!**

In dippy only:
1. Create `moltbot-extension/` directory
2. Create `moltbot-extension/dippy-extension.ts`
3. Create `moltbot-extension/README.md`

### Installation and Usage

```bash
# In dippy repo
cd /path/to/dippy

# Symlink to moltbot
ln -s $(pwd)/moltbot-extension ~/.moltbot/extensions/dippy

# Test
moltbot agent --message "run ls -la"     # → allow
moltbot agent --message "run rm -rf /"   # → deny
```

### Verification

1. **Bash commands**: `allow git status`, `deny rm -rf **`
2. **File reads**: `allow-read ~/projects/**`, `deny-read ~/.ssh/**`
3. **File edits**: `allow-edit ~/projects/**`, `deny-edit /etc/**`

---

## Implementation Steps (after approval)

1. **Create `moltbot-extension/` in dippy repo**
   - `dippy-extension.ts` - main extension code (see above)
   - `README.md` - documentation (see above)

2. **Test installation**
   ```bash
   cd /path/to/dippy
   mkdir -p moltbot-extension
   # create files

   # symlink
   ln -s $(pwd)/moltbot-extension ~/.moltbot/extensions/dippy
   ```

3. **Verification**
   ```bash
   moltbot agent --message "run ls"          # allow
   moltbot agent --message "run rm -rf /"    # deny
   moltbot agent --message "read a file"     # test read rules
   ```

## Files to Create

| File | Repo | Content |
|------|------|---------|
| `moltbot-extension/dippy-extension.ts` | dippy | Extension code (see plan) |
| `moltbot-extension/README.md` | dippy | Installation guide |

**Moltbot repo**: No changes

## Decisions

1. **Location**: Extension lives in **dippy repo** (`moltbot-extension/`)
   - User symlinks or references in moltbot config
   - Shares `pi_wrapper.py` with pi-extension

2. **"Ask" handling**: **Configurable** with three options:
   - `askBehavior: "block"` - block (default, most secure)
   - `askBehavior: "ask"` - prompt user (when UI available)
   - `askBehavior: "allow"` - allow without prompt (fast, risky)

3. **Python dependency**: **OK** - Python 3 is commonly available
