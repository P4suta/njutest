<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Development

**Status: implemented.** Every gate, task, and tool named here exists; the catalog near the end says which milestone each arrived in.

This document describes the infrastructure for working on njutest and rust-mutants themselves.
The other pages under `docs/` describe the tools;
this one describes the tests, gates, and diagnostics that hold them to their contracts.
Setup, pull request rules, and source conventions live in [CONTRIBUTING.md](../CONTRIBUTING.md).

Developer infrastructure comes first.
Every milestone of the [roadmap](roadmap.md) carries its tests, traces, gates, and diagnostics as completion criteria, and speed never wins over them.

## TDD protocol

Development is test-driven, in three steps of one change:

1. **Red.** Write the test against the behaviour, not the implementation, and watch it fail for the stated reason.
   Paste that output in the pull request.
   A test that passes before the change is not evidence.
2. **Green.** Make it pass with the smallest change that is honest about the contract.
   Fail-closed behaviour is part of the contract, not an error path to add later.
3. **Refactor.** Remove the duplication the change introduced, with the suite green throughout.

Evidence is doubled where it can be: a pure function has unit tests and a property test; a boundary (a subprocess, the filesystem, cargo) has a test against a fixture project; a contract (JSON, a command line, an exit code, a trace) has a golden.
The suite is itself mutation-tested twice — weekly by cargo-mutants, and from M3 by `mise run dogfood` — and a survivor is a test to write or an acceptance to record with a reason, never something to leave.

## Gates

`mise run check` runs every local gate, the ones that answer quickest first.
That is not the order CI runs them in, because CI runs the jobs at once and waits for all of them while a person waits for each in turn: formatting answers in seconds, `lint` — clippy, rustdoc, the repository gates, the fuzz crate's own type check, spelling, TOML, workflows — in tens of them, and the suite in minutes.

It is also what the pre-push hook runs.
The hook refuses a ref whose object is not the checked-out `HEAD`, and an update of an existing remote ref unless its old commit is present locally and is an ancestor of that `HEAD`.
It then asks Git to render the object into a fresh detached worktree and checks the isolated tree again afterwards.
Adjacent edits, ignored local files, and a mistaken non-fast-forward command therefore cannot become inputs to an answer attributed to the commit going out.
A push that goes out has already answered everything CI asks but four things this machine cannot answer: the suite on Linux and Windows, the coverage ratchets, the suite under Miri, and the composite action driven the way another repository drives it.
`xtask/tests/tasks.rs` holds the correspondence — a job added to `ci-success` has to name the local task that answers it first, or say why none can.

The `manual-variant-list` rule refuses a list naming every variant of a set this repository closes, wherever it is written.
A closed fieldless enum derives `njutest_macros::AllVariants`, and its compiler-generated `ALL` changes length and contents with the variants, so adding one cannot leave a ledger silently partial.
The rule reads an array or a comma-separated macro body anywhere in the tree, tests included, and looks through what a list puts around each element: a generator's `Just(...)`, a screen's `.as_str()`, a table's `&`.
None of those changes which set the list is over.

Where the variants carry data, no derive can invent a specimen, and the list stays.
What must stand beside it, in the same body, is a `match` over the same set with every arm named and doing nothing.
That match decides nothing; what it does is make the compiler refuse the function the day somebody adds a variant, which is the only thing that sends them to the one place a specimen has to be written.
A list over a set declared `#[non_exhaustive]` is not asked for, because no match of it can be exhaustive either — and a type that publishes a whole list while saying the list is open is already `open-and-closed`.

Among them, `cargo xtask all` is this repository's own:

| Gate | Refuses |
| --- | --- |
| `lints` | `allow-attribute`: an `#[allow]` anywhere in the repository, tests included. `owned-trait-object`: any owned or ownership-ambiguous type argument containing `dyn Trait`; only a direct borrowed `&dyn Trait` is accepted. Closed implementations use an enum and open ones a generic parameter. `trait-object-alias` and `owned-pointer-alias`: aliases and renamed imports may not hide either side of that ownership from a different source file. `derived-enum-default`: `#[derive(Default)]` or `#[default]` on an enum, including qualified and conditional forms. `semantic-default`: a manual `Default` implementation outside the exact file-and-type allowlist of configuration, UI, and neutral containers; execution, evidence, report, and wire states use named constructors so `..Default::default()` cannot invent a conclusion and a new variant cannot leave old policy compiling silently. `default-derive-alias`: `Default` may not be renamed around those checks. `deserialize-derive-alias`: `Deserialize` may not be renamed or re-exported under a name a different file can use to escape the input-shape rules. `unit-domain-conversion`: `From<()>` may not turn the absence of a value into a domain state; unit aliases, renamed `From`, and macro-token forms are closed at their declaration, while a meaningful state is a named constructor. `glob-import`: a `*` import except from a module explicitly named `prelude`; qualified and grouped imports are inspected by their supplying module. `string-error`: a `Result` that has already flattened its error into `String` or `&str`; variants stay typed until the output boundary. `unit-error`: a `Result` whose error is `()`; a closed error enum keeps the failure exhaustively distinguishable. `string-alias` and `result-alias`: neither half may be hidden from another source file by an alias or renamed import. `discarded-result`: neither `Result::ok`, `filter_map(Result::ok)`, `Result::into_iter`, an error-ignoring fallback closure, nor an iterator `flatten` may turn a failure into absence or an unrelated fallback. Compiler-resolved Clippy policy additionally rejects `Result::ok`, `Result::unwrap_or`, `Result::unwrap_or_default`, `Result::map_or`, `Result::map_or_else`, `Result::or`, and `Result::or_else`; use an exhaustive match so the error policy is explicit and type-checked. `dropped-computation`: `drop` may end one named value's lifetime, but cannot compute and discard a call, method, or expression; renaming `drop` is rejected at the declaration so another file cannot hide the call. `ignored-computation`: neither `let _ = call()` nor an underscore-prefixed binding may silence unused and must-use diagnostics for a call, method, or macro; a RAII value has a semantic name and an explicit `drop`. `open-deserialization`: a map-shaped owned deserializer must deny unknown fields, and current owned report, trace, and wire inputs may not use input-side `flatten`, `other`, `untagged`, `default`, or `alias`. A foreign protocol has one structural exception: an exact private `#[serde(flatten)] external_fields: BTreeMap<String, serde_json::Value>` field in the gate's narrow semantic boundary, which retains additions instead of ignoring them. `direct-json-input`: direct `serde_json` readers and `Deserializer` aliases are forbidden in production, tests, examples, fuzz targets, and macro tokens; every input passes through an exact `strictjson` module that rejects duplicate keys before typed conversion, so a last-key-wins oracle cannot certify different bytes. `opaque-macro-syntax`: a repository macro may not assemble a checked derive, serde option, or `cfg_attr` from tokens the source gate cannot parse; an unknown generated shape is a refusal, not evidence that the prohibited shape is absent. `comment`: a comment that is not documentation, the licence header, or a `rust-mutants:` annotation the engine reads. `unbounded-removal`: a recursive removal inside a loop, which `rust_mutants::reclaim` does with a budget and an account of what is still there. `perishable-handle`: a command or a record built with an identity in it, which the next edit re-mints. `loose-layout`: a layout decided anywhere but in the configuration — a test joining an exported constant that spells a directory structure, or any file spelling a path under a directory a `DEFAULT_*_DIRECTORY` names. The default lives in its own `config.rs`; everybody else asks the type that owns the layout. `hand-painted`: a terminal escape put together anywhere but `rust-mutants::telling`, which is the one module in the workspace that turns a style into bytes. A surface asks for what a thing *is* — `Style::Gap`, `Style::Command` — and never for a colour, because two modules that each named their own drew the same timed-out mutation amber in one place and green in another ([ADR 0023](adr/0023-a-run-may-not-conclude-from-how-it-measured.md)). Reading an escape in order to skip it is not putting one together, so measuring how wide a painted line is stays where it is. `wildcard-over-our-own`: a catch-all arm in a match over a set this repository closes, ratcheted against `xtask/wildcard_allowlist.txt`, which may shrink and never grow. A waiver is named `<file>::<item> over <Enum>, <n> arm(s)` and never by a line number: a number says where a catch-all sits and nothing about what it absorbs, so changing the arm above one moved its waiver onto a different closed set with no diff for anybody to read, and the gate stayed green when that was tried. The three parts change exactly when what is being waived changes, and moving the code, renaming a binding or reformatting a body changes none of them. The arm count is in the name because several arms absorbing one set in one item are one claim and one more is a claim nobody read. A name the tree no longer stands is refused, and the refusal says whether the catch-all went away or changed which set it is over, because a reader does a different thing about each. The gate reads the arms rather than the scrutinee — an arm spelling `Decision::Tests` says what is being matched without anything being resolved — and it collects every enum the workspace declares first, so a foreign one keeps its catch-all: the values of `syn::Expr` are not ours to list, and an arm standing for the rest is the handling rather than a default. `open-and-closed`: a type that publishes `ALL`, `every`, `variants`, a fixed `[Self; N]`, or a total inherent match while also carrying `#[non_exhaustive]`. One file cannot hold both promises. `#[non_exhaustive]` belongs on a truly extensible error or protocol where no exhaustive list is published. `foreign-remainder`: a struct literal of a type another crate owns whose remainder is that type's own `default()`. The fields nobody names are then answered by whoever owns the type, so a field they add next arrives here already decided, in a release that still compiles and in a diff nobody on this side reads; `njutest` took every engine switch that way, and a build a person had configured could have changed under them without a line changing here. Build the whole value once in this crate, naming every field — `njutest`'s `assure::engine::switches` is the shape — and let the call sites take their remainder from that, which makes a new engine field an `error[E0063]` at the one place somebody has to answer it. A remainder from a value this crate computed is not refused, and code that measures rather than ships — `tests/`, `benches/`, `examples/` — is outside the rule, because taking somebody's defaults answers no question wrongly when it asks none of them |
| `devgates` | a seam the ledger `xtask/seam_allowlist.txt` does not name, and a ledger line the tree no longer has: `static mut`, a `static` with interior mutability, `thread_local!`, `#[cfg(test)]` outside a `mod tests`, a read of the process environment or an exit outside `main.rs`, an import of test support from production code ([ADR 0001](adr/0001-seam-policy.md)) |
| `deps` | an internal dependency in the wrong direction ([ADR 0012](adr/0012-one-workspace-two-products.md)); a direct dependency on `anyhow`/`eyre`/`color-eyre`/`miette` that erases typed error variants behind downcasts; or a direct dependency on `async-trait`/`async-recursion`/`typetag`. Those procedural macros can introduce owned trait objects only after the source gate has inspected their input. Cargo metadata reports the canonical package name even when a manifest renames it, so an alias cannot conceal one |
| `fixtures` | a fixture project without a `[workspace]` table, a committed `Cargo.lock`, the SPDX header, or with a dependency that is not a path inside itself |
| `tracked` | a committed path under a directory named `target`, which is build output: the next build writes it again, and `git add -A` picked up 196 files of trybuild's output from a crate-level `target/` that `.gitignore`, anchored at the root, did not name. `.gitignore` now names every `target/`, and the gate refuses a force-added one too. It reads what git tracks, so it runs git with every variable that could point it at another repository removed, and it finds three planted build outputs and passes three lookalikes before its silence about the tree is believed |
| `release-check` | a workspace version that disagrees with the release manifest, or a member that does not inherit it |
| `milestones` | a milestone-shaped reference in the book that has no unique row in the roadmap |
| `surfaces` | a workspace crate that does not declare its Rust visibility as `public`, `incidental`, or `test-support`; an incidental crate absent from the actual `surface-*` binary targets; a harness binary that names no incidental crate; publishable test apparatus |
| `reached` | a public function of a crate whose surface is `incidental` that a test names and nothing shipping does, ratcheted against `xtask/reached_ceiling.txt`. A capability with a test is a capability somebody believed shipped ([ADR 0023](adr/0023-a-run-may-not-conclude-from-how-it-measured.md)): `Interposer::during()` had a test, passed it, and production never called it, so the test was evidence about a function nothing used. Where a crate's surface is an API the rule says nothing, because a function with no caller here is what an API is for; where it is public only because Rust needed it to be, a function only a test reaches is a function nothing reaches. Anything a `cfg` puts behind a test is test support and outside the rule, `cfg(feature = "testkit")` included — reading only `cfg(test)` reported sixteen of those, which was the gate believing its own omission |

