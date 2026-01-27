# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **pi-mono extension enhancement** - Full tool validation for [pi-mono](https://github.com/badlogic/pi-mono)
  - Added file access control for `read`, `write`, and `edit` tools
  - Integrated native `allow-edit`, `ask-edit`, `deny-edit` rules for file modifications
  - Added synthetic `cat` validation for file reads to reuse existing command safelists
  - Updated `pi-extension/dippy-extension.ts` and `src/dippy/pi_wrapper.py` for multi-tool support
  - Updated [pi-extension/README.md](pi-extension/README.md) with file-specific configuration examples

## [Previous Versions]

See upstream [ldayton/Dippy](https://github.com/ldayton/Dippy) for changes before this fork.

### Fork Features (not in upstream)

- File Edit Approval - `allow-edit`, `ask-edit`, `deny-edit` rules
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
