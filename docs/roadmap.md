<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Roadmap

**Status: M0 to M11, M13, M14, K1, and E2 to E12, are done.** What remains of M8 is the tag itself, which is a decision rather than a change: see `docs/release.md`.
The user's decisions: one workspace, the engine first,
the engine a standalone product too, every milestone completed, test-driven throughout, developer infrastructure first.

Each milestone has two halves — the feature and the developer infrastructure that lets it be seen, tested, and audited — and both are completion criteria.

| # | Milestone | Feature | Developer infrastructure | Done when |
| --- | --- | --- | --- | --- |
| M0 ✓ | Scaffold | workspace, lints, gates, CI, contracts, both command-line skeletons, the public API crate | devkit, error-code ledger, `xtask` gates from tests, `bacon`, `doctor`, `CONTRIBUTING.md` | `mise run check` green |
| M1 ✓ | rust-mutants engine | stable IDs, byte splicing, snapshot and owners, process supervision, syntactic discovery, guards and runtime, compiler-validated acceptance, execution, the library API, `list`/`catalog`/`run` | engine trace, goldens, property tests, fuzz targets, fixtures, explain commands, contract tests | every fixture's fates are fixed by tests |
| M2 ✓ | `njutest verify` baseline | configuration, targets, per-target baseline under coverage, regions, report v1, verdicts, `plan`/`doctor`/`init` | trace v1, diagnostics, testkit, `report-diff`, `trace summary`/`diff`, fuzz targets, benchmarks | a fixture yields a report with regions and a trace |
| M3 ✓ | Mutation phase | routing, paired confirmation, accounting, acceptances, `explain`/`replay`/`accept`, HTML/SARIF/JUnit | scripted session, route events, dogfood | njutest reaches a verdict on itself |
| E2 ✓ | rust-mutants standalone | `.rust-mutants.toml`, run report v1, exit policy, timeout retry, score, expectations | engine dogfood, report goldens, interrupt contract | rust-mutants scores itself |
| M4 ✓ | Identity, cache, evidence | digests, behaviour keys, exact cache, checkpoint, evidence reuse, `--changed`, unsafe inventory, build-cache gc | interruption injection, reuse goldens | a second run reuses the first |
| M5 ✓ | Proofs | probe tree, infection log, witness tree, branch proof, discharges, dashboard | `proofaudit` with zero violations on a dogfood recording | kill implies infection on `fixture-probeable` |
| E3 ✓ | Engine incremental | outcome cache, `--changed`, sharding and merge, coverage-guided selection | determinism and concurrency tests | a sharded run equals a whole one |
| M6 ✓ | Resources and repair | providers, candidates, `fix --apply`, retention | provider fakes, rollback tests | the goatest provider suite passes here |
| M7 ✓ | `deep-v1` and fuzz | Miri, sanitizers, cargo-fuzz, corpus promotion | nightly jobs, fuzz fixture | weak test → survivor → fuzz → corpus → fresh kill |
| E4 ✓ | Engine reports | Stryker projection, offline HTML, TUI, `doctor-v1` | schema validation, TUI snapshots | a Stryker-valid report |
| M8 ✓ | Release | release workflow, SBOM, provenance, comparison document | release gates, install-surface job | v0.1.0 |
| M9 ✓ | The contracts and the code, said the same way | a timeout is a finding, an acceptance answers only for a mutation nothing noticed, the infection proof fires, a scoped run builds its own packages, mutations are measured `[execution] jobs` at a time, `replay` puts one finding back to the tests, a mutation nothing reached is unreached only where the evidence says so, a library's documentation is a target | typed `route`/`mutant-exec`/`probe-exec` records, every stage timed, `trace summary` naming the slowest commands and reading the engine's recording, `proofaudit` holding the layers to the kills | every page describes what the code does, and `proofaudit --trace` re-decides a real recording with no violations |
| M10 ✓ | Equivalent mutants, proved | `[mutation] equivalence`: the compiler renders a mutation identically or it does not, and a run says `equivalent` only where the tests ran the position | `rust-mutants equivalence` over a whole catalog, `fixture-equivalent`, ADR 0013 | a mutation nothing could notice is not a finding, and one in code nothing calls still is |
| M11 ✓ | The Rust-shaped gaps | fifty-one operators, mutation inside the assertion macros, the files `include!` pastes in, `#![no_std]` crates, a proc-macro crate's own tests, mutations routed to a library's documentation | six fixtures with fate tables, the rule-order guard, the skip reasons that are now emitted rather than named, a target cargo runs rather than the engine | every limitation the docs list is one a report carries |
| M14 ✓ | Evidence between machines | `--shard K/N`, `njutest merge`, cache collection and expiry, and `--export`/`--import` move answers without moving an unverified build | imported records pass through the same `Store::put` validation as local ones, with merge and cache contract tests | a matrix can reuse only answers the receiving machine would itself retain |
| K1 ✓ | Model-checked survivors | `verified-v1` asks a closed, side-effect-free Rust fragment whether any input distinguishes a survivor from its original | generated differential harnesses, typed Kani results, parser fuzzing, a scripted checker, real-checker CI and an independent proof audit | every eligible survivor is proved, distinguished, or carries one typed reason no answer was established |
| E5 ✓ | The engine sees itself | `--trace[=DIR]` on every command, `trace summary`/`check`/`diff`, typed `verify`/`probe-exec`/`witness`/`route` records, sub-phases through `prepare`, `Session::route` as a question anyone can ask, and the byte span and source digest a reader re-mints an identity from | a scripted toolchain the tests drive instead of cargo, the suite cut into an inner loop that starts nothing and a `toolchain_` half that does, `cargo xtask engine-audit` re-deciding a run in nine layers, three committed runs it re-decides, the dogfood ledger and its weekly shard job, and one test per ledger the documentation keeps | every judged mutant leaves one route record, `engine-audit --trace` re-decides three committed runs with no violations, and `mise run test:fast` starts no cargo |
| E6 ✓ | The contracts and the code (the engine) | a run compiles what it is told to (`[build]`, features, target, profile), it says which test noticed a mutation and what signal a process died from, a tree that reaches outside itself or does not link is refused before any round, a build script's generated code is skipped by name, a `harness = false` target answers by exiting, documented examples are a target a run can switch off, `build.rustflags` is read and put back rather than refusing the measurement, a crate that forbids what the guards allow is skipped whole, a cancelled build is a cancellation, attribution reads every span, bisection names what it isolated and what interacts, the budget is derived from what the target measured, four mutants are measured at once and delivered as they finish, coverage is on by default, a branch or a probe discharges a target that could not have noticed, and a survivor is asked whether the compiler renders it at all | `cargo::config` and `cargo::manifest` read what cargo does not report, `Session::judge`/`describe`/`source`, the driver and the report model in the engine, `Observer` and the worker pool, `prove::discharges` as a pure function, the evidence a run keeps for its own audit, the `proofs` layer that re-derives every discharge, `cache`/`select`/`identical`/`evidence` records, and eight fixtures for eight things nothing measured | `run --jobs 4 --probe` completes on the engine itself and `engine-audit --trace --ledger` re-decides it with no violations |
| E7 ✓ | Every place says what it is for | eighteen more operators and three more families — a match arm asked what it is for, a jump swapped, a terminal `else` dropped, a literal moved by one — a return type the syntax cannot default stated rather than guessed, every branch of a returned `if` or `match` a return site of its own, and `rust-mutants: skip` and `[[mutation.skip]]` as the two ways an author says what to pass over and why | `Form::M`, the guard written where an arm had none, the census that refuses a place the walk passed over silently, the item path every candidate now carries, locator-form expectations that outlive an edit elsewhere in the file, the `skip-claim` record, the `unmatched-skip` finding, the `sites` audit layer, and `fixture-annotated` | every place a rule targets is a mutant or a skip with a reason, and a claim that hides nothing is a finding rather than a comment nobody notices |
| E8 ✓ | The engine as a product | a run says what it is doing while it does it and streams `rust-mutants-run-stream-v1` for a program, `--fail-fast` and seven filters narrow it after the catalog, `--dry-run` prices it, `explain` and `replay` answer from what was stored, `doctor` grades thirteen checks and `diagnostics` gathers one run into one directory, `cache`/`merge --runs`/`run --run-id` name what a sharded run leaves, `rules` lists what a team can pin, and a stored run projects into markdown, JUnit, SARIF, a page that shows every survivor in place, and Stryker | the run-stream, explain, doctor, reached and diagnostics schemas, the two-way test between the command-line page and `--help`, the subcommand help golden, the README session recorded from the binary, the terminal browser's five recorded frames, and the ledger that keeps `schema/` and its page equal in both directions | a bug report is one command, every flag is on one page, and the README shows what the tool actually prints |
| E9 ✓ | Answering without running | a run is counted in pairs of one mutant and one target rather than in seconds, what removed each pair is named and labelled a proof, a sufficient answer, a remembered one or a narrower question, and a remembered answer is keyed on the sources the build compiled, the manifests that chose its dependencies and the toolchain that compiled it rather than on the digest of a whole tree | `rust_mutants::work::Work` derived from the stored report alone, the `work` audit layer that holds it to the recording, `xtask/work_ceiling.txt` as a ratchet that may shrink and never grow, and the differential harness that runs four fixtures with every layer on and every layer off and holds the two answers to each other | the engine does less for the same question every release, and a test says so |
| E10 ✓ | The guards are the measurement | reach is recorded by the guards on the run that verifies the baseline, so a mutation goes to the tests that reached it and no coverage build is made | `touched-v1.json` and the audit layer that re-decides from it, `fixture-order-dependent`, `fixture-threaded`, the differential harness over every combination of the two measurements, ADR 0014 | the answer is the one a run with nothing removed gives, and the tests started fall from 46 to 18 on `fixture-coverage` |
| E11 ✓ | The difference that never was | a guard whose condition the compiler vouched for evaluates both of its branches on the baseline and records where they parted, so `never-infected` is a default layer with no tree, no build and no run of its own | `narrowing` in `touched-v1.json` and the audit layer that re-derives both kinds of discharge from it, the instrumenter reporting which guards it actually wrote the call into, ADR 0015 | eight fixtures stop starting a process for a mutation nothing could have noticed, and the answer is still the one a run with nothing removed gives |
| M13 ✓ | Where the code is being written | `njutest watch` verifies again whenever the tree changes and says what moved, `njutest lsp` shows a completed run's findings in the editor and hands back the acceptance to record without applying it, `cargo njutest` answers to the name cargo looks for, and `.github/actions/njutest` runs a verification in somebody else's repository and hands the findings to code scanning | the loop driven by a look and a round the test supplies, so every rule of it is asserted rather than waited on, the protocol exchange recorded as a golden, the action exercised against a fixture with this workspace's own build, and `docs/` built as a book with the summary and the pages held to each other in both directions | a save is a verdict, a survivor is a diagnostic where the mutation is, and every page of the documentation is one the book reaches |
| E12 ✓ | The tree nobody needed | a return replacement writes a constant, so the guard compares the value the branch that keeps the original produced against it, and the probe tree — a check loop, a build of the workspace and a run of every target — goes | `instrument::observable` as one sealed trait rendered into both generated modules, the probe question put to the witness tree in the shape the guard will hold, `Marker::keyed_to` so a build cache nobody can look up again is collected, ADR 0016 | `prepare` falls from 575s to 427s cold on this engine's own workspace, three more fixture survivors are discharged without an execution, and the differential harness still holds every answer to a run with nothing removed |