The `implicit-scalar-erasure` lint keeps scalar and domain newtypes nominal:
they may not implement or rename `Deref`, `AsRef`, `Borrow`, `Into`, or a representation-side `From` into strings and paths.
Representation boundaries use an explicit `as_str`, `as_path`, or `into_inner`, so generic coercion cannot erase the distinction the type exists to enforce.

The `lossy-text` lint rejects `String::from_utf8_lossy` and `OsStr::to_string_lossy` everywhere, including tests, examples, fuzz targets,
renamed imports, and macro tokens.
Identity, evidence, protocol, path, and test-oracle code decodes exactly and retains failure as a typed result.
A human-only boundary may display a platform path or losslessly escape invalid bytes, but it may not replace two different inputs with the same invented Unicode text.

The `forgotten-value` lint rejects `mem::forget` and `ManuallyDrop`, including renamed imports, macro tokens, and code compiled only under another `cfg`.
Proof harnesses run the same destructors as shipped code: suppressing a destructor only for Kani would establish a theorem about weaker ownership semantics than the program has.
An intentional ownership transfer is a named owner or state transition, not a value silently made immortal.

The `fabricated-overflow` lint applies to report, accounting, identity, cache,
key, offset, and count code.
It rejects saturating arithmetic and numeric `unwrap_or(0)` / `unwrap_or(MAX)` fallbacks, including renamed and macro-hidden forms.
Those boundaries must use checked arithmetic and keep overflow in a typed result; deliberately saturating presentation geometry lives behind a separately named UI helper outside the persisted domain.

The `wrapping-counter` lint rejects `fetch_add`, `fetch_sub`, and `wrapping_*` through direct, qualified, renamed, and macro-token forms.
Atomic counters use `fetch_update` with checked arithmetic and retain exhaustion as typed or sticky state, so release-mode wraparound cannot impersonate an earlier event.

The `unchecked-cast` lint covers platform source that the host compiler cannot type-check.
Integer and pointer `as` casts are refused there, including macro tokens; conversions use `TryFrom` or preserve the exact FFI pointer type so an unrepresentable value retains a failure branch on every target.

The `unowned-spawn`, `unbounded-channel`, and `poison-recovery` lints keep concurrency policy in types rather than cleanup conventions.
A raw thread or child may only be constructed inside the exact owner that joins or reaps it;
queues have a finite capacity and an explicit full/disconnect policy; and a panic-interrupted lock remains a typed sticky failure rather than being reclassified with `PoisonError::into_inner` or `clear_poison`.
Renamed imports,
qualified calls, macro bodies, and code excluded by the host's `cfg` are subject to the same rules.

The `tri-state-bool` lint rejects `Option<bool>` and the aliases or macro constructors that can hide it.
Three semantic states are a closed enum with three named variants, so every match is exhaustive and no caller has to guess whether `None` means unknown, unrecorded, inherited, or not applicable.

`broad-expectation` additionally refuses crate- or module-wide `expect(dead_code)` and `expect(unsafe_code)`.
The expectation belongs on the exact expression it permits: one existing hit must not license every unsafe or unused item added later.

`vacuous-cfg` refuses conditions that the syntax proves always true or always false, including `cfg(any())`, `cfg(not(all()))`, `cfg(all())`, and `cfg(not(any()))`.
The rule evaluates nested `all`, `any`, and `not`, descends through `cfg_attr`, repository macro output, and `cfg!`, and leaves real target,
feature, and test predicates alone.
Dead code cannot be hidden from every compiler, and unconditional code cannot wear a conditional-looking attribute as evidence that another configuration checked it.
Qualifying `cfg!` does not hide it, and importing or re-exporting that built-in macro under another name is refused because a source-only walk cannot soundly resolve the macro namespace across files.

