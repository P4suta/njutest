<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-unicode

Non-ASCII identifiers and comments before the code that matters, so the
question "which column?" has one answer that a test can hold both tools to.

`llvm-cov` region columns are 1-based **byte** columns; rustc's *diagnostic*
columns are **character** columns. One toolchain uses both units, so neither
is a safe default, and the engine records both for every mutant. On the line
of `大きい方`, the comparison's two columns differ by twelve, so a test that
compared a coverage region against the wrong one fails here.

`使われない` is called by nothing: its region is instrumented and uncovered,
which is what "no test reaches this" looks like in a coverage export.
