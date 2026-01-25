# Plan: Dippy Extension for pi-mono

## Overview
Create a **completely independent pi-mono extension** that integrates dippy's bash command approval system. The extension will be installable via `pi install` with **zero modifications** to pi-mono core code.

## Key Principle: External Extension Architecture
- ✅ **No pi-mono core modifications**
- ✅ **Extension lives in dippy-dev repository**
- ✅ **Installable via `pi install npm:dippy-extension`**
- ✅ **Uses official pi-mono extension API**
- ✅ **Fully distributed as npm package**

## Current State Analysis

### Dippy (Developed by You)
- **Location**: `/home/michael/work/ai/CLAUDE/TOOLS/dippy-dev`
- **Language**: Python with bash AST parser
- **Security Model**: Whitelist-based (allow/ask/deny)
- **Modifiable**: You control the codebase
- **Capabilities**:
  - AST analysis of bash commands
  - Context-aware security (pipelines, subshells, redirects)
  - Hierarchical configuration
  - Extensible rule system

### Pi-mono Extension System
- **Extension API**: Fully documented with TypeScript interfaces
- **Loading**: Automatic discovery from `~/.pi/agent/extensions/` or npm packages
- **Capabilities**:
  - Tool execution hooks (`tool_call`, `tool_result`)
  - Custom tool registration
  - UI integration (prompts, dialogs)
  - State persistence
  - Command registration
- **Distribution**: npm, git, local installation
- **Examples**: prompt-url-widget, diff-tool, plan-mode

## Architecture Decision

### Approach: **Standalone Extension in dippy-dev**

Create extension **inside dippy-dev repository** that:

1. **Wraps Python dippy core** via subprocess
2. **Implements pi-mono Extension API** in TypeScript
3. **Distributes as npm package** with bundled Python
4. **Hooks into bash tool** via `tool_call` event
5. **Provides approval UI** via pi-mono UI components

### Why This Approach?

**Pros:**
- ✅ **Zero pi-mono modifications** - Pure extension usage
- ✅ **Single codebase** - dippy + extension together
- ✅ **Easy updates** - Update dippy, extension follows
- ✅ **Clear ownership** - You control both Python and TS
- ✅ **Proper distribution** - npm package with dependencies
- ✅ **Flexible installation** - npm, git, local paths

**Cons:**
- ⚠️ Subprocess overhead (~50-100ms per validation)
- ⚠️ Python runtime dependency
- ⚠️ Package size (includes Python dependencies)

## Implementation Plan

### Phase 1: Python JSON API (Dippy Side)

**File: `src/dippy/pi_api.py`** (NEW)

Create JSON endpoint for command validation:

```python
#!/usr/bin/env python3
"""
pi-mono extension API for dippy
Provides JSON interface for command validation
"""
import json
import sys
from pathlib import Path
from typing import Dict, Any

# Add dippy to path
sys.path.insert(0, str(Path(__file__).parent.parent))

from dippy.core.analyzer import CommandAnalyzer
from dippy.core.config import Config


def validate_command_json(input_data: Dict[str, Any]) -> Dict[str, Any]:
    """
    Validate a command and return JSON decision

    Input:
    {
        "command": "rm -rf node_modules",
        "context": {
            "cwd": "/path/to/project",
            "isSubshell": false,
            "isPipeline": false
        }
    }

    Output:
    {
        "action": "ask",  // "allow" | "ask" | "deny"
        "message": "This will delete files",
        "rule": "destructive",
        "confidence": 0.95
    }
    """
    command = input_data.get('command', '')
    context = input_data.get('context', {})

    # Load config from current working directory
    config = Config.from_cwd(context.get('cwd'))

    # Analyze command
    analyzer = CommandAnalyzer(config)
    decision = analyzer.analyze(
        command,
        is_subshell=context.get('isSubshell', False),
        is_pipeline=context.get('isPipeline', False)
    )

    # Return JSON decision
    return {
        'action': decision.action,  # 'allow' | 'ask' | 'deny'
        'message': decision.message,
        'rule': decision.rule,
        'confidence': decision.confidence
    }


def main():
    """Read JSON from stdin, validate, output JSON"""
    try:
        # Read input
        input_data = json.loads(sys.stdin.read())

        # Validate command
        result = validate_command_json(input_data)

        # Output JSON
        print(json.dumps(result))
        sys.exit(0)

    except Exception as e:
        # Error handling
        error_result = {
            'action': 'ask',  # Conservative fallback
            'message': f'Dippy error: {str(e)}',
            'error': True
        }
        print(json.dumps(error_result))
        sys.exit(1)


if __name__ == '__main__':
    main()
```

