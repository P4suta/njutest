<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-strict-lints

A crate that denies every lint the code the engine plants could trip, so a guard, a checkpoint, or the runtime module that trips one fails validation here rather than in somebody's project.
The set is the union of this repository's own list and an adopter's `[workspace.lints.rust]`: `warnings`, `unused` and `future_incompatible` as groups; `unsafe_code`, `non_ascii_idents`, `unexpected_cfgs`, `missing_debug_implementations`, `missing_copy_implementations`, `unreachable_pub`, `unused_qualifications`, `unused_results`, `unused_import_braces`, `elided_lifetimes_in_paths`, `unused_lifetimes`, `single_use_lifetimes`, `redundant_lifetimes`, `trivial_casts`, `trivial_numeric_casts`, `let_underscore_drop`, `meta_variable_misuse`, `unit_bindings`, `variant_size_differences` and `ambiguous_negative_literals`.
A lint somebody denies that trips generated code joins this list with the fix.

The runtime module lives at the file root and is reached from three depths: the root itself, `signs`, which glob-imports its parent and so already has the runtime's name in scope, and `signs::plain`, which imports nothing.
A qualified call from `signs` is one `unused_qualifications` refuses; an unqualified one from `plain` does not resolve.

| Path | Unit | Candidates |
| --- | --- | --- |
| `src/lib.rs` | lib and test | `max`, `is_positive`, `is_negative`, `signs::positive`, `signs::negative`, `signs::plain::negative` |

## Fates

What one run of this fixture establishes for every mutation of it, and for every candidate the compiler refused.
The run is `rust-mutants run --tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:8:5 return-default killed
src/lib.rs:8:8 condition-to-false killed
src/lib.rs:8:8 condition-to-true killed
src/lib.rs:8:8 negate-condition killed
src/lib.rs:8:10 gt-to-ge survived
src/lib.rs:8:16 return-default killed
src/lib.rs:8:27 return-default killed
src/lib.rs:13:5 return-true killed
src/lib.rs:18:5 return-true killed
src/lib.rs:25:9 return-true killed
src/lib.rs:25:16 int-increment killed
src/lib.rs:25:19 gt-to-ge killed
src/lib.rs:25:21 int-increment killed
src/lib.rs:29:9 return-true killed
src/lib.rs:34:13 return-true killed
src/lib.rs:34:15 lt-to-le killed
src/lib.rs:34:17 int-increment killed
```
