# Dippy Moltbot Extension - Implementation Plan

## Klíčový závěr

**Moltbot nepotřebuje žádné úpravy.** Řešení je čistě v dippy-dev repo - vytvoříme `moltbot-extension/` obdobně jako existující `pi-extension/`.

## Proč to funguje

Moltbot používá **stejné nástroje z `@mariozechner/pi-coding-agent`** jako pi-mono:
- `read` - čtení souborů
- `write` - zápis souborů
- `edit` - editace souborů
- `exec` - bash příkazy (moltbot-specific název pro `bash`)

Moltbot hook API (`before_tool_call`) je téměř identické s pi-mono:

| Aspect | pi-mono | moltbot |
|--------|---------|---------|
| Hook | `pi.on("tool_call", ...)` | `api.on("before_tool_call", ...)` |
| Tool name | `event.toolName` | `event.toolName` |
| Parameters | `event.input` | `event.params` |
| Block | `{ block: true, reason }` | `{ block: true, blockReason }` |
| UI confirm | `ctx.ui.confirm()` | TBD |

## Mapování toolů

| Moltbot Tool | Dippy Type | Dippy Config Rules |
|--------------|------------|-------------------|
| `exec` | `bash` | `allow`, `ask`, `deny` |
| `read` | `read` | `allow-read`, `ask-read`, `deny-read` |
| `write` | `edit` | `allow-edit`, `ask-edit`, `deny-edit` |
| `edit` | `edit` | `allow-edit`, `ask-edit`, `deny-edit` |

---

## Implementace v dippy-dev repo

### Struktura souborů (v dippy-dev)

```
dippy-dev/
├── pi-extension/              # Existující pi-mono extension
│   ├── dippy-extension.ts
│   └── README.md
├── moltbot-extension/         # NOVÁ - moltbot extension
│   ├── dippy-extension.ts     # Hlavní extension soubor
│   ├── README.md              # Instalační návod
│   └── IMPLEMENTATION_PLAN.md # Tento soubor
└── src/dippy/
    └── pi_wrapper.py          # Existující - sdílený Python wrapper
```

### moltbot-extension/dippy-extension.ts

Adaptace existujícího pi-extension pro moltbot API:

```typescript
/**
 * Dippy Extension for Moltbot
 *
 * Validates bash commands and file operations via dippy's Python analyzer.
 *
 * Installation:
 *   1. Symlink to moltbot workspace extensions:
 *      ln -s /path/to/dippy-dev/moltbot-extension \
 *            ~/.moltbot/extensions/dippy
 *
 *   2. Or reference in moltbot config:
 *      plugins:
 *        - path: /path/to/dippy-dev/moltbot-extension
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

### Změny v existujícím kódu

**Žádné změny v moltbot repo!**

V dippy-dev pouze:
1. Vytvořit `moltbot-extension/` adresář
2. Vytvořit `moltbot-extension/dippy-extension.ts`
3. Vytvořit `moltbot-extension/README.md`

### Instalace a použití

```bash
# V dippy-dev repo
cd /home/michael/work/ai/CLAUDE/TOOLS/dippy-dev

# Symlink do moltbot
ln -s $(pwd)/moltbot-extension ~/.moltbot/extensions/dippy

# Test
moltbot agent --message "run ls -la"     # → allow
moltbot agent --message "run rm -rf /"   # → deny
```

### Verifikace

1. **Bash příkazy**: `allow git status`, `deny rm -rf **`
2. **File reads**: `allow-read ~/projects/**`, `deny-read ~/.ssh/**`
3. **File edits**: `allow-edit ~/projects/**`, `deny-edit /etc/**`

---

## Implementační kroky (po schválení)

1. **Vytvořit `moltbot-extension/` v dippy-dev repo**
   - `dippy-extension.ts` - hlavní extension kód (viz výše)
   - `README.md` - dokumentace (viz výše)

2. **Test instalace**
   ```bash
   cd /home/michael/work/ai/CLAUDE/TOOLS/dippy-dev
   mkdir -p moltbot-extension
   # vytvořit soubory

   # symlink
   ln -s $(pwd)/moltbot-extension ~/.moltbot/extensions/dippy
   ```

3. **Verifikace**
   ```bash
   moltbot agent --message "run ls"          # allow
   moltbot agent --message "run rm -rf /"    # deny
   moltbot agent --message "read a file"     # test read rules
   ```

## Soubory k vytvoření

| Soubor | Repo | Obsah |
|--------|------|-------|
| `moltbot-extension/dippy-extension.ts` | dippy-dev | Extension kód (viz plán) |
| `moltbot-extension/README.md` | dippy-dev | Instalační návod |

**Moltbot repo**: Žádné změny

## Rozhodnutí

1. **Umístění**: Extension žije v **dippy-dev repo** (`moltbot-extension/`)
   - Uživatel symlinkne nebo referencuje v moltbot config
   - Sdílí `pi_wrapper.py` s pi-extension

2. **"Ask" handling**: **Configurable** s třemi možnostmi:
   - `askBehavior: "block"` - blokovat (default, nejbezpečnější)
   - `askBehavior: "ask"` - promptnout user (až bude UI)
   - `askBehavior: "allow"` - povolit bez promptu (rychlé, riskantní)

3. **Python dependency**: **OK** - Python 3 je běžně dostupný
