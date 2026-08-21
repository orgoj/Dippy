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

// Resolve symlinks to get actual file location
const extensionPath = realpathSync(fileURLToPath(import.meta.url));
const __dirname = dirname(extensionPath);

interface DippyInput {
  type: "bash" | "read" | "edit";
  command?: string;
  path?: string;
  cwd: string;
  agent?: string;
}

interface DippyDecision {
  action: "allow" | "ask" | "deny" | "pass";
  reason: string;
  context_flags?: string[];
  error?: boolean;
}

interface DippyConfig {
  enabled?: boolean;
  askBehavior?: "block" | "ask" | "allow";
}

/**
 * Validate a tool call through dippy's Python wrapper.
 */
async function validateDippy(
  wrapperScript: string,
  input: DippyInput,
): Promise<DippyDecision> {
  return new Promise((resolve, reject) => {
    const python = spawn("python3", [wrapperScript], {
      cwd: input.cwd,
      env: { ...process.env, PYTHONUNBUFFERED: "1" },
    });

    let stdout = "";
    let stderr = "";

    python.stdout.on("data", (data) => {
      stdout += data.toString();
    });

    python.stderr.on("data", (data) => {
      stderr += data.toString();
    });

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

/**
 * Map moltbot tool names and params to dippy input format.
 */
function mapToolToDippyInput(
  toolName: string,
  params: Record<string, unknown>,
  cwd: string,
): DippyInput | null {
  const baseInput = { cwd, agent: "moltbot" as const };
  switch (toolName) {
    case "exec":
    case "bash": {
      const command = params.command as string;
      if (command) return { ...baseInput, type: "bash", command };
      break;
    }

    case "read": {
      const readPath = (params.path as string) ?? (params.file_path as string);
      if (readPath) return { ...baseInput, type: "read", path: readPath };
      break;
    }

    case "write":
    case "edit": {
      const editPath = (params.path as string) ?? (params.file_path as string);
      if (editPath) return { ...baseInput, type: "edit", path: editPath };
      break;
    }
  }
  return null;
}

const dippyPlugin = {
  id: "dippy",
  name: "Dippy Command Validator",
  description: "Validates bash commands and file operations via dippy",
  version: "1.0.0",

  register(api: MoltbotPluginApi) {
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

    const askBehavior = config?.askBehavior ?? "block";

    api.logger.info(`Dippy validation enabled (askBehavior: ${askBehavior})`);

    api.on(
      "before_tool_call",
      async (
        event: PluginHookBeforeToolCallEvent,
        ctx: PluginHookToolContext,
      ): Promise<PluginHookBeforeToolCallResult | undefined> => {
        const cwd = process.cwd();
        const input = mapToolToDippyInput(event.toolName, event.params, cwd);

        if (!input) return undefined;

        try {
          const decision = await validateDippy(wrapperScript, input);

          api.logger.debug?.(
            `[dippy] ${decision.action} [${event.toolName}]: ${decision.reason}`,
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
                case "allow":
                  return undefined;
                case "block":
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
      },
      { priority: 100 },
    );
  },
};

export default dippyPlugin;