## What is left, and what is not there to be had

[ADR 0004](adr/0004-proof-layers-not-budgets.md) names three ways a mutant survives and calls propagation — the state differed and the difference never reached an assertion — the next layer to build.
After E11 the honest reading is that the engine already has the only propagation proof it can make, and that the rest of it is not a layer this engine can add.

Reach and infection are both answerable from **one run of the unmutated program**, which is why they are cheap: the guards record what the tests reached, and where the compiler vouches that a site is inert they record whether its two branches ever parted.
Propagation is not like that.
Whether a difference reaches an assertion depends on everything between the site and the assertion, and there are two ways to know it: read the whole program, or run the mutated one.
Reading it means a type-and-dataflow engine —
[ADR 0008](adr/0008-compiler-validated-acceptance-and-the-type-witness-pass.md) decision 5 keeps `rust-analyzer` out, and an approximation of dataflow that is wrong once is a kill reported as a survivor.
Running it is what an execution already is.

The exception is the whole-program case, and the engine has it:
[ADR 0013](adr/0013-codegen-identity-is-the-equivalence-proof.md) asks the compiler whether it renders the mutation identically, and an answer of `identical` is the strongest propagation proof there is — no test of any kind can notice it.
It costs a build of the mutant, which is why `--equivalence` asks it about survivors rather than before the run: a build costs more than the tests it would save.
That trade is about the sizes, not about the proof.