**Usage:**
```bash
echo '{"command":"rm -rf node_modules"}' | python3 pi_api.py
```

### Phase 2: TypeScript Extension (Dippy Side)

**File: `pi-extension/package.json`** (NEW)

```json
{
  "name": "dippy-extension",
  "version": "1.0.0",
  "description": "Bash command approval system for pi-mono",
  "keywords": ["pi-package", "security", "bash", "approval"],
  "main": "dist/index.js",
  "types": "dist/index.d.ts",
  "pi": {
    "extensions": ["./dist/index.js"]
  },
  "scripts": {
    "build": "tsc",
    "prepublishOnly": "npm run build",
    "test": "jest"
  },
  "dependencies": {
    "@mariozechner/pi-coding-agent": "^2.0.0"
  },
  "devDependencies": {
    "@types/node": "^20.0.0",
    "typescript": "^5.0.0",
    "jest": "^29.0.0"
  },
  "files": [
    "dist",
    "python"
  ]
}
```

**File: `pi-extension/src/index.ts`** (NEW)

```typescript
import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import { spawn } from "child_process";
import { dirname, join } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));

interface DippyInput {
  command: string;
  context: {
    cwd?: string;
    isSubshell?: boolean;
    isPipeline?: boolean;
  };
}

interface DippyOutput {
  action: 'allow' | 'ask' | 'deny';
  message?: string;
  rule?: string;
  confidence?: number;
  error?: boolean;
}

export default function dippyExtension(pi: ExtensionAPI) {
  const pythonScript = join(__dirname, "../python/pi_api.py");

  // Hook into tool execution
  pi.on("tool_call", async (event, ctx) => {
    // Only intercept bash tool
    if (event.toolName !== "bash") return;

    const command = event.input?.command as string;
    if (!command) return;

    try {
      // Validate with dippy
      const decision = await validateCommand(command, {
        cwd: event.input?.cwd,
        isSubshell: event.input?.runInBackground === true
      });

      // Handle decision
      switch (decision.action) {
        case "allow":
          // Proceed with execution
          return;

        case "ask":
          // Prompt user for approval
          const approved = await promptUser(ctx, command, decision);
          if (!approved) {
            return { block: true, reason: "User declined command" };
          }
          return;

        case "deny":
          // Block execution
          return {
            block: true,
            reason: decision.message || "Command blocked by dippy"
          };
      }
    } catch (error) {
      // On error, log and proceed conservatively
      console.error("Dippy validation error:", error);
      // Could return { block: true } for fail-safe mode
    }
  });

  async function validateCommand(
    command: string,
    context: DippyInput["context"]
  ): Promise<DippyOutput> {
    return new Promise((resolve, reject) => {
      const python = spawn("python3", [pythonScript], {
        cwd: context.cwd || process.cwd()
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
        try {
          const result: DippyOutput = JSON.parse(stdout);
          resolve(result);
        } catch (error) {
          reject(new Error(`Failed to parse dippy output: ${stdout}`));
        }
      });

      python.on("error", (error) => {
        reject(error);
      });

      // Send input
      python.stdin.write(
        JSON.stringify({
          command,
          context
        })
      );
      python.stdin.end();
    });
  }

  async function promptUser(
    ctx: any,
    command: string,
    decision: DippyOutput
  ): Promise<boolean> {
    // Use pi-mono UI to show approval dialog
    const result = await ctx.ui.prompt({
      type: "confirm",
      message: `Execute this command?`,
      detail: `${decision.message || "Dippy requires approval"}\n\nCommand: ${command}`,
      confirm: "Allow",
      deny: "Deny"
    });

    return result === true;
  }
}
```

**File: `pi-extension/tsconfig.json`** (NEW)

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "declaration": true,
    "outDir": "./dist",
    "rootDir": "./src",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "forceConsistentCasingInFileNames": true
  },
  "include": ["src/**/*"],
  "exclude": ["node_modules", "dist"]
}
```

### Phase 3: Python Integration

**File: `pi-extension/python/pi_api.py`** (NEW)

Copy the Python JSON API from Phase 1 into the extension package.

```bash
# Build script to include Python files
mkdir -p pi-extension/python
cp src/dippy/pi_api.py pi-extension/python/
```

### Phase 4: Package Structure

Final dippy-dev structure:

```
/home/michael/work/ai/CLAUDE/TOOLS/dippy-dev/
├── src/dippy/
│   ├── core/
│   │   ├── analyzer.py
│   │   ├── config.py
│   │   └── allowlists.py
│   ├── cli/
│   └── pi_api.py          # NEW: JSON API
├── pi-extension/          # NEW: pi-mono extension
│   ├── src/
│   │   └── index.ts       # Extension implementation
│   ├── python/
│   │   └── pi_api.py      # Bundled Python script
│   ├── package.json
│   ├── tsconfig.json
│   └── dist/              # Build output
├── tests/
└── README.md
```

### Phase 5: Build and Distribution

**File: `pi-extension/build.sh`** (NEW)

```bash
#!/bin/bash
set -e

