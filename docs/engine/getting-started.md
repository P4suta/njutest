<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Getting started with rust-mutants

**Status: implemented.** Every command here exists in this release. If one
does not do what this page says, that is a bug; `rust-mutants diagnostics`
gathers what a report needs.

Mutation testing changes your code in small ways and asks whether your tests
notice. A change nothing notices is a **survivor**: either a gap in the tests
or a change that could not matter, and only you can say which. That is the
whole idea; everything below is about making it cheap and honest.

## Install

```console
$ cargo install --path crates/rust-mutants-cli   # binaries: rust-mutants, cargo-rust-mutants
$ rust-mutants doctor
```

`cargo binstall rust-mutants-cli` takes the archive a release published rather
than compiling this workspace again, and `cargo rust-mutants doctor` works
wherever the binary is on the path: the same program answers to the name cargo
looks for.

`doctor` answers about this machine rather than about your code: whether cargo
and rustc are here, whether the root is a workspace, whether the packages you
would measure have test targets, and whether the LLVM tools coverage routing
wants are installed. A **warning** there is a run that will cost more or
measure less; only a **failure** stops one.

## The first run

Start narrow. One package, and an estimate before you pay for anything:

```console
$ rust-mutants run --package my-crate --dry-run
$ rust-mutants run --package my-crate
```

`--dry-run` prepares the tree, verifies it, and says how many mutants there
are, how many targets they would run against, and roughly how long that
takes. It executes nothing.

A run copies your tree, compiles it once with every mutation behind a guard,
and then runs the tests once per mutation with one guard live. Your working
tree is never written to.

## One run, end to end

Recorded from the binary by a test against one of this repository's own fixtures, so the page cannot drift from the tool:

```console
$ rust-mutants run

run       <run>
workspace <workspace digest>
catalog   <catalog digest>

discharged-mutant      every target that could have noticed f0d20edfda2959667ff1 was removed by a proof, so no test could have: the mutation is in code the tests run and never observe

MUTANTS   10 mutants were cataloged: 9 executed, 0 refused by the compiler, 3 places that produced no candidate.
OUTCOMES  killed=9 survived=0 step_limit_reached=0 waited=0 inconclusive=0 errored=0 not_run=1
OF THOSE  Those 7 add to the 10 cataloged. Within them, not run is 0 unreached, not run is 1 discharged, survived is 0 expected.
SCORE     100.0%  (9 detected of 9 decided)
WORK      started=9 of 30 pairs across 3 targets; 70.0% removed (unreached=20 never-infected=1)
          tests=9 of 30; 70.0% removed
REPORT    ./reports/mutation/<run>/run-report-v2.json

$ rust-mutants explain f0d2
NAME      src/lib.rs:max:gt-to-ge@11
MUTANT    f0d20edfda2959667ff19be48ef229cba965a672c7a6e367624a73bc4fcec22f
SHORT     f0d20edfda2959667ff1
RULE      gt-to-ge@1 (comparison)
WHERE     src/lib.rs:11:10
EDIT      ">" => ">="
RUN       <run>
OUTCOME   not_run
TIMING    <duration>
ROUTE     discharged reaching [] executed []
PROVED    fixture-simple/lib/fixture_simple: never-infected
REPRODUCE rust-mutants run --mutant src/lib.rs:max:gt-to-ge@11
ACCEPT    [[mutation.expect]]
          path = "src/lib.rs"
          item = "max"
          rule = "gt-to-ge"
          original = ">"
          line = 11
          reason = ""  # why this is not a gap in the tests

--- a/src/lib.rs
+++ b/src/lib.rs
@@ -8,7 +8,7 @@
 
 /// The larger of two numbers, spelled with a comparison a mutant can flip.
 pub fn max(a: i32, b: i32) -> i32 {
-    if a > b { a } else { b }
+    if a >= b { a } else { b }
 }
 
 /// Whether `n` is even.
```

That mutation is never run.
The guards of the instrumented tree hold both branches at `a > b`, and on the one baseline run they never answered differently, so the engine reports what running it would have established rather than spending a process on it ([ADR 0015](../adr/0015-the-guard-is-the-infection-probe.md)).
It is a finding all the same, and the exit code is 1: a mutation the tests run and cannot notice is the same gap as one they run and do not notice.
The score is over what a run *decided*, and this one was decided by a proof rather than by a test.

## Reading the summary

```
MUTANTS   cataloged=117 refused=3 skipped=17 executed=113
OUTCOMES  killed=98 survived=14 step_limit_reached=1 waited=0 inconclusive=0 errored=0 not_run=4 unreached=4 …
SCORE     87.5%  (98 detected of 112 decided)
```

