# Gemini CLI Setup Guide for Dippy

This guide explains how to integrate Dippy with Gemini CLI to automate tool approvals.

## Recommended Setup (Automated)

The easiest way to set up Dippy for Gemini CLI is using the built-in hooks manager:

```bash
# 1. Install the hooks
dippy hooks install gemini --global

# 2. Enable Pure Dippy Control (YOLO mode)
dippy hooks setup-gemini-yolo --global
```

This will automatically configure your `~/.gemini/settings.json` with the correct hooks and set `approvalMode` to `yolo` so that Dippy has full authority over command approvals.

---

## Manual Configuration (Advanced)

1. Find the absolute path to `dippy`:
   - Usually `$(which dippy)` (e.g., `/home/user/.local/bin/dippy`)

2. Edit your Gemini CLI settings file at `~/.gemini/settings.json`.

3. Add the following `hooks` configuration:

```json
{
  "approvalMode": "yolo",
  "hooks": {
    "BeforeTool": [
      {
        "matcher": "run_shell_command|write_file|replace|read_file|google_web_search",
        "hooks": [
          {
            "name": "dippy",
            "type": "command",
            "command": "dippy --gemini",
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
            "command": "dippy --gemini",
            "description": "Dippy post-tool feedback"
          }
        ]
      }
    ]
  }
}
```

**Note:** Setting `"approvalMode": "yolo"` is critical for "Pure Dippy Control". Without it, Gemini may still prompt you for commands that Dippy has already allowed.

## Supported Tools

Dippy can intercept and approve/deny the following Gemini CLI tools:

| Category | Tool Name | Dippy Rule |
|----------|-----------|------------|
| Shell | `run_shell_command` | `allow`, `ask`, `deny` |
| Files | `write_file`, `replace` | `allow-edit`, `ask-edit`, `deny-edit` |
| Files | `read_file`, `read_many_files` | `allow-read`, `ask-read`, `deny-read` |
| Web | `google_web_search` | `allow-web`, `ask-web`, `deny-web` |

## How it works

- **BeforeTool**: Runs before the tool executes. If Dippy returns `allow`, the tool runs immediately (provided YOLO mode is active). If `ask`, Gemini prompts you. If `deny`, Dippy exits with **Exit Code 2**, which blocks the tool and provides immediate feedback to the agent without a confirmation dialog.
- **AfterTool**: Runs after the tool completes. Used for providing feedback to the agent (e.g., reminding it to check something after a command).

## Configuration

Dippy uses the same configuration files for Gemini CLI as it does for Claude Code:
- User global: `~/.dippy/config`
- Project-local: `.dippy` in project root

See [Configuration Documentation](../config.md) for more details.

## Troubleshooting

- **Logs**: Check `~/.gemini/hook-approvals.log` for Dippy's internal logs when running in Gemini mode.
- **Diagnostics**: Run `dippy doctor --agent gemini` to check your configuration.
- **Manual Check**: You can test how Dippy sees a command with `dippy --cmd "your command" --gemini`.

## References

- [Gemini CLI Hooks Official Documentation](https://geminicli.com/docs/hooks/)
- [Pure Dippy Control Guide](./gemini-cli-hooks.md)