None of this reaches a **killer** layer.
A proof layer removes an execution and must therefore fail toward running more; a killer adds one, so the wall that stops propagation from being proved cheaply is not a wall it stands behind.
A model checker over one function — Kani is the one to look at — says "no input distinguishes these two", which is a stronger answer than any test run, and it is fail-closed for the ordinary reason: it costs a run, and a run it cannot finish leaves the mutation to the tests.

That paragraph was written as a design.
It has now been held against Kani 0.68 with CBMC 6.11, and what a spike established is here rather than in somebody's memory, because every premise below decides a piece of the shape and two of them are not what the design assumed.

- **It answers the question.** A differential harness — both renderings of one function, one symbolic argument, `assert_eq!` between them — proved `>` to `>=` on a clamp equivalent in six milliseconds, and disproved `>` to `<` in the same run.
  This is the propagation answer: not *no test noticed* but *no input distinguishes them*.
- **Every argument type must be `kani::Arbitrary`.** The harness needs `kani::any::<T>()` for each one, so the observer can be asked about a function of `i32` and not about a function of somebody's struct until that struct is arbitrary.
  What can be asked is therefore a property of the signature, and a run can say so without starting anything.
- **An unbounded loop does not terminate.** Not *slow*: the harness above,
  with a `while i < n` over a symbolic `n`, produced nothing in ten minutes and was killed.
  So a bound is not a tuning knob a caller may leave alone,
  it is a thing the caller must set, and `Undecided` is reached by the caller's clock rather than by Kani declining.
