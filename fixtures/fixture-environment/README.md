<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-environment

A library each of whose tests depends on exactly one thing a machine may set differently, and one test that depends on nothing.
It is what a run of the repeatability knobs is held to: each knob has to move the verdict of the one target that depends on it and of no other.

| Target | What it depends on | What moves it |
| --- | --- | --- |
| `timezone` | the zone the process is in | `TZ=Australia/Lord_Howe`, a half-hour offset whose daylight saving moves by half an hour |
| `locale` | the language the process speaks | `LC_ALL=tr_TR.UTF-8`, whose dotless i breaks case folding |
| `temp` | a temporary path built into a command line without quotes | a temporary directory whose path holds a space |
| `home` | a home directory with something in it | an empty home directory |
| `umask` | a new file being readable by its group | `umask 077` |
| `columns` | room for forty columns | `COLUMNS=37` |
| `threads` | `a_waits_for_the_signal` only passing while `b_signals` runs beside it | `--test-threads=1` |
| `steady` | nothing | nothing |

The doctest on `double` depends on nothing either, and it is run through cargo, which finds its toolchain by the home directory: a knob that moved the home directory without keeping cargo's would fail it for a reason that is about the apparatus.

## Fates

What one run of this fixture establishes for every mutation of it, and for every candidate the compiler refused.
The run is `rust-mutants run --tier all --offline --locked`; `cargo test -p rust-mutants-cli --test toolchain_fates` does it again and refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:10:5 return-default survived
src/lib.rs:10:19 string-to-empty survived
src/lib.rs:10:44 string-to-empty survived
src/lib.rs:15:5 return-default survived
src/lib.rs:15:19 string-to-empty survived
src/lib.rs:16:36 string-to-empty survived
src/lib.rs:17:29 string-to-empty survived
src/lib.rs:22:5 return-default killed
src/lib.rs:27:39 string-to-empty killed
src/lib.rs:28:16 false-to-true survived
src/lib.rs:30:5 return-true survived
src/lib.rs:31:28 return-true survived
src/lib.rs:31:43 is-some-to-is-none killed
src/lib.rs:32:19 false-to-true survived
src/lib.rs:38:5 return-default killed
src/lib.rs:38:19 string-to-empty survived
src/lib.rs:41:20 int-decrement survived
src/lib.rs:41:20 int-increment survived
src/lib.rs:50:5 return-default killed
src/lib.rs:50:7 mul-to-div killed
src/lib.rs:50:9 int-decrement killed
src/lib.rs:50:9 int-increment killed
```