echo "Building dippy extension..."

# Copy Python script
mkdir -p dist/python
cp src/dippy/pi_api.py dist/python/

# Build TypeScript
npm run build

echo "Extension built successfully!"
echo "Install with: pi install /path/to/dippy-dev/pi-extension"
```

**Publish to npm:**
```bash
cd pi-extension
npm publish
```

### Phase 6: Installation in pi-mono

Users can install dippy extension in several ways:

**Option 1: From npm (after publishing)**
```bash
pi install npm:dippy-extension@latest
```

**Option 2: From local path**
```bash
pi install /home/michael/work/ai/CLAUDE/TOOLS/dippy-dev/pi-extension
```

**Option 3: Manual installation**
```bash
mkdir -p ~/.pi/agent/extensions
ln -s /home/michael/work/ai/CLAUDE/TOOLS/dippy-dev/pi-extension/dist/index.js \
      ~/.pi/agent/extensions/dippy.js
```

**Option 4: Add to settings.json**
```json
{
  "packages": [
    "/home/michael/work/ai/CLAUDE/TOOLS/dippy-dev/pi-extension"
  ]
}
```

### Phase 7: Configuration

**Project-level `.dippy` config:**
```python
# .dippy in project root
rules:
  # Development commands - auto-allow
  - pattern: "git status"
    decision: allow
  - pattern: "git diff*"
    decision: allow
  - pattern: "cc-find*"
    decision: allow

  # Safe read operations
  - pattern: "ls*"
    decision: allow
  - pattern: "cat*"
    decision: allow

  # Destructive - ask
  - pattern: "rm*"
    decision: ask
    message: "This will delete files"

  # Package managers - ask
  - pattern: "npm install*"
    decision: ask
    message: "This will install packages"
  - pattern: "npm uninstall*"
    decision: ask

  # Dangerous - deny
  - pattern: "rm -rf /"
    decision: deny
    message: "Destructive command blocked"

edit_rules:
  - pattern: "*.ts"
    decision: allow
  - pattern: "*.md"
    decision: allow
  - pattern: "package.json"
    decision: ask
```

**Global config (`~/.dippy/config`):**
```python
# User defaults
rules:
  - pattern: "code*"
    decision: allow  # Allow opening VS Code

  - pattern: "docker rm*"
    decision: ask
```

## Critical Files

### New Files (Dippy Side)
1. `src/dippy/pi_api.py` - Python JSON API
2. `pi-extension/src/index.ts` - Extension implementation
3. `pi-extension/package.json` - NPM package config
4. `pi-extension/tsconfig.json` - TypeScript config
5. `pi-extension/python/pi_api.py` - Bundled Python script

### Modified Files (Dippy Side)
1. `setup.py` - Add pi_api.py entry point (optional)
2. `README.md` - Add pi-mono installation instructions

### No pi-mono Files Modified! ✅

## Verification Strategy

### 1. Build Testing
```bash
cd pi-extension
npm install
npm run build
ls -la dist/  # Should have index.js, index.d.ts, python/
```

### 2. Python API Testing
```bash
# Test Python JSON API directly
cd /home/michael/work/ai/PI/pi-mono
echo '{"command":"ls","context":{}}' | \
  python3 /home/michael/work/ai/CLAUDE/TOOLS/dippy-dev/src/dippy/pi_api.py
# Expected output: {"action":"allow",...}
```

### 3. Extension Loading Test
```bash
# Install extension locally
pi install /home/michael/work/ai/CLAUDE/TOOLS/dippy-dev/pi-extension

# Check if loaded
pi --version
# Should see extension loaded
```

### 4. Functional Testing
```bash
# Start pi-mono
pi

# Test safe command (should execute immediately)
> "List files in current directory"
# AI: ls
# Expected: Executes without prompt

# Test dangerous command (should prompt)
> "Delete node_modules"
# AI: rm -rf node_modules
# Expected: Shows approval dialog

# Test blocked command
# Attempt to trigger deny rule
# Expected: Blocks with message
```

### 5. Integration Testing
```bash
# Create test .dippy config
cat > .dippy << 'EOF'
rules:
  - pattern: "echo*"
    decision: allow
  - pattern: "rm*"
    decision: ask