- **Under too small a bound Kani says `UNDETERMINED`, not `SUCCESS`.** Two functions differing only at the eleventh iteration, asked under a bound of five, come back undetermined with a failed unwinding assertion beside them — rather than the false proof the design feared.
  The status is three-valued and maps onto the answers directly: `SUCCESS` to proved, `FAILURE` to noticed, `UNDETERMINED` to undecided.
- **The rule is still enforced here rather than trusted there.** A proof is the equivalence assertion succeeding *and* every unwinding assertion succeeding.
  Kani already refuses the other combination, and a layer that removed executions on the strength of somebody else's refusal would be trusting the tool it exists to check — the same rule that keeps `xtask proofaudit` re-deciding what the producer's types already forbid.

How far it reaches was then measured on this workspace rather than guessed,
because that number is what says whether the observer is worth its contract and it costs no checker run to get.
Of 5,177 functions, 2,073 take no argument at all — a symbolic input cannot be made where there is nothing to make one of.
Of the remaining 3,104:

| | |
| --- | --- |
| a checker can be asked | **65 (2.1%)** |
| blocked by `self` | 993 |
| blocked by `&str` | 396 |
| blocked by `&Path` | 321 |
| blocked by a borrowed type of this workspace's own | ~300 |

**2.1% is the inconvenient number and it is the correct output.** The first figure taken was 41%, and it was 41% because it counted the 2,073 functions with nothing to make symbolic.
Splitting those out is the same move as verifying that a faster gate is doing less of the same work rather than less work: the headline agreed with what was hoped for, and the count underneath did not.

I said next that the population was the wrong one — `self` at a third of the blocked set is methods and `&Fixture` at 143 is test support, so a survivor,
living in the logic a test did not reach, should skew away from both.
**That was a guess and it was wrong.** Measured again over `crates/*/src` alone,
with test and bench trees excluded and methods counted separately:

| | |
| --- | --- |
| free functions taking at least one argument | 1,448 |
| a checker can be asked | **33 (2.3%)** |

Two tenths of a point.
Excluding every test, every benchmark and every method moved nothing, because what blocks the question is not *where the function lives* — it is that this is a program about paths, source text and records, and a symbolic `&Path` is not a thing a checker mints.

Those measurements originally argued against spending the milestone here.
That decision was superseded by the campaign's stricter completion rule: an opt-in assurance contract may be narrow, but it may not be half-built.
K1 is now complete as `verified-v1`; the 2.1% figure describes its deliberately small domain rather than a reason to leave the protocol unaudited.

The implementation does not retain the spike's open-ended `askable` guess.
Eligibility is one exhaustive Rust AST over free functions with at least one plain by-value argument.
Inputs are closed compositions of primitives,
arrays, tuples and `Option`; outputs are the same equality domain without floats.
Calls, methods, globals, references, macros, named fields, unsafe or ABI modifiers, configuration attributes, and profile-dependent arithmetic are typed refusals.
The original and mutant are both checked before a harness exists.

Each admitted question first gets a fresh copy of the immutable prepared tree,
which is re-digested before and after the attempt.
Kani does not compile that package.
The compiler receives a second, exclusively created minimal crate:
one fixed dependency-free manifest, one fixed lockfile, the original and mutant function clones, and the harness.
The pristine source is an inert hexadecimal record in that generated file, allowing the independent audit to re-mint the mutation while keeping `build.rs`, procedural macros, dependencies,
and unrelated targets out of the proof program.
The three crate files are read through no-follow directory handles and checked before and after every Kani subprocess.
Each attempt also reserves a new empty private target directory,
so a stale or symlinked Cargo cache cannot become a proof input.
The measured host target is explicit.
The isolated crate is always offline.
Kani 0.68 accepts no Cargo `--locked` option on either proof or catalog discovery, so both are instead confined by the exact dependency-free manifest,
exact empty lockfile, offline environment, and pre/post whole-crate check.
The report binds those files, the generated source, and the closed `minimal-v1` subprocess environment in a domain-separated `crate_input` digest which `modelaudit` independently recomputes;
subject feature and profile settings cannot enter the dependency-free proof crate.
Caller rustflags, profile overrides, compiler selection and wrappers are either masked by a higher-precedence empty value or rejected as a typed configuration refusal.

The generated program uses mutation-unique aliases for both the external `core` and Kani crates, so names in the subject cannot rewrite the proof boundary.
It contains no executable subject-package code beyond independently named original and mutant clones and exactly one tagged equality assertion.
Kani 0.68, its two-line version banner, export schema 1.0, embedded nightly rustc, CBMC/goto 6.11, solver,
host target and release build mode are exact protocol values.
This is a proof under the pinned Kani compiler semantics, not a claim that Kani and the measured rustc are byte-identical.

