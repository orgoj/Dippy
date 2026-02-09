from pathlib import Path
from dippy.core.config import load_config, match_read

cwd = Path("/home/michael/work/ai/CLAUDE/setup")
path = "/home/michael/work/ai/SKILLS/wshobson-agents/plugins/context-management/agents/context-manager.md"

# Nacteme tvuj REALNY config z disku
config = load_config(cwd)

match = match_read(path, config, cwd)

if match:
    print(f"DECISION: {match.decision}")
    print(f"MATCHED PATTERN: {match.pattern}")
    print(f"FROM SOURCE: {match.source}")
else:
    print(f"NO MATCH - DEFAULTING TO: {config.default}")
