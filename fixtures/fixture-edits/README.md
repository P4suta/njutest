<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-edits

A library each of whose edits changes the outcome of a target that never enters the code the edit is in.
It is the ground truth a selection is held to: a selection that skips a target an edit breaks is unsound, whatever it reasoned from.

Every edit is a directory under `edits/` holding each file as the edit leaves it, or a `FEATURES` file where the edit is to how the library is built rather than to what it says.
`cargo test -p njutest --test toolchain_edits` applies each edit to a copy, runs every target, and refuses a difference from the block below.
Nothing in it asks a selection anything: it establishes what an edit breaks, independently of whatever will later be asked to predict it.

| Edit | What it changes | Why what ran the edited code is not what notices |
| --- | --- | --- |
| `allman-signature` | a `-> u64` inserted between `make`'s signature and the brace that opens its body on the next line | `named(make)` reads what `make` returns off its type and never calls it |
| `impl-added-in-body` | an `impl Drop for Beta` that panics, inside the body of `alpha` | nothing calls `alpha`, and every target that drops a `Beta` runs the new drop |
| `line-shift-location` | a line added to the body of `early` | `late` never calls `early`, but the line it calls `here` on moved |
| `include-str-of-module` | the value in `data.rs` | nothing calls `value`, and `included::SOURCE` is the file's text |
| `tree-reading-test` | `quiet`'s body becomes `todo!()` | nothing calls `quiet`, and a test reads its file the way a lint reads a tree |
| `env-clear-child` | the comparison in `decide` | only the binary calls `decide`, in a process the test starts with nothing inherited |
| `untracked-compiled-file` | the value in `generated.rs` | a file a checkout may leave untracked is still compiled |
| `feature-change` | the library is built with `loud` | no file changes at all |
| `proc-macro-body` | the literal the proc macro expands to | the macro runs inside the compiler, and no test process enters it |
| `custom-build-path` | what `tools/generate.rs` says | the build script runs before anything is compiled, and it is not named `build.rs` |
| `doc-comment` | the expected value in `double`'s example | a doctest is compiled from the comment, which is no function's body |
| `shadowed-std-macro` | an unqualified `vec![]` in `untouched`, whose `vec!` is the module's own and expands to an `impl` | nothing calls `untouched`, and every target that drops a `Gamma` runs what the macro wrote |
| `comment-in-body` | a comment added in `quiet`'s body | nothing: the control that says a target is not reported broken for being near an edit |

```edits
allman-signature breaks edits/test/allman
comment-in-body breaks nothing
custom-build-path breaks edits/test/built
doc-comment breaks edits/doc/edits
env-clear-child breaks edits/test/child
feature-change breaks edits/test/loud
impl-added-in-body breaks edits/test/drops
include-str-of-module breaks edits/test/included
line-shift-location breaks edits/test/located
proc-macro-body breaks edits/test/answer
shadowed-std-macro breaks edits/test/shadowed
tree-reading-test breaks edits/test/reads_tree
untracked-compiled-file breaks edits/test/generated
```

## Fates

What one run of this fixture establishes for every mutation of it, and for every candidate the compiler refused.
The run is `rust-mutants run --tier all --offline --locked --skip-target edits/test/included`; `cargo test -p rust-mutants-cli --test toolchain_fates` does it again and refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

It leaves `edits/test/included` out because that target is the class it stands for, seen from the engine's side.
Instrumenting `data.rs` changes its text, so a test that reads the file as text fails with nothing active, and the run refuses it with `RM5002` rather than report outcomes that are about the instrumentation.

```fates --skip-target edits/test/included
answer/src/lib.rs:11:5 return-default unreached
answer/src/lib.rs:11:5 string-to-empty unreached
answer/src/lib.rs:11:25 string-to-empty unreached
src/allman.rs:22:5 return-default killed
src/answered.rs:8:5 return-default killed
src/built.rs:8:5 return-default killed
src/child.rs:8:5 return-default survived
src/child.rs:8:8 condition-to-false survived
src/child.rs:8:8 condition-to-true survived
src/child.rs:8:8 negate-condition survived
src/child.rs:8:10 gt-to-ge survived
src/child.rs:8:12 int-increment survived
src/child.rs:8:16 return-default survived
src/child.rs:8:16 string-to-empty survived
src/child.rs:8:36 return-default survived
src/child.rs:8:36 string-to-empty survived
src/data.rs:8:5 int-decrement survived
src/data.rs:8:5 int-increment survived
src/data.rs:8:5 return-default survived
src/documented.rs:12:5 return-default killed
src/documented.rs:12:7 mul-to-div killed
src/documented.rs:12:9 int-decrement killed
src/documented.rs:12:9 int-increment killed
src/generated.rs:8:5 int-decrement killed
src/generated.rs:8:5 int-increment killed
src/generated.rs:8:5 return-default killed
src/located.rs:9:5 return-default killed
src/located.rs:14:5 int-decrement survived
src/located.rs:14:5 int-increment survived
src/located.rs:14:5 return-default survived
src/located.rs:19:5 return-default killed
src/loud.rs:8:5 return-default killed
src/loud.rs:8:8 condition-to-false survived
src/loud.rs:8:8 condition-to-true killed
src/loud.rs:8:8 negate-condition killed
src/loud.rs:8:33 int-decrement survived
src/loud.rs:8:33 int-increment survived
src/loud.rs:8:33 return-default survived
src/loud.rs:8:45 int-decrement killed
src/loud.rs:8:45 int-increment killed
src/loud.rs:8:45 return-default killed
src/main.rs:7:38 int-decrement survived
src/main.rs:7:38 int-increment survived
src/quiet.rs:8:5 int-increment survived
```
