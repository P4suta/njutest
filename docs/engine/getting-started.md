<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
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

## Reading the summary

```
MUTANTS   cataloged=120 refused=3 skipped=17 executed=117
OUTCOMES  killed=98 survived=14 timed_out=1 inconclusive=0 errored=0 not_run=4 unreached=4 …
SCORE     87.6%  (99 detected of 113 decided)
```

- **killed** — a test failed with the mutation live. The tests noticed.
- **survived** — every test passed. Nobody noticed.
- **timed out** — it never finished, twice. Counted as noticed.
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
$ rust-mutants explain e5e872bfbcb2
$ rust-mutants replay e5e872bfbcb2
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
