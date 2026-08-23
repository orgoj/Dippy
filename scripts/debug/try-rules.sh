#!/usr/bin/env bash
# Try candidate rules against sample commands, isolated from the live config.
#
# Usage:
#   scripts/debug/try-rules.sh RULES_FILE [--cwd DIR] [--home DIR] CMD [CMD ...]
#   scripts/debug/try-rules.sh RULES_FILE [--cwd DIR] [--home DIR] -   # commands on stdin
#
# RULES_FILE is a standalone Dippy config. HOME is redirected to an empty
# directory so ~/.dippy/config never leaks into the result. Pass --home to point
# it somewhere else when a rule contains `~` that must expand to a real path;
# that directory must not contain a .dippy/config.
set -uo pipefail

rules=${1:?usage: try-rules.sh RULES_FILE [--cwd DIR] CMD ...}
shift

cwd=$PWD
home=
while :; do
    case ${1:-} in
        --cwd) cwd=$2; shift 2 ;;
        --home) home=$2; shift 2 ;;
        *) break ;;
    esac
done

empty_home=$(mktemp -d)
trap 'rm -rf "$empty_home"' EXIT
[[ -z $home ]] && home=$empty_home

run_one() {
    local cmd=$1 out
    out=$(HOME="$home" dippy --config "$rules" --json --cwd "$cwd" --cmd "$cmd" 2>&1)
    printf '%-60s %s\n' "$cmd" "$out"
}

if [[ ${1:-} == - ]]; then
    while IFS= read -r line; do
        [[ -n $line ]] && run_one "$line"
    done
else
    for cmd in "$@"; do
        run_one "$cmd"
    done
fi
