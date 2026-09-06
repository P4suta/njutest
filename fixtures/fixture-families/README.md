<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-families

One library whose source is `crates/rust-mutants/tests/testdata/syntax/families.input`
verbatim: at least one site for every v1 rule. The golden beside that
input lists every candidate; discovery over this fixture must find exactly
those 119 candidates in `src/lib.rs` and count one `test-code` skip for the
`assert!` in its test module.