EOF

# Test with pi-mono
pi
```

### 6. Performance Testing
```bash
# Measure validation overhead
time echo '{"command":"ls -la"}' | python3 src/dippy/pi_api.py
# Target: < 100ms
```

## Advantages of This Approach

1. **✅ Zero pi-mono modifications** - Pure extension using public API
2. **✅ Single codebase** - Dippy and extension in same repo
3. **✅ Easy distribution** - Standard npm package
4. **✅ Version controlled** - Extension version matches dippy version
5. **✅ Independent updates** - Update extension without touching pi-mono
6. **✅ Clear separation** - Extension concerns separate from dippy core
7. **✅ Multiple installation methods** - npm, local, git
8. **✅ Proper TypeScript support** - Types from pi-mono package
9. **✅ Testing friendly** - Can test extension independently
10. **✅ Documentation friendly** - Extension can have own README

## Potential Issues and Solutions

### Issue 1: Python Not Found
**Problem**: `python3` not available in PATH
**Solution:**
```typescript
// Detect python in extension
const pythonExe = process.env.DIPPY_PYTHON || 'python3';
```

### Issue 2: Extension Not Loading
**Problem**: Build errors or missing dependencies
**Solution:**
- Add pre-install check script
- Provide clear error messages
- Document dependencies in README

### Issue 3: Performance Overhead
**Problem**: Subprocess spawning adds latency
**Solution:**
- Consider persistent Python process with IPC
- Cache validation results for repeated commands
- Profile and optimize hot paths

### Issue 4: Python Script Path Issues
**Problem**: Relative paths break when installed globally
**Solution:**
```typescript
// Use absolute path based on extension location
const extensionDir = dirname(fileURLToPath(import.meta.url));
const pythonScript = join(extensionDir, "../python/pi_api.py");
```

### Issue 5: pi-mono API Version Mismatch
**Problem**: Extension built for incompatible pi-mono version
**Solution:**
```json
// package.json
{
  "peerDependencies": {
    "@mariozechner/pi-coding-agent": "^2.0.0"
  }
}
```

## Alternative Approaches Considered

### Alternative 1: Port Dippy to TypeScript ❌
**Pros:** No subprocess overhead, single language
**Cons:**
- Complex AST parser rewrite
- Duplicate codebase
- Lose Python dippy features
- High maintenance burden

### Alternative 2: Modify pi-mono Core ❌
**Pros:** Tighter integration possible
**Cons:**
- Requires pi-mono PR/merge
- Tied to pi-mono release cycle
- Distribution pain
- Against user requirement

### Alternative 3: HTTP Server Model ❌
**Pros:** Persistent process, no spawn overhead
**Cons:**
- Background process management
- Port conflicts
- Complex lifecycle
- Overkill for this use case

## Implementation Priority

### Phase 1: MVP (Must Have)
1. ✅ Python JSON API (`pi_api.py`)
2. ✅ TypeScript extension skeleton
3. ✅ Subprocess wrapper
4. ✅ Basic `tool_call` hook
5. ✅ Simple allow/deny logic

### Phase 2: Functional (Should Have)
1. ✅ Approval UI integration
2. ✅ Configuration loading (.dippy files)
3. ✅ Error handling and fallbacks
4. ✅ Logging and diagnostics
5. ✅ Package.json and build setup

### Phase 3: Polish (Nice to Have)
1. ⭐ Caching for repeated commands
2. ⭐ "Always allow" memory
3. ⭐ Better UI with syntax highlighting
4. ⭐ Statistics dashboard
5. ⭐ Rule editor integration
6. ⭐ Comprehensive tests

## Success Criteria

- ✅ Extension loads without pi-mono modifications
- ✅ Safe commands execute automatically
- ✅ Dangerous commands show approval prompt
- ✅ Blocked commands prevent execution
- ✅ Configuration via .dippy files works
- ✅ Extension installable via `pi install`
- ✅ Performance overhead < 100ms per validation
- ✅ Clear error messages when something goes wrong
- ✅ Documentation for users and developers

## Next Steps

1. **Create Python JSON API** - Start with `pi_api.py`
2. **Create extension skeleton** - Basic `package.json` and `index.ts`
3. **Test subprocess communication** - Verify Python↔TypeScript data flow
4. **Implement tool_call hook** - Intercept bash commands
5. **Add approval UI** - Integrate with pi-mono UI
6. **Package and test** - Build, install, verify functionality
7. **Document** - README, installation guide, configuration examples