The result is genuinely three-valued.
Only every safety/unwind property plus the tagged assertion succeeding is `model-proved`; only the tagged assertion failing while every sibling succeeds is `model-noticed`.
Exhausted unwind,
outer timeout, cancellation, an unknown property, a protocol mismatch or any other failure remains `survived` with one typed `undecided` reason.
Raw JSON,
generated Rust, process exit, backend identity and their SHA-256 identities are retained.
`modelaudit` decodes the pristine-source record, re-mints the mutation, and reconstructs the complete generated proof program, then re-parses the raw export without calling the producer, and `proofaudit` requires exactly one model record for every `verified-v1` test survivor.

A fake checker fixes argv, environment, timeout and malformed-protocol regressions; property tests and `model_result` fuzz arbitrary bytes through the strict parser; the pinned real checker compiles both shadowing regressions and exercises proved and noticed paths in the gating `kani-verified` CI job.

And a quarter of what is blocked is borrowed bytes — `&str`, `&Path`,
`&[u8]`, `&String` together.
`verified-v1` rejects every reference before harness generation.
Kani could search bounded borrowed data, but such a search **must not be spent as a proof**: *no input of up to sixty-four bytes distinguishes these* is not *no input distinguishes these*. A future bounded search therefore needs a new, non-affirmative typed result rather than a wider interpretation of `model-proved`.

`Proved` will not grow a qualifier.
A word that sometimes means *within sixty-four bytes* is a word every renderer already written is now misusing,
and **there is no diff for anybody to review** — the damage is invisible precisely because nothing changed.
The current closed reason set has no bounded-search variant, so a bounded borrowed-input experiment cannot enter an assurance report at all.
If *searched to sixty-four bytes and found nothing* is ever worth reporting it is a new non-affirmative reason with its own name and bound, argued for on its own rather than inherited by widening this one.
Closing a set is worth doing; the first thing to do with a closed set is not to loosen a variant.

Somebody reading 2.1% will want exactly that loosening, which is why the refusal is on the same page as the number.

A model checker that hangs is not a checker that answers slowly.
That is why `verified-v1` requires both a nonzero unwind bound and a nonzero wall-clock cutoff.
The process group is supervised; timeout is the typed `Undecided::Cutoff`, and a failed unwind assertion is the distinct `Undecided::BoundExhausted`.
Neither can become an affirmative decision.

Any later widening starts from the measured 2.1% reach, preserves this closed decision boundary, and earns its own differential and audit evidence rather than silently enlarging the meaning of an existing result.

Speed is a product gate, not a deferred layer.
The first focused `doctor.rs`/`plan.rs` rerun exposed a preparation bug: although only 94 mutations could run, validation compiled all 12,743 candidates and spent 19,376 seconds doing it.
Selection now reaches the witness, validation and instrumentation passes before they compile anything while the catalog itself stays complete.
The comparable 97-mutation run spent 56 seconds in validation and 17:59 wall-clock in total — 346 times faster in the faulty phase and 18.9 times faster end to end.

The remaining repeated cost was `verify`, which started every target again for an unchanged instrumented tree.
A wholly passing baseline is now remembered under its complete execution inputs and reused only after every newly built target executable compares byte for byte (a doctest is additionally keyed by its exact Cargo command).
Failing, tree-writing, incomplete and changed measurements always run again, and `--no-cache` bypasses the layer.
That makes the baseline a first-measurement cost instead of a tax on every iteration; the performance record in [limitations](limitations.md) carries the heavy fresh phase and the CLI-level process-elision proof.
Total wall time over equivalent mutants, including preparation and baseline, is the comparison metric — mutation-loop speed on its own is not a product claim.

One candidate was measured and is not one.
The witness pass checks into a target directory of its own, and a cold run spends sixty seconds there, which reads as a dependency graph built twice.
It is not: measured warm on both products, that check costs six to nine seconds whatever the workspace, while the pristine check costs three seconds when nothing was edited and twelve to thirty-seven when something was.
The dependencies in the witness directory are reused between runs; what the sixty seconds bought was the first build of a directory a fresh `TMPDIR` had just created.
Sharing the directory would buy that once and pay for it on every run afterwards, so the separation stays.

The runner has no saving to import either, and that is now a decision rather than an accident: [ADR 0018](adr/0018-the-assurance-layer-rides-the-standard-interfaces.md) settles that a third-party test runner is neither a dependency nor an option,
because a tool that might become part of the language's infrastructure hands its dependencies to everybody who adopts it.
What somebody installs to use this is cargo.
So the baseline is made cheaper by making the baseline cheaper,
and by nothing else.

M14 is done.
A run is cut into parts with `--shard K/N` and put back together with `njutest merge`; `njutest cache` collects and expires the store, and `--export`/`--import` carry its answers between machines, which is what lets a matrix reuse what one of its legs established.
An answer that arrives is held to what a run of the receiving machine would keep it to, because that rule lives in `Store::put` and nowhere else.
What the answers do not carry is the build: the compiled tree is the engine's, keyed to the workspace root under the temporary directory, and `docs/ci.md` says how a matrix caches it.

