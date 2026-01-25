/**
 * Dippy Extension for pi-mono
 *
 * Integrates dippy's bash command approval system with pi-mono.
 * Intercepts bash tool calls and validates them through dippy.
 *
 * Installation:
 *   ln -s /path/to/dippy-dev/pi-extension/dippy-extension.ts \
 *         ~/.pi/agent/extensions/dippy-extension.ts
 *
 * Requirements:
 *   - Dippy must be installed in system Python (pip install dippy)
 *   - dippy's pi_wrapper.py must be accessible
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
  command: string;
  cwd: string;
}

interface DippyDecision {
  action: 'allow' | 'ask' | 'deny' | 'pass';
  reason: string;
  context_flags?: string[];
  error?: boolean;
}

/**
 * Validate a command through dippy's Python wrapper.
 */
async function validateCommand(
  wrapperScript: string,
  command: string,
  cwd: string
): Promise<DippyDecision> {
  return new Promise((resolve, reject) => {
    const python = spawn("python3", [wrapperScript], {
      cwd: cwd,
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

    // Send input
    const inputData: DippyInput = { command, cwd };
    python.stdin.write(JSON.stringify(inputData));
    python.stdin.end();
  });
}

/**
 * Main extension function.
 */
export default function dippyExtension(pi: ExtensionAPI) {
  // Path to Python wrapper script
  // Extension is at: /path/to/dippy-dev/pi-extension/dippy-extension.ts
  // Wrapper is at:   /path/to/dippy-dev/src/dippy/pi_wrapper.py
  const wrapperScript = join(__dirname, "../src/dippy/pi_wrapper.py");

  // Verify wrapper exists
  if (!existsSync(wrapperScript)) {
    console.error(`[dippy] Wrapper script not found: ${wrapperScript}`);
    console.error("[dippy] Extension will not function correctly");
    return;
  }

  console.log("[dippy] Extension loaded");

  // Hook into bash tool calls
  pi.on("tool_call", async (event, ctx) => {
    // Only intercept bash tool
    if (event.toolName !== "bash") return undefined;

    const command = event.input?.command as string;
    if (!command) return undefined;

    try {
      // Validate command with dippy
      const decision = await validateCommand(wrapperScript, command, ctx.cwd);

      // Log decision for debugging
      console.log(`[dippy] ${decision.action}: ${decision.reason}`);

      // Handle decision
      switch (decision.action) {
        case "allow":
          // Command is safe - proceed with execution
          return undefined;

        case "pass":
          // Dippy is passing through - let pi-mono handle it
          return undefined;

        case "ask":
          // Prompt user for approval
          if (!ctx.hasUI) {
            // No UI available - block conservatively
            return {
              block: true,
              reason: decision.reason || "Dippy requires approval (no UI available)",
            };
          }

          const approved = await ctx.ui.confirm(
            "Dippy Approval Required",
            `${decision.reason || "Dippy requires approval"}\n\nCommand: ${command}`
          );

          if (!approved) {
            return { block: true, reason: "User declined command" };
          }
          return undefined;

        case "deny":
          // Block execution
          return {
            block: true,
            reason: decision.reason || "Command blocked by dippy",
          };
      }
    } catch (error) {
      // On error, log and block conservatively
      const errorMsg = error instanceof Error ? error.message : String(error);
      console.error(`[dippy] Validation error: ${errorMsg}`);
      return {
        block: true,
        reason: `Dippy validation error: ${errorMsg}`,
      };
    }
  });
}
