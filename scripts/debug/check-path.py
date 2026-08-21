"""Check one path against the live Dippy configuration.

Usage: PYTHONPATH=src python3 scripts/debug/check-path.py PATH [CWD]
"""

import sys
from pathlib import Path

from dippy.core.config import load_config, match_read

if len(sys.argv) < 2:
    sys.exit(f"usage: {sys.argv[0]} PATH [CWD]")

path = sys.argv[1]
cwd = Path(sys.argv[2]).resolve() if len(sys.argv) > 2 else Path.cwd()

# The real config from disk, exactly as a hook would load it
config = load_config(cwd)

match = match_read(path, config, cwd)

if match:
    print(f"DECISION: {match.decision}")
    print(f"MATCHED PATTERN: {match.pattern}")
    print(f"FROM SOURCE: {match.source}")
else:
    print(f"NO MATCH - DEFAULTING TO: {config.default}")