## What this engine still cannot measure about itself

A test that inspects the tree it is being measured in reads a tree carrying the guards rather than the one a person wrote.
That refusal remains correct,
and `docs/limitations.md` says so.

Most of the cost turned out to be avoidable.
Before subprocesses inherited the outer mutation identity, **a guard recorded only what its own process reached.** A suite that started the binary was therefore measured by nothing the binary then did, so the whole command layer of both products — every suite that drove `rust-mutants` or `njutest` as a child — was reached by no mutation at all.
Those suites drive `run_from` in this process now, which is the same command against the same tree with test-level attribution.
What kept them starting a process for so long after the reason to stop was known is that they were written against `std::process::Output`;
`njutest_devkit::process::answered` hands that shape back for a command driven here, so each suite changed in one function.

What still starts a process is what is about a process: an interrupt, a hang, a panic, a stream read while it is still being written, the language server's stdio, and the suites whose subject is a variable a process inherits.
Those processes are measurable now.
Each instrumented composition root embeds its catalog digest and accepts exactly one inherited `ACTIVE` or `TOUCH` mode only when the nonempty catalog beside it matches.
Normal builds, partial pairs,
stale catalogs, and both modes remain `RM0006`; Cargo's environment tracking rebuilds the binary when the catalog changes.
The only configured target skip left here is the gate whose subject is the instrumented tree itself.

## What the configuration audit closed

A configuration key is a promise: the file says what the run will do, and the run does it.
Reading the two against each other found **five keys the file promised and no run kept**, every one of them on one page, and one command that read none of it.

`[project] exclude` reached the report, the evidence identity and `watch`, and never the engine, so a tree that said "do not mutate the generated code" mutated it and reported the findings.
`[project] packages` narrowed the evidence key and nothing else, so a tree that named one member measured every member and called it `SCOPE_ASSURED` — a narrow claim over a wide tree, and with a name nobody wrote it exited zero.
`[execution] features` reached the key alone, so two runs configured differently were told apart and both measured the same program.
`[execution] test_binary_args` was validated by both products and read by neither.
An acceptance's `expires` was a comment: a suppression a reviewer put an end date on went on hiding its finding for ever.
And `njutest plan`, which exists to say what a run would measure, read none of the configuration that decides it.

Two of those turned out to be about more than the key.
The arguments a run gives the harness reached the mutation executions and never the **baseline**,
in both products, including the ones that did work — and the baseline is what every result is held against, so a mutation could be noticed by a test the baseline never ran, which is a kill nothing vouched for.
And the equivalence layer built its tree with the defaults, so "the compiler renders these identically" was a claim about a pair of programs the run had not measured.

The shape was the same every time: **a narrowing whose empty answer is spelled the same way as an honest empty answer.** The same reading found the flags that narrow nothing and say nothing — `--skip-target` naming no target, `--run` naming no stored run — and one gate that had the shape itself: the ledger test holding the command-line page to the help texts pooled every flag of every command into one set, so a flag that exists on some other command counted as existing on this one, and `explain --run` was on the page and not on the command for as long as that test had been green.

## What E11 closed

The infection question — the test ran the mutation and the mutation computed the same thing anyway — had an answer that cost a **second instrumented tree**: another build of the workspace and another run of every target, which is why `--probe` was off by default and why the layer almost never fired.

The instrumented tree already holds both branches at every site.
What stopped a run from evaluating both is that evaluating a mutation is running the program's code.
But the witness pass of [ADR 0008](adr/0008-compiler-validated-acceptance-and-the-type-witness-pass.md) already asks the compiler exactly when it is not: a condition of primitives,
comparisons and connectives runs none of it, and a swap of one of its operators leaves it that way.
So a guard there evaluates both branches and records every time they parted, on the run that was already recording reach ([ADR 0015](adr/0015-the-guard-is-the-infection-probe.md)).

`never-infected` became a default layer that costs nothing, and it narrows per test rather than per target.
Every fixture whose one survivor was a comparison inside an `if` — eight of them — now reports that mutation without starting a process for it, and the differential harness still holds every answer to a run with nothing removed.

Making the record say what it can and cannot speak about turned up a hole ADR 0014 had left: a branch body inside a guard's own site takes no marker, and `branch-never-taken` was reading the resulting silence as "nothing entered the body".
The record now carries which markers the tree actually holds, so such a body falls back to the coverage region and, with nothing measured, its targets run.

