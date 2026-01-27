# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Agent Identification** - Audit log now includes `agent` field (`claude`, `gemini`, `cursor`, `pi`, `cli`)
- **File Read Approval** - `allow-read`, `ask-read`, `deny-read` rules
  - Added full support for `Read` tool in Claude Code and pi-mono
  - Native config directives for read operations (replaces synthetic cat checks)
  - Consistent glob matching with file edit rules
- **pi-mono extension enhancement** - Full tool validation for [pi-mono](https://github.com/badlogic/pi-mono)
  - Added file access control for `read`, `write`, and `edit` tools
  - Integrated native `allow-edit` and `allow-read` rules
  - Updated `pi-extension/dippy-extension.ts` and `src/dippy/pi_wrapper.py` for multi-tool support
  - Updated [pi-extension/README.md](pi-extension/README.md) with file-specific configuration examples

## [Previous Versions]

See upstream [ldayton/Dippy](https://github.com/ldayton/Dippy) for changes before this fork.

### Fork Features (not in upstream)

- File Edit/Read Approval - `allow-edit`, `allow-read` etc. rules
- Include directive - `include <path-or-glob>` for composable configs
- Context-aware rules - `[flags]` syntax with `@subshell`, `@compound`, negation
- Custom wrappers - `wrapper <name>` for project-specific tools
- Option rules - `allow-opt`, `ask-opt`, `deny-opt` for subcommand control
- WebSearch support - auto-approval for WebSearch tool
- Structured JSON output - for PostToolUse hooks
- SSH/sudo handlers - remote context support
- Log rotation - `set log-rotate-max-days N`
- Hook approvals log control - `set log-hook-approvals off`
- Hybrid mode - `set default pass`
- Audit log - `cwd` field added
- CLI mode - standalone command validation
