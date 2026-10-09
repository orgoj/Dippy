#!/usr/bin/env python3
"""Generate rust/parity/fuzz_cases.jsonl: corpus commands in shell contexts.

Deterministic. Wraps a sample of default-config corpus commands in
wrappers, pipelines, lists, subshells, substitutions and redirects, so the
parity run exercises how the analyzer composes handler decisions.
"""

from __future__ import annotations

import json
import random
from pathlib import Path

HERE = Path(__file__).resolve().parent

TEMPLATES = [
    "timeout 5 {c}",
    "nice -n 10 {c}",
    "command {c}",
    "nohup {c} &",
    "time {c}",
    "{c} | head -5",
    "cat f | {c}",
    "({c})",
    "{{ {c}; }}",
    "{c} && echo done",
    "echo start; {c}",
    "{c} || true",
    "! {c}",
    "echo $({c})",
    'echo "$({c})"',
    "echo `{c}`",
    "x=$({c})",
    "diff <({c}) /dev/null",
    "{c} > out.txt",
    "{c} 2>/dev/null",
    "{c} >> /tmp/log.txt",
    "{c} 2>&1 | tee log",
    "FOO=bar {c}",
    "env FOO=bar {c}",
    "sudo {c}",
    "sh -c '{c}'",
    "bash -c '{c}'",
    "xargs {c} < list",
    "find . -exec {c} \\\;",
    "if {c}; then echo ok; fi",
    "while {c}; do sleep 1; done",
    "for f in *; do {c}; done",
    "[[ -n $({c}) ]]",
    "f() {{ {c}; }}; f",
    "cd /tmp && {c}",
    "ssh host {c}",
    "docker exec ctr {c}",
    "kubectl exec pod -- {c}",
    "watch {c}",
    "{c} # trailing comment",
]


def main() -> None:
    cases = [
        json.loads(line) for line in (HERE / "corpus.jsonl").read_text().splitlines()
    ]
    pool = sorted(
        {
            c["cmd"]
            for c in cases
            if not c["config"]
            and c["src"] != "fuzz"
            and "\n" not in c["cmd"]
            and len(c["cmd"]) < 120
        }
    )
    random.seed(1009)
    out = []
    for template in TEMPLATES:
        for cmd in random.sample(pool, 90):
            if "'" in cmd and "'{c}'" in template:
                continue
            out.append(template.format(c=cmd))
    with open(HERE / "fuzz_cases.jsonl", "w", encoding="utf-8") as fh:
        for cmd in dict.fromkeys(out):
            fh.write(json.dumps({"cmd": cmd}, ensure_ascii=False) + "\n")
    print(len(out))


if __name__ == "__main__":
    main()