- **killed** — a test failed with the mutation live. The tests noticed.
- **survived** — every test passed. Nobody noticed.
- **step limit reached** — the active guard crossed its configured count. The
  nonce-correlated notice makes that execution fact exact, but a finite count
  cannot prove nontermination; it is neither detected nor survived.
- **waited** — a bound expired before anything finished. Not counted as
  noticed, and not counted against the tests either: nothing was established.
- **inconclusive** — the run could not decide, and says so rather than guessing.
- **not run** — nothing executed it, and each one says why: `unreached` (no
  measured test reaches it), `discharged` (a proof says no target could have
  noticed), `unselected`, `stopped-early`, `interrupted`.
- **refused** — the compiler would not accept the mutation. Not a finding.
- **skipped** — discovery passed over the place, and every skip states a reason.

The exit code is `0` when there is no finding, `1` when there is, `2` when the
run itself failed, `130` when it was interrupted.

## Killing a survivor

```console
$ rust-mutants explain 16b072bfbcb2
$ rust-mutants replay 16b072bfbcb2
```

`explain` reads what the run stored — no rebuild — and shows the mutation as a
diff, the route it took, what the tests did, and the command that puts it back
in front of them. `replay` runs that command for you and says whether it is
still what the run said.

Then write the test that fails with the mutation live. That is the whole job.

## Accepting one with a reason

Some survivors cannot be killed, because the change cannot matter. Say so
where the next reader will see it:

```rust
// rust-mutants: skip the bound is equivalent under the invariant the type carries
if index <= limit {
```

or in `.rust-mutants.toml`, addressed by where the mutation is rather than by
an identity your next edit changes:

```toml
[[mutation.expect]]
path = "src/lib.rs"
item = "clamp"
rule = "le-to-lt"
original = "<="
reason = "the bound is equivalent under the invariant the type carries"
outcome = "survived"
```

A reason is required, both times. A run confirms the claim: if the mutant is
killed after all, that is a **stale expectation** and a finding, so an
accepted survivor never quietly hides a real one. A marker or an entry that
hides nothing is an **unmatched skip**, also a finding, so the file never
fills up with claims about code that moved.

## Narrowing a run

```console
$ rust-mutants run --changed                       # only what differs from HEAD
$ rust-mutants run --file src/parser.rs:40-120     # one file, or a stretch of it
$ rust-mutants run --rule gt-to-ge --rule lt-to-le # only these operators
$ rust-mutants run --from-report --outcome survived  # last run's survivors again
$ rust-mutants rules                                 # what there is to name
```

`--tier balanced` is the default; `strong` and `all` ask more questions and
cost more. `operators = [...]` in the configuration pins an exact set, so a
release that adds an operator does not change what your gate measures until
you say so.

## Continuous integration

```yaml
- run: rust-mutants run --jobs 4 --shard ${{ matrix.shard }}/4 --run-id "${{ github.run_id }}-${{ matrix.shard }}"
- run: rust-mutants merge --runs "${{ github.run_id }}-*" --output mutants.json
- run: rust-mutants report --format junit --output mutants.xml
```

The exit code is the gate: `1` means there is a finding. `--json` streams one
object per line while the run happens, `--format junit` and `--format sarif`
feed the views your CI already has, and `--format markdown` appended to
`$GITHUB_STEP_SUMMARY` puts the survivors in front of whoever opened the pull
request.

**There is no threshold flag, and there will not be one.** A percentage is not
a claim anybody can check, and a gate that passes at 80% is a gate that never
says which 20% is missing. The gate is: no finding. A survivor you have
decided about is an expectation with a reason, and one you have not is work.
[ADR 0004](../adr/0004-proof-layers-not-budgets.md) is the argument.

A run reads back what an earlier run of the *same tree* established, so a
re-run of an unchanged commit is nearly free; a changed tree is a cold cache.
`--shard K/N` cuts the catalog by index and `merge` puts the parts back
together, which is how a long run becomes four short ones.

## Where things go

| What | Where |
| --- | --- |
| the run report, catalog, measurement, guard record | `reports/mutation/<run id>/` |
| the recording, with `--trace` | `reports/mutation/<run id>/trace/` |
| a bug report bundle | `reports/mutation/<run id>/diagnostics/` |
| what earlier runs established | the user cache directory; `cache` says where |
| the snapshot and its build cache | the temporary directory; `cache --gc` clears it |

`reports.keep` decides how many run directories survive; the rest are pruned
oldest first.

## Next

[the command line](command-line.md) · [configuration](configuration.md) ·
[operators](operators.md) · [reports](reports.md) ·
[proofs](proofs.md) · [troubleshooting](troubleshooting.md) ·
[upgrading](upgrading.md)