And the measurement said where the layer does not reach yet.
A `--dry-run` of this repository's own engine — 5,716 mutants against 67 targets — removes 88.6% of the pairs and 93.8% of the tests, and **every one of them by reach**:
`discharged=0`.
The witness pass claims almost nothing here even after its type boundary was widened, because the conditions this engine writes mostly call program code.
The sealed trait now names text and standard-library containers as well as primitives, and `w_ord<A, B>` asks about the operands independently:
unlike the first version, it proves heterogeneous comparisons such as `Vec<u8>` against `&[u8]` and `String` against `&str`.
The toolchain proof suite compiles and observes both directions, so that boundary is no longer a proposed widening.
The `witness` note remains the measurement of whether a future closed, effect-free type family would pay for its own expansion.

## What E10 closed

The measurement was a second full build.
Instrumenting for coverage reaches every crate in the dependency graph through `RUSTFLAGS`, so finding out what the tests reach cost more than running them, and stable Rust has no way to ask for it on the workspace's own crates alone.
And a region places a mutation in a **target**, which then runs every test it has for that one mutation: a library with two hundred tests ran two hundred of them to find out what one of them would have said.

The engine was already running every target once with nothing activated, to refuse a session whose instrumented baseline does not pass.
The guards report what they reached on that run, and libtest names each test's thread after the test, so the measurement is free and it is per test ([ADR 0014](adr/0014-the-guards-are-the-measurement.md)).
`branch-never-taken` came with it: a marker at the first statement of the body a claim names is a thing that either ran or did not, where a coverage region has to be inferred from where regions begin.

On this repository's own engine — 5,680 mutants against 67 test targets — that removes 87.6% of the processes and 93.6% of the tests, and makes no coverage build at all.
The guards turned out to be more precise than the regions as well as cheaper: every fixture fate that moved moved from `survived` to `unreached`, and no kill was lost.

## What E9 closed

Every layer this engine has removes work by proving the work would have established nothing.
The number that says whether that is getting better was the one number nobody could see: a run said how long it took, and a second is about the machine rather than about the engine.

A run is now counted in **pairs** — one mutant asked of one target, one test process — and every pair short of a whole run is one something named removed.
The count is the same on every machine, so `xtask/work_ceiling.txt` can hold it the way the seam allowlist holds seams: it may shrink and never grow.

And because a removal is a claim, the claims are checked.
Four fixtures run twice, once with the measurement and the proofs on and once with nothing removed, and the two reports are held to each other mutant by mutant.
A discharged mutant that turns out to be killed when something actually runs it fails a six-second test rather than somebody's report.

The first work removed under that discipline was the cache key, which used to be the digest of a whole tree: a note beside the code threw away every remembered answer.
It is now the sources the build actually compiled, the manifests that chose its dependencies, and the toolchain — finer, and stricter than before, since the toolchain was not in the old key at all.

## What E8 closed

The engine could establish things and could barely say them.
A run printed nothing until it was over, so a person watching a long one had no way to tell a slow phase from a hung one, and a program had nothing to read but an exit code.
There is a line per phase and a line per mutant now, in completion order, and `--json` is the same run as one object per line with a reader shipped so a consumer does not have to write one.

What a run measured, it measured whole.
Narrowing meant editing the configuration or the command until the catalog was smaller, which changed the identities and threw away the cache.
The filters cut after the catalog instead: the digest does not move, a dropped mutant is `unselected` rather than missing, and `--from-report --outcome survived` is the loop a person actually works in — fix a test, ask the same survivors again.
`--dry-run` says what any of it would cost before paying for it.

A report could be read by this engine's own readers and by Stryker's.
A team already has a test view and a code scanning view, and a pull request already has a place for a summary; JUnit, SARIF and Markdown are those, with the mapping between six outcomes and four states written down rather than guessed.
The page grew the part that makes a survivor worth reading: the file it is in, whole, with the mutation on its line — and it shows a file only when the recorded digest says it is the one the run measured, because showing the bytes that are there now would be a lie about what was established.

When something went wrong there was no first step.
`doctor` answered about four things and graded none of them; now it answers about thirteen, says whether each is `ok`, `warn` or `fail`, and carries the next step for every one that is not `ok`.
`diagnostics` is the second step: one command, one directory, everything a reader re-decides the run from, and the names of the environment variables with none of their values.
Both report a reserved variable instead of refusing to start under it, which is the one situation they exist for.

## What E7 closed

The walk had places it passed over in silence.
A `const fn` body was counted as a `const-context`, a condition that binds with `let` and a range with no end were counted as nothing at all, and a return type the syntax cannot default produced a candidate the compiler would refuse — a refusal that reads as a fact about the program rather than a decision the tool made.
Each is a reason now, and `crates/rust-mutants/tests/census.rs` is what holds it: for every file it walks, the decisions are exactly the candidates plus the skips.
`cargo xtask engine-audit --sites` re-derives the same count from a real run's recording.

