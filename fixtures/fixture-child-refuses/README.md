<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-child-refuses

One function and one test, which runs its own binary again as a child and hands the child its environment through the library.
The mutation that empties what `handed_on` returns corrupts the catalog the child is told about, so the child's runtime refuses it — it prints `rust-mutants: this binary was built from catalog …` and exits 97 — and the parent test, which expected the child to succeed, fails and prints what the child said.

That is the shape an adopter met with a crate that builds environment blocks: the refusal was the child's, the failure was the test's, and the run concluded `errored` because the refusal's words appeared in the output.
A refusal is the process's own only when the process exits with the refusal's code, so here the test noticed the mutation and the run concludes `killed`.

| Path | Unit | Candidates |
| --- | --- | --- |
| `src/lib.rs` | lib | `handed_on`: its return |

```fates
src/lib.rs:9:5 return-default killed
```
