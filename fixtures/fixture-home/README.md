<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-home

One library that keeps a setting under the home directory, as a command-line tool keeps its configuration, and one integration test that writes the setting and reads it back.

Under measurement every execution's `HOME` is a directory inside its own scratch ([ADR 0044](../../docs/adr/0044-a-test-writes-only-where-its-execution-may.md)), so the write lands there and never in the home the run was given, whatever a mutation did to the path.
`toolchain_cli_contract`'s `a_write_a_test_makes_under_its_home_lands_in_its_execution` runs this fixture with a home of its own and finds it untouched afterwards.
The same test adds a test that reads a setting only the given home holds, which passes only with the given home, and finds that target reported `unconfined-target` and measured with it.

## Fates

What one run of this fixture establishes for every mutation of it, and for every candidate the compiler refused.
The run is `rust-mutants run --tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:11:5 return-default killed
src/lib.rs:11:5 return-some-default killed
src/lib.rs:11:30 question-to-unwrap survived
src/lib.rs:11:37 string-to-empty survived
src/lib.rs:11:59 string-to-empty killed
src/lib.rs:19:67 string-to-empty unreached
src/lib.rs:19:78 question-to-unwrap survived
src/lib.rs:21:9 delete-call-statement killed
src/lib.rs:21:37 ignore-question-statement survived
src/lib.rs:21:37 question-to-unwrap survived
src/lib.rs:23:5 return-ok-default killed
src/lib.rs:31:67 string-to-empty unreached
src/lib.rs:31:78 question-to-unwrap survived
src/lib.rs:32:5 return-ok-default killed
```
