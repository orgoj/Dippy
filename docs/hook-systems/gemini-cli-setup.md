# Gemini CLI Setup Guide for Dippy

This guide explains how to integrate Dippy with Gemini CLI to automate tool approvals.

## Prerequisites

- Gemini CLI installed and configured
- Dippy installed (either via Homebrew or manually)

## Quick Start

1. Find the absolute path to `dippy-hook` script:
   - If installed via Homebrew: `$(which dippy-hook)` (usually `/opt/homebrew/bin/dippy-hook` or `/usr/local/bin/dippy-hook`)
   - If installed manually: `/path/to/Dippy/bin/dippy-hook`

2. Edit your Gemini CLI settings file at `~/.gemini/settings.json`.

3. Add the following `hooks` configuration:

```json
{
  "hooks": {
    "BeforeTool": [
      {
        "matcher": "run_shell_command|write_file|replace|read_file|google_web_search",
        "hooks": [
          {
            "name": "dippy",
            "type": "command",
            "command": "/path/to/dippy-hook --gemini",
            "description": "Dippy approval autopilot"
          }
        ]
      }
    ],
    "AfterTool": [
      {
        "matcher": "run_shell_command|google_web_search",
        "hooks": [
          {
            "name": "dippy-post",
            "type": "command",
            "command": "/path/to/dippy-hook --gemini",
            "description": "Dippy post-tool feedback"
          }
        ]
      }
    ]
  }
}
```

**Note:** Replace `/path/to/dippy-hook` with the actual absolute path found in step 1.

## Supported Tools

Dippy can intercept and approve/deny the following Gemini CLI tools:

| Category | Tool Name | Dippy Rule |
|----------|-----------|------------|
| Shell | `run_shell_command` | `allow`, `ask`, `deny` |
| Files | `write_file`, `replace` | `allow-edit`, `ask-edit`, `deny-edit` |
| Files | `read_file`, `read_many_files` | `allow-read`, `ask-read`, `deny-read` |
| Web | `google_web_search` | `allow-web`, `ask-web`, `deny-web` |

## How it works

- **BeforeTool**: Runs before the tool executes. If Dippy returns `allow`, the tool runs immediately. If `ask`, Gemini prompts you. If `deny`, Dippy exits with **Exit Code 2**, which blocks the tool and provides immediate feedback to the agent without a confirmation dialog.
- **AfterTool**: Runs after the tool completes. Used for providing feedback to the agent (e.g., reminding it to check something after a command).

## Configuration

Dippy uses the same configuration files for Gemini CLI as it does for Claude Code:
- User global: `~/.dippy/config`
- Project-local: `.dippy` in project root

See [Configuration Documentation](../config.md) for more details.

## Troubleshooting

- **Logs**: Check `~/.gemini/hook-approvals.log` for Dippy's internal logs when running in Gemini mode.
- **Permissions**: Ensure `dippy-hook` has execution permissions (`chmod +x`).
- **Path**: Always use absolute paths in `settings.json`.

## References

- [Gemini CLI Hooks Official Documentation](https://geminicli.com/docs/hooks/)
- [Gemini CLI Hooks Reference (JSON Schema)](https://geminicli.com/docs/hooks/reference/)
