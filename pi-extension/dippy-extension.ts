/**
 * Dippy Extension for pi-mono
 *
 * Integrates dippy's bash command and file access approval system with pi-mono.
 * Intercepts bash, read, write, and edit tool calls and validates them through dippy.
 *
 * Installation:
 *   ln -s /path/to/dippy-dev/pi-extension/dippy-extension.ts \
 *         ~/.pi/agent/extensions/dippy-extension.ts
 */
import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import { spawn } from "child_process";
import { dirname, join } from "path";
import { fileURLToPath } from "url";
import { existsSync, realpathSync } from "fs";

// Resolve symlinks to get actual file location
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

/**
 * Validate a tool call through dippy's Python wrapper.
 */
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

    python.stdout.on("data", (data) => {
      stdout += data.toString();
    });

    python.stderr.on("data", (data) => {
      stderr += data.toString();
    });

    python.on("close", (code) => {
      if (code !== 0) {
        reject(new Error(`Python script failed (exit ${code}): ${stderr}`));
        return;
      }

      try {
        const result: DippyDecision = JSON.parse(stdout);
        resolve(result);
      } catch (error) {
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
 * Main extension function.
 */
export default function dippyExtension(pi: ExtensionAPI) {
  const wrapperScript = join(__dirname, "../src/dippy/pi_wrapper.py");

  if (!existsSync(wrapperScript)) {
    console.error(`[dippy] Wrapper script not found: ${wrapperScript}`);
    return;
  }

  console.log("[dippy] Extension loaded (Bash + File Access)");

  pi.on("tool_call", async (event, ctx) => {
    let dippyInput: DippyInput | null = null;
    let label = "";

    // Map pi-mono tools to Dippy requests
    switch (event.toolName) {
      case "bash":
        const command = event.input?.command as string;
        if (command) {
          dippyInput = { type: 'bash', command, cwd: ctx.cwd };
          label = `Command: ${command}`;
        }
        break;

      case "read":
        const readPath = event.input?.path as string;
        if (readPath) {
          dippyInput = { type: 'read', path: readPath, cwd: ctx.cwd };
          label = `Read: ${readPath}`;
        }
        break;

      case "write":
      case "edit":
        const editPath = event.input?.path as string;
        if (editPath) {
          dippyInput = { type: 'edit', path: editPath, cwd: ctx.cwd };
          label = `Edit: ${editPath}`;
        }
        break;
    }

    if (!dippyInput) return undefined;

    try {
      const decision = await validateDippy(wrapperScript, dippyInput);

      // Log decision for debugging
      console.log(`[dippy] ${decision.action} [${event.toolName}]: ${decision.reason}`);

      switch (decision.action) {
        case "allow":
        case "pass":
          return undefined;

        case "ask":
          if (!ctx.hasUI) {
            return {
              block: true,
              reason: decision.reason || "Dippy requires approval (no UI available)",
            };
          }

          const approved = await ctx.ui.confirm(
            "Dippy Approval Required",
            `${decision.reason || "Dippy requires approval"}\n\n${label}`
          );

          if (!approved) {
            return { block: true, reason: "User declined tool execution" };
          }
          return undefined;

        case "deny":
          return {
            block: true,
            reason: decision.reason || "Action blocked by dippy",
          };
      }
    } catch (error) {
      const errorMsg = error instanceof Error ? error.message : String(error);
      console.error(`[dippy] Validation error: ${errorMsg}`);
      return {
        block: true,
        reason: `Dippy validation error: ${errorMsg}`,
      };
    }
  });
}