The `lints` walk is fail-closed over its declared source universe.
Every Rust source under `compiler-surfaces/`, `crates/`, `xtask/`, and `fuzz/` must be reached, read, and parsed before the gate can pass.
A repository-wide inventory refuses an `.rs` file outside those four roots; `fixtures/` is the one explicit exception because its Rust is external input corpus, not code this workspace ships or runs as a gate.
Root and fuzz Cargo manifests are preflighted before Cargo reads them.
Cargo's complete target inventory and every recursively local normal, build, development, and target dependency must resolve to exact `.rs` members of the same four-root set, never through a `target/` directory.
A literal `include!` may name only a scanned `support/*.rs`; `#[path]` is limited to the exact compiler-surface composition roots.
Computed and macro-generated redirects are refusals.
A qualified `include!` is still a redirect, and importing or re-exporting it under another name is refused at the declaration instead of trusting a file-local spelling to identify compiler input.

Tests are in that set; they do not make an owned `Box<dyn Trait>`, a string error, an aliased escape hatch, or a discarded iterator error harmless.
A borrowed `&dyn Trait` is allowed because it does not erase the owned implementation set.
The other deliberate boundaries are the ones the catalogue names: a glob from a module explicitly named `prelude`;
documentation, SPDX headers, and engine annotations rather than ordinary comments; and tests/testkits spelling perishable handles, terminal bytes, or bounded cleanup calls because their job is to inspect or clean up the exact thing production is forbidden to hand out or implement itself.

Macro definitions and invocations in repository source are inspected recursively as token trees: an owned trait object and an owned pointer receiving a type metavariable are rejected there, and a pointer alias that a repository macro would emit is rejected at its constructor token before a later invocation can use it with `dyn Trait`.
The workspace procedural-macro inventory is closed to `AllVariants`, `integration`, and `unit`.
Its implementation may neither parse or construct opaque tokens, format identifiers, nor compose separately quoted fragments; the one interpolated `quote!` template is the exact closed `AllVariants` implementation.
This is a structural source proof, not a claim that `syn` observes compiler expansion: substituted or external procedural macro output has no source AST for this gate.
The complete locked dependency all-feature graphs of both the root and fuzz workspaces therefore enumerate every procedural-macro package by canonical name, exact version, and Cargo source in `xtask/proc_macro_inventory.txt`; metadata must agree with its corresponding lock file (whose registry entries retain their checksums) and with that exact list.
An arbitrary new external generator is a gate failure, not a silently enlarged trust boundary.
The implementations of the inventoried external macros remain supply-chain inputs rather than source AST this repository claims to inspect.
Known direct generators of owned trait objects (`async-trait`, `async-recursion`, and `typetag`) are additionally denied by canonical Cargo package name, including renamed dependencies.
Renaming or type-aliasing a token primitive, or renaming `format_ident!`, is also refused in the workspace procedural-macro implementation; changing the local name cannot turn opaque identifier synthesis back into inspected Rust.

The workspace also denies Clippy's `unwrap_used`, `expect_used`, `panic`,
`todo`, `unimplemented`, and `unreachable`.
A test or testkit may use `#[expect(clippy::expect_used, reason = "…")]` or the corresponding panic expectation when the panic is the test's failure report or a failed setup precondition.
That is a local, compiler-checked expectation, not a blanket test exemption: a reason is visible at the use site, an unfulfilled expectation fails under denied warnings, and `#[allow]` is one of the catalogue's shapes the repository gate refuses everywhere.

The gates are also tests (`xtask/tests/gates.rs`), so `cargo test` refuses the same things.

`compiler-surfaces` gives `incidental` a compiler-enforced meaning.
It compiles each CLI library again as a private module together with the real binary composition roots.
Workspace `unused = "deny"` then makes a public-looking function with no production caller a type-check error; tests cannot keep it alive in this view.
Public libraries are deliberately not put through that view because callers outside this repository are their production callers.
The `surfaces` gate derives the private-view set from Cargo's actual `compiler-surfaces` binary targets and proves it equals the incidental crates,
so neither adding a crate nor deleting a harness while leaving a ledger entry can silently escape the rule.

`cargo xtask proofaudit <run-directory> [--trace <recording>]` stands apart from `all`, because it is about one completed run rather than about the tree.
It reads that run's `njutest-assurance-report-v1.json` and decides again, with code that never calls the runner's, whether each verdict is the one the recorded evidence supports: whether the columns say what the records they summarise say and add up the way the [assurance contract](assurance-contract.md) states, whether every kill names a target this run itself saw pass on the original tree,
whether the mutations nothing noticed and the `surviving-mutant` findings are the same set, and whether every disposition read back from an earlier run names one a reader could go and read.
Given the run's recording as well, it holds the proof layers to what the run wrote down: no target a proof removed from what could notice a mutation may then be the target that killed it, a route that says no measured target reaches a mutation may not then run one against it, a route the measurement widened has to run something, and a route may not say both that it read an answer back and that it refused one.
The reach layer is re-derived rather than confirmed, because a route names the targets it removed every execution from: each of those names is held to the targets the run reports, to the targets the same route kept, and to the proofs that route names — and a route that removed every execution while naming nobody is a violation, since nothing reaches a place only if somebody was in a position to notice and did not.
It reads the recording as lines of JSON rather than through the code that wrote them, and a run recorded without `--trace` leaves the layers `unaudited` rather than passed.
A complete report holds its facts per configured build and per catalog part; the audit re-decides the one part of a report that measured one build whole, and refuses a report of several builds or of shards with exit code 2 rather than reading one of them as the whole.
Such a report states no verdict — the verdict is derived from its records — so the verdict is `unaudited` there, since one this audit derived would be a verdict it agreed with by construction.
This is [ADR 0004](adr/0004-proof-layers-not-budgets.md) decision 5,
which ships a proof layer only against a re-implementation that is not asked whether it agrees with itself.

