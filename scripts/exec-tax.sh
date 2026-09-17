#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 njutest contributors
# SPDX-License-Identifier: MIT OR Apache-2.0
#
# What it costs to run a file that has just been written, which the toolchain
# suites do once per fixture binary they build.
#
# A system that evaluates an executable before it may run pays that cost once
# per file, on the first execution and never again. Where that evaluation has a
# backlog the first execution costs minutes: a suite that would take two takes
# an hour, nothing in the output says why, and a process waiting on it looks
# hung rather than slow. Nothing else a caller can see reports it, so this
# writes one file, runs it twice, and prints both numbers. The pair is the
# evidence: one slow run could be a slow disk, and a slow run beside a fast run
# of the same file cannot be anything else.
set -uo pipefail

limit=${EXEC_TAX_LIMIT:-5}
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
# The probe is a copy of a real program, not a script. What the system
# evaluates is a newly written executable, and a `#!/bin/sh` file is not one:
# it is read by an interpreter that was evaluated long ago. Measured side by
# side on a machine in this state, a fresh script cost two seconds and a fresh
# copy of a real binary cost a hundred and seventy-six. A probe that asked the
# cheap question would have reported a machine that was fine.
probe="$dir/probe"
cp "$(command -v git)" "$probe" 2>/dev/null || {
    echo "exec-tax: no program to copy as a probe" >&2
    exit 0
}
chmod +x "$probe"

elapsed() {
    local start end
    start=$(date +%s)
    "$probe" --version >/dev/null 2>&1 || return 1
    end=$(date +%s)
    echo $((end - start))
}

first=$(elapsed) || { echo "exec-tax: the probe could not be run" >&2; exit 1; }
second=$(elapsed) || { echo "exec-tax: the probe could not be run again" >&2; exit 1; }

printf 'exec-tax  a newly written file took %ss to run, and %ss the second time\n' \
    "$first" "$second"

if [ "$first" -ge "$limit" ]; then
    cat >&2 <<MESSAGE

This machine is evaluating new executables before they may run. The suites
below build a binary per fixture and run each one once, so every one of them
pays what the first number says, and what you would measure is the evaluation
rather than the tests. A process waiting on it looks hung: a sample of one
shows a single frame in the dynamic loader.

Wait until the first number is under a second and run this again. Nothing you
change in the tree makes it shorter, and raising a timeout only moves where it
gives up. Set EXEC_TAX_LIMIT to override this check.
MESSAGE
    exit 1
fi