The operator table grew from fifty-one rules in twelve families to sixty-nine in fifteen.
A `match` had no operator of its own, though two questions about one are worth asking: does anything notice when an arm stops matching, and does anything notice when its guard stops narrowing it.
An arm without a guard has nothing to replace, so Form M writes one — the only guard shape that adds syntax rather than replacing it.
A function returns from every branch of a returned `if` or `match`, and only the whole expression was a site, so a suite that noticed the whole could notice nothing about one branch and the run said it was fine.

A reviewer who had decided a place was not worth measuring had nowhere to write it down.
`// rust-mutants: skip <reason>` is where, and `[[mutation.skip]]` is the same decision from the configuration file.
Both require the reason, because a skip nobody explained is one nobody can review,
and both report a claim that hides nothing rather than letting it quietly stop meaning anything when the code under it moves.
An expectation can now name its mutant by where it is rather than by an identity minted from the whole file's digest, so a claim survives an edit anywhere else in the file and the run says where the code went.

## What E6 closed

Two products' worth of documentation described an engine that did not exist.
`--trace` had shipped, but the engine still could not be told what to compile:
a project's features, target triple, and profile were nobody's to choose, so a run measured the defaults whatever the project ships.
It compiles what it is told to now, the report says which program was measured, and a stored outcome is only reused for a run compiled the same way.

Reading the rest of what the contracts promised found a run's worth of wrong answers.
A tree that configures `build.rustflags` lost coverage routing entirely, because the coverage build replaces them and nobody read them.
A crate that forbids one of the lints a guard's own attribute turns off had every mutant refused with nothing saying why.
A tree that type-checks and fails to link was accepted, instrumented, and failed every round, where the failure reads as a mutation the compiler refused.
`Ctrl-C` during a round was read as a tree that does not compile, so bisection condemned mutants nothing had refused.
Attribution read only a diagnostic's primary span, so every type error whose primary span is the definition cost a bisection to rediscover what the message already said.
And bisection, having found the offenders, said "the compiler refused this mutant, and no diagnostic named it".

The proof layers were the other half.
The compiler had been vouching for branch proofs and the probe had been recording what each test infects, and nothing read either back: a mutation the tests run and cannot observe was executed against every target that reached its line.
`prove::discharges` puts the lemma and the premise together as a pure function, coverage is on by default, and 139 rows of the fixtures' fate ledgers moved from `survived` to `unreached` with no kill lost.
What a run keeps beside its report — the measurement, the catalog with the branch bodies, every probe log — is what lets `engine-audit` re-decide every discharge without the engine.

Turning coverage on turned up two more.
The coverage build compiled the whole workspace where the run was about some of its packages, and a test that wrote into the tree during the coverage pass was absorbed by the reseal that takes in the instrumentation, so the drift report said nothing about it.

## What E5 closed

The engine could not record what it did.
Every path in its command line handed the recorder a disabled one, so the only place a person could watch the engine decide was the runner's copy of the trace, which stops at the engine's edge.
It records now, beside its own report, and reads one back with `trace summary`, `trace check`, and `trace diff`.

What that made possible is the audit.
`cargo xtask engine-audit` re-decides a completed run in nine layers with code that never calls the engine's, and running it on three real runs of the fixtures found three things the engine was getting wrong: merging the parts of a sharded run turned a mutation nothing reaches into a broken run rather than a gap in the tests, a mutant nobody could decide was reported as a timeout that did not repeat whether or not anything had timed out, and the recording of a route named a mutant one way where the recording of its execution named it another.
The ledger tests found four pages that had stopped saying what the code does.

Running the things that were supposed to work found the rest.
`RM5002` said the pristine tree passes, which a run type-checks rather than runs.
A coverage export whose region ended before it began was read rather than refused.
A test process inherited `LLVM_PROFILE_FILE`, so a project measuring its own coverage had a profile written into the tree the run was measuring and the drift report blamed its tests for it.
And nothing built the scripted cargo eight of these tests drive, because `cargo test --all-targets` builds an example as a libtest harness rather than as the program it is: a clean checkout failed seven tests,
the coverage job failed them all, and the engine could not verify its own suite at all.

The suite was also paying for a toolchain it did not need: the inner loop is now the half that starts no cargo, and the `toolchain_` half runs everything it did before.
With all of it in place the engine runs on its own `duration` module and re-decides the result with no violations, and the one mutation that survived was a `?` nothing exercised.

## What M9 closed

Each of these was a page promising something the code did not do, found by reading the two against each other: a timeout that raised no finding, an acceptance that answered for an outcome nobody could sign off, an infection proof that never fired, a scoped run that built every package, `[execution] jobs` that nothing read, a `replay` command that was in the help and was not a command, a documentation target nothing ran, and a mutation reported as reaching nothing on evidence that said no such thing.

A loaded machine is still a different machine from the one a budget was calibrated on, which is why an expired budget now buys one measurement with the machine to itself before a run decides that time really ran out.