The `wire` layer is the same rule for the seams.
It mints the fault catalogue again from the exchanges the recording holds — by the rules and the identity recipe written out in `xtask/src/wire.rs`, which never calls the runner's — and holds it to what the run says became of each question: a question the exchanges license and the recording has nobody putting, a question the run put that no exchange licenses, a question nothing noticed that the report does not name,
and a finding the report carries that no run put.
The identity recipe is pinned as a literal digest in `xtask/tests/wire.rs` and again in `crates/njutest/tests/derive.rs`, so the two implementations agreeing is evidence rather than two copies of one mistake.

The `drift` layer is the same rule for the measurement routing rests on ([ADR 0025](adr/0025-a-reach-that-moves-is-not-a-measurement.md)).
It reads the engine recording under `builds/*/engine/` beside the runner's, and re-derives from its `touch` records alone which targets reached something on an original-code control that they did not reach on their baseline, over the same passing tests.
Each measured target is held to the report's drift record about it (`held`, `moved`, or `not-measured`), a moved one to an `unstable-baseline` finding and every such finding to a moved target, and a target no comparable control recorded to the `drift-not-measured` limitation.
Its planted defect is a control that reached a site its baseline never did, recorded as `held`.

`cargo xtask engine-audit <run-directory> [--trace <recording>] [--shard <report>…] [--ledger .rust-mutants.toml]` is the same rule for the engine's own runs.
It reads that run's `run-report-v1.json` and re-decides it in fourteen layers, none of which calls the engine's code; among them:

| Layer | Re-derives |
| --- | --- |
| `identity` | every `id`, minted again from the row's own path, rule, version, byte span, source digest, and edit; the short form as the head of the full one; the indices of the accepted and the refused as one dense catalog |
| `accounting` | every column from the rows, and the equations the columns stand in: the outcomes come to `executed`, `executed + not_run` comes to `cataloged`, and `unreached` is never larger than `not_run` |
| `score` | present exactly when the run decided something, over the columns it is a ratio of |
| `findings` | each kind as a set equality with the rows in both directions, and every finding as one that names a row |
| `expectations` | `met`, `stale`, and `unmatched` against the rows they name, and each accepted row against the one claim that accounted for it |
| `exit` | the code the run returned, from what it found |
| `merge` | the parts of one catalog: every `K/N` exactly once, same digests/scope/tool versions, disjoint indices, and the whole they come to |
| `proofs` | every discharge against the measurement and the catalog the run kept: a target that covered the body it was discharged from, a discharge whose premises are missing, a discharged pair that then ran, the `discharged` column, and a mutant that never ran and whose reason the recording does not give |
| `trace` | every row against the recording of what actually ran: the target it names ran, its outcome is that execution's, a believed timeout repeated and a step-limit stop carried the notice that established it, instrumenting moved no line, every refusal was condemned by a round, a discharged target did not then run, an unreached route ran nothing, and every target the build produced was verified |
| `ledger` | every survivor as one the ledger accepts with a reason, and every acceptance as one the run still holds |
| `entry` | every site a test reached and every mutation a test noticed as lying in an item that test entered, by the item catalog and `entered` of `touched-v1.json`; every mutation as sitting in a measurable item whose name is the row's `item` |

Its output and exit codes are `proofaudit`'s: one line per remark, a summary line, and 0, 1, or 2.
Before it reads the run, it re-decides a clean synthetic run (`xtask::engineaudit::sentinel::clean`), in which no layer may find anything, and every defect `Layer::planted` holds for each layer, each of which that layer must report; a layer that fires on the clean run or misses a defect planted for it stops the gate with exit code 2 and `the <layer> layer is blind`, before the run is read.
Three runs of the fixtures are committed under `xtask/tests/testdata/engine-run-*/` and a test re-decides all three, so a change that makes the engine disagree with itself fails here rather than in a weekly job.

`mise run dogfood:audit` and `mise run dogfood:engine:audit` are those rules as one command each: they run this workspace through the release build, keep the recording, and re-decide it.

```console
$ mise run dogfood:audit
proofaudit: 10 planted defects found first, each by the layer it was planted for, and the clean specimen drew none
proofaudit: 20260906T052111Z-047fc6: 39 mutants and 16 targets re-decided; 0 violations, 1 unaudited
```

Where the recording does not carry enough to decide something again — which survivors a reviewer accepted, what a reused disposition was routed under,
which regions a route was decided from —
the gate says `unaudited` and counts it apart from the violations, because fail-closed is never turning "I cannot check this" into "this is fine", and equally never into "this is broken".
One line per remark names its layer and its subject, a summary line closes the report, and the exit code is 0 with no violations, 1 with them, and 2 when the run directory could not be read at all.
Before it reads the run, `proofaudit` re-decides a clean synthetic run (`xtask::proofaudit::sentinel::clean`), in which no layer may find anything, and every defect `Layer::planted` holds for each of its ten layers, each of which that layer must report; a layer that fires on the clean run or misses a defect planted for it stops the gate with exit code 2 and `the <layer> layer is blind`, before the run is read.

### A gate finds what was planted for it before it is believed

A gate that cannot see a shape reports that the shape is absent, and that report is green.
`wildcard-over-our-own` passed for as long as syn 3 put a match arm's guard inside its pattern, because the reader returned nothing for a guarded arm and a gate that finds nothing passes; the pre-push gate refused every push while its own eight tests, each feeding a newline-terminated input, stayed green.
Neither was a missing gate.
Each was a gate that was evidence only about what it could see.

So `lints`, `devgates`, `engine-audit` and `proofaudit` each start with a positive control.
Every kind a gate says it detects has a planted example, reached through a total match over the kind (`Kind::planted`, `SeamKind::planted`, and each audit's `Layer::planted`), so a kind added without one does not compile.
For the two scans the examples are files under `xtask/sentinels/`; for the two audits each is a defect laid over a clean synthetic run, which no layer may find anything in.
The gate reads each planted example with the code that reads the real tree or the real run, and a kind not found in one of its examples stops the gate with `the <kind> check is blind`, or `the <layer> layer is blind` and exit code 2 for an audit: nothing it would have said is believed until the example is found again.
The planted files are text, not `.rs`, so they are never a file of this tree, a finding of it, or a region of its coverage.

A planted file holds one or more shapes, each one form of the kind:

```text
=== guarded-arm tree
--- crates/app/src/lib.rs
<the file>
=== by-full-path source crates/app/src/lib.rs
<the file>
```

A `source` shape is one file read by the per-file scan under the path it names, because some rules switch on the path.
A `tree` shape is laid over the smallest repository the gates accept (`xtask::sentinel::skeleton`) and read by the whole gate, which is how the rules that look across files see it.
Each shape is read on its own, so one cannot make another's finding appear.
When a rule learns a new form, the form gets a shape of its own beside the test that taught it.
A shape proves that its form is found, not which reader found it.
With the guard reader removed, a guarded arm over a typed parameter was still found, because the parameter's type named the set; only a scrutinee with no typed binding leaves the guarded arm as the one thing naming the set, and that is the shape that went red.
So a shape is written against the reader it exists for, and checked by removing that reader and watching the gate refuse.

## Test harness

`crates/njutest-devkit` is test-only support shared by every crate: the golden-file comparison, the workspace and fixture paths, the `cargo` that built the test binary, a throwaway copy of a fixture project, and the scripted toolchain.
Each crate's `testkit` module (behind `cfg(test)` or the `testkit` feature) holds its own fakes; production code may import neither, and `devgates` checks.

### Two halves of the suite

A suite that starts a real `cargo` is named `toolchain_*.rs`, and every other one is not.
`mise run test:fast` is the inner loop and runs the second half in seconds; `mise run test:slow` runs the first; `mise run test` runs both and the doctests, which is what the pipeline runs.
`cargo xtask test`'s own suite (`xtask/tests/tasks.rs`) holds the naming to the rule, so a test that quietly starts a toolchain cannot land in the inner loop.

### The platform this machine is not

The pipeline runs the suite on Linux, macOS, and Windows, and most of what the other two answer differently needs their machine to find out.
Code behind `#[cfg(windows)]` does not: this machine's compiler never reads it, so a workspace that builds here can fail to build there on a lint nobody could have seen.
`mise run check:windows` asks for the reading without the machine.
It needs the target's standard library once:

```console
$ rustup target add x86_64-pc-windows-msvc
$ mise run check:windows
```

It is not part of `mise run check`, because a machine without that target installed would fail a gate for the want of a download rather than for anything about the change.

### Which half a rule goes in

The split is about speed, and it decides something else as well: **whether a mutation run can see that a rule is held.** The guards record which test reached which mutation in the process they run in, and a test that starts the binary in another process leaves no record there.
So a mutation of the runner's own code is never routed to a `toolchain_*` target, and an assertion that lives only there is one the measurement cannot attribute to anything.
The rule is held; the run reports a survivor.

That happened here on 2026-09-09 to `assure/run.rs`'s first stage.
A test in `toolchain_verify.rs` asserted that a verification names each stage as it starts, breaking the rule failed that test, and the mutation survived the run anyway, because the route named `toolchain_watch` — the one test that drives the runner in this process.
Moving the assertion there, unchanged, killed it.

So: **a rule about what this code does goes in an in-process test**, and a rule about what a person typing a command gets — the exit code, which stream a line went to, the files left behind — goes in a `toolchain_*` one, where it cannot be observed any other way.
When a survivor's route names only `toolchain_*` targets, the question to ask first is not "what test is missing" but "is the test somewhere the measurement can see".

A whole run in this process is `njutest::run_from` with an `Environment` the test builds, and it reaches every phase the configuration turns on.
That is what `toolchain_watch.rs` does, and it is why a `.njutest.toml` written into a copied fixture is the lever for a whole cluster of survivors rather than one:
`[execution] timeout` with a fixture that is slow once reaches the bound that expires and the quiet measurement after it, `[mutation] equivalence` reaches the layer that removes findings, `[resources]` and `[generation]` reach the providers, and a `fuzz/fuzz_targets` directory reaches the gap a run states about targets nobody asked it to drive.
Each of those was measured as unreached until a run in this process was configured into it.

A suite already written against a spawned process does not have to be rewritten to move here.
`njutest_devkit::process::answered(code, out, err)` hands back a `std::process::Output` for a command driven through the entry point, so a file changes in one function and every assertion in it stays as it was.
Both products' command suites were moved that way on 2026-09-11; what still starts a process is what is about one — an interrupt, a hang, a panic, a stream read while it is being written, the language server's stdio, and the three suites whose subject is a variable a process inherits.
That last kind must stay.
An instrumented child keeps a valid outer identity; a refusal test removes its activation and catalog and adds an incomplete touch mode so the composition root, rather than a stale generated runtime, answers the question.

### The scripted toolchain

`njutest_devkit::fake_cargo` writes a script of invocations —
program, argument prefix, environment, and what to print, write, wait, and exit with — and `crates/rust-mutants/examples/fake_cargo.rs` is the program that answers it as `cargo`, as `rustc`, as a coverage tool, or as a test binary.
It is an example rather than a binary of the devkit because `--all-targets`, `cargo nextest`, and `cargo llvm-cov` build the examples of a crate under test on every platform and build no binary of a dev-dependency on any of them.
A command no entry matches exits 99 with its own command line on stderr: a suite that forgot to script something says what it forgot.

That is what lets `crates/rust-mutants/tests/workspace.rs` hold the engine to every refusal it can report about a toolchain — a cargo that is not a file, a banner with no release line, metadata that is not metadata, a stream whose second line is not a message, a build that outruns its timeout — in a fifth of a second and with no toolchain at all.

### A copy of a fixture

`njutest_devkit::fixture::Fixture::copy` is the tree a suite hands to the thing it is testing.
The copy is canonical, because a path a run reports has to compare equal to the one the test holds; its temporary and cache directories sit beside the tree, because a cache under the root would change the tree's own digest every time a run wrote to it; and `copy_with_siblings` puts a second fixture next to the first, which is what a path dependency that climbs out of the tree needs.

### Line endings

`.gitattributes` says `* -text`, so a checkout is byte-exact everywhere: an identity hashes the exact bytes of the file it was cut from, and a CRLF checkout would change every source digest and therefore every mutant ID.
The CRLF variants the tests use are **derived** from the LF inputs by `rust_mutants::testkit::source::crlf` rather than committed.
A committed copy is a second spelling of the same program that a checkout, an editor, or a careless rewrite can quietly change, and then the test proves the two copies agree rather than that the engine handles both endings.
What the tests hold the engine to: the same candidates at the same lines and columns, the same line count after instrumenting, the same rewrite, the same fates, and identities of its own.

### Golden files

`golden(path, got)` compares recorded bytes against a file, byte for byte,
and reports one failure that names the file and shows a unified diff.
Without `UPDATE_GOLDEN=1` the comparison is read-only, and a missing file is a failure rather than a silent first recording.
`TRYBUILD=overwrite` is the same switch for the compile-error goldens of the attribute macros.

### Fixture projects

`fixtures/` holds independent cargo projects the suites drive with a real `cargo`, offline.
Each states its purpose in a `README.md` and, where it exists to have a known fate under mutation, a table of every mutant and its expected outcome.
See [fixtures/README.md](../fixtures/README.md).

### Fuzz targets

`fuzz/` is a standalone cargo-fuzz crate (nightly, sanitizer) with one target per fail-closed parser or byte transformation of the engine; each target states one property in its doc comment and `fuzz/README.md` lists them.
`mise run fuzz:smoke` runs every target briefly, which is what somebody changing a parser does before pushing; the `fuzz` workflow spends twenty-five minutes a target, weekly and on request, and never on a pull request.
Five thousand executions searches nothing a parser is afraid of, and the workflow does not gate `ci-success`, so a crash found there could not have stopped a merge in any case.
A crash reproducer worth keeping becomes a regular test.
`xtask/tests/fuzz_ledger.rs` keeps the four places that name the targets in step: the source files, the manifest stanzas (each with `bench = false`, so `cargo bench` never builds a sanitizer target), the README rows, and the weekly workflow's matrix.

Being standalone is what makes them cheap to run and easy to lose: nothing in `cargo test --workspace` compiles them, so a target can rot against an API change and say nothing until the weekly job.
`mise run fuzz:clippy` — part of `mise run lint`, and a step of the CI lint job — puts the crate through the root Clippy policy on the toolchain the workspace uses, which compiles it and is the cheapest thing that would have noticed.

### The documentation

`docs/` is an mdbook: `mise run book` builds it into `target/book`, and `mise run book:serve` reloads it as the pages change.
Nothing is written for the book — the pages are the ones this repository already keeps, and `docs/SUMMARY.md` is the order to read them in.

Two gates hold the summary and the pages to each other, one in each direction.
mdbook is configured with `create-missing = false`, so a summary that names a page nobody holds fails the build; `xtask/tests/docs.rs` refuses a page the summary does not name.
Without the second one a new page is simply absent from the book, which nobody notices, because a book that is missing a chapter looks exactly like a book that never had it.

### Coverage

The `coverage` job in `ci.yml` runs the suite once under `cargo llvm-cov` and then ratchets four numbers: the workspace at 80% of regions, `rust-mutants` at 87%, `rust-mutants-cli` at 85%, and `xtask` at 80%. Each floor is a little under what the suite reaches, so an ordinary change has room and a change that drops a whole area does not.
**A floor is raised when a suite earns it and never lowered**; lowering one is a decision to argue for in the pull request that does it.

The job builds the examples inside the coverage environment before running the suite.
`nextest` builds the test targets and nothing else, and part of this suite drives a scripted `cargo` that lives in an example, so without that step every suite that uses it fails for want of a binary rather than for a reason.
The same is true locally: `cargo llvm-cov nextest` needs `cargo llvm-cov show-env` and a `cargo build --examples -p rust-mutants` in between.

### Ledgers the documentation keeps

A page that names a set the code also names goes stale silently, so each such pair is a test: `crates/rust-mutants/tests/docs_ledger.rs` holds the trace page to `trace::EVERY_TYPE`, the architecture page's skips to `SkipReason::ALL`, the operators page to `CANONICAL_TABLE` and its two counts,
and the limitations page to `rust_mutants::limitation::ALL`, and refuses a page under `docs/` that does not say whether what it describes is implemented.
`crates/rust-mutants-cli/tests/docs_ledger.rs` holds the configuration page to the keys the reader accepts in both directions, and the JSON page to `FindingKind::ALL` and the exit codes.
`xtask/tests/docs.rs` holds `docs/ci.md` to the workflows and the jobs they hold.

### Error codes

Every error variant carries a code; `docs/errors.md` is the ledger, and a test in each crate keeps the two equal in both directions.

## Diagnostics

Everything a run does is recordable: the runner's current [trace v1](trace-v1.md) (with [trace v1](trace-v1.md) retained as a historical contract), the engine's current [trace v1](engine/trace.md), `--keep-temp`, the diagnostics bundle of a failed run, and the `explain` family of commands.
The rule for all of it is [ADR 0002](adr/0002-trace-is-not-evidence.md): never a claim, never a failure, always honest about what was dropped.

Both products bundle a run the same way.
`rust-mutants doctor` says what the engine would find in this environment, and `rust-mutants diagnostics` gathers one run — report, catalog, measurement, probe logs, recording, configuration,
doctor document, toolchain — into one directory to attach to an issue, with the names of the variables that were set and none of their values.
What to reach for when something is wrong is [engine troubleshooting](engine/troubleshooting.md).

`njutest trace summary` is where a person asks where a run went.
It counts the events by type, times every stage the run said it had reached, counts the commands by program, says how many executions each proof removed, and names the slowest commands.
A run records the engine's own trace in a directory beside its own, and the summary reads that too: the engine does most of a run — the snapshot, the instrumentation, the validation rounds, the builds — so a summary that read only the runner's would leave the larger part of every run unaccounted for.
Both are read with one command:

```console
njutest verify --trace
njutest trace summary
```

The numbers are the ones to optimise against, and the rule for acting on them is [ADR 0004](adr/0004-proof-layers-not-budgets.md): a run that is too slow is a run missing a proof, or doing work nothing reads — never a run that should measure less.

## The catalog

The developer-facing infrastructure, and the milestone it arrives in:

| Means | For | Arrives |
| --- | --- | --- |
| devkit (golden, paths), error-code ledger, `cargo xtask` gates, `bacon`, `mise run doctor`, `CONTRIBUTING.md` | the inner loop and the ratchets | M0 |
| engine trace (every discovery decision, every validation round), goldens with CRLF variants, property tests, fuzz targets for every fail-closed parser, fixtures with fate tables, `rust-mutants explain` / `instrument --file` / `why-skipped`, runner contract tests, external-consumer contract test | seeing why the engine did what it did | M1 |
| runner trace v1 with `trace summary` and `trace diff`, diagnostics bundle, `--keep-temp` ledger, testkit (fixture repository builder, scripted workspace, `normalize_report`, helper subprocesses), report and help goldens, `xtask report-diff`, `njutest plan --why` | seeing why a run routed what it routed | M2 |
| scripted session, route events, `njutest explain`, accounting property tests, `mise run dogfood` | the runner on itself | M3 |
| evidence-store goldens, interruption injection, concurrent cache tests | reuse and resumption | M4 |
| `xtask proofaudit` (independent reimplementation of every proof layer), fixtures for probes and branch proofs, the kill-implies-infection soundness test | proofs before they ship | M5 |
| provider fakes with failure injection, repair rollback tests | providers and repairs | M6 |
| the nightly fuzz job | `deep-v1` | M7 |
| release consistency, install-surface job, release checklist | shipping | M8 |

## Benchmarks

`mise run bench` measures what the byte foundation, the pipeline, and the report cost: `splice`, `flatten`, and a mutant identity in the engine's `foundation` bench; discovery, instrumentation, cataloging, reading a coverage export, reading the cargo configuration of a ten-deep tree, walking a match of five hundred arms, and reading a file of `rust-mutants: skip` markers in its `pipeline` bench; the audit and the two projections in the runner.

They are observations, never gates.
Nothing fails when a number moves and no verdict depends on one — they exist so a person can answer "did that change make discovery slower" by looking rather than guessing, which is the same thing [ADR 0004](adr/0004-proof-layers-not-budgets.md) asks of a proof layer.
Measured on one machine, for scale rather than for comparison:
a 200-edit splice about 7 µs, flattening one function about 13 µs, one identity about 1.7 µs, auditing a 2000-target report about 3 µs, and writing that report about 450 µs as JSON and 650 µs as records.
Of the pipeline:
discovering a 2000-function file about 260 ms, instrumenting a 200-function one about 18 ms, cataloging its candidates about 32 ms, and reading a coverage export of 500 functions about 2.4 ms.

The harness is criterion with `harness = false` and a hand-written `main`:
`criterion_group!` generates an undocumented public function, and this workspace documents everything.
