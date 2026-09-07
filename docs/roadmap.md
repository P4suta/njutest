<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Roadmap

**Status: M0 to M11 and E2 to E4 are done.** What remains of
M8 is the tag itself, which is a decision rather than a change: see
`docs/release.md`. The user's decisions: one workspace, the engine first,
the engine a standalone product too, every milestone completed, test-driven
throughout, developer infrastructure first.

Each milestone has two halves — the feature and the developer infrastructure
that lets it be seen, tested, and audited — and both are completion criteria.

| # | Milestone | Feature | Developer infrastructure | Done when |
| --- | --- | --- | --- | --- |
| M0 ✓ | Scaffold | workspace, lints, gates, CI, contracts, both command-line skeletons, the public API crate | devkit, error-code ledger, `xtask` gates from tests, `bacon`, `doctor`, `CLAUDE.md` | `mise run check` green |
| M1 ✓ | rust-mutants engine | stable IDs, byte splicing, snapshot and owners, process supervision, syntactic discovery, guards and runtime, compiler-validated acceptance, execution, the library API, `list`/`catalog`/`run` | engine trace, goldens, property tests, fuzz targets, fixtures, explain commands, contract tests | every fixture's fates are fixed by tests |
| M2 ✓ | `mjutest verify` baseline | configuration, targets, per-target baseline under coverage, regions, report v1, verdicts, `plan`/`doctor`/`init` | trace v1, diagnostics, testkit, `report-diff`, `trace summary`/`diff`, fuzz targets, benchmarks | a fixture yields a report with regions and a trace |
| M3 ✓ | Mutation phase | routing, paired confirmation, accounting, acceptances, `explain`/`replay`/`accept`, HTML/SARIF/JUnit | scripted session, route events, dogfood | mjutest reaches a verdict on itself |
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
| E5 ✓ | The engine sees itself | `--trace[=DIR]` on every command, `trace summary`/`check`/`diff`, typed `verify`/`probe-exec`/`witness`/`route` records, sub-phases through `prepare`, `Session::route` as a question anyone can ask, and the byte span and source digest a reader re-mints an identity from | a scripted toolchain the tests drive instead of cargo, the suite cut into an inner loop that starts nothing and a `toolchain_` half that does, `cargo xtask engine-audit` re-deciding a run in nine layers, three committed runs it re-decides, the dogfood ledger and its weekly shard job, and one test per ledger the documentation keeps | every judged mutant leaves one route record, `engine-audit --trace` re-decides three committed runs with no violations, and `mise run test:fast` starts no cargo |
| E6 ✓ | The contracts and the code (the engine) | a run compiles what it is told to (`[build]`, features, target, profile), it says which test noticed a mutation and what signal a process died from, a tree that reaches outside itself or does not link is refused before any round, a build script's generated code is skipped by name, a `harness = false` target answers by exiting, documented examples are a target a run can switch off, `build.rustflags` is read and put back rather than refusing the measurement, a crate that forbids what the guards allow is skipped whole, a cancelled build is a cancellation, attribution reads every span, bisection names what it isolated and what interacts, the budget is derived from what the target measured, four mutants are measured at once and delivered as they finish, coverage is on by default, a branch or a probe discharges a target that could not have noticed, and a survivor is asked whether the compiler renders it at all | `cargo::config` and `cargo::manifest` read what cargo does not report, `Session::judge`/`describe`/`source`, the driver and the report model in the engine, `Observer` and the worker pool, `prove::discharges` as a pure function, the evidence a run keeps for its own audit, the `proofs` layer that re-derives every discharge, `cache`/`select`/`identical`/`evidence` records, and eight fixtures for eight things nothing measured | `run --jobs 4 --probe` completes on the engine itself and `engine-audit --trace --ledger` re-decides it with no violations |
| E7 ✓ | Every place says what it is for | eighteen more operators and three more families — a match arm asked what it is for, a jump swapped, a terminal `else` dropped, a literal moved by one — a return type the syntax cannot default stated rather than guessed, every branch of a returned `if` or `match` a return site of its own, and `rust-mutants: skip` and `[[mutation.skip]]` as the two ways an author says what to pass over and why | `Form::M`, the guard written where an arm had none, the census that refuses a place the walk passed over silently, the item path every candidate now carries, locator-form expectations that outlive an edit elsewhere in the file, the `skip-claim` record, the `unmatched-skip` finding, the `sites` audit layer, and `fixture-annotated` | every place a rule targets is a mutant or a skip with a reason, and a claim that hides nothing is a finding rather than a comment nobody notices |
| E8 ✓ | The engine as a product | a run says what it is doing while it does it and streams `rust-mutants-run-stream-v1` for a program, `--fail-fast` and seven filters narrow it after the catalog, `--dry-run` prices it, `explain` and `replay` answer from what was stored, `doctor` grades thirteen checks and `diagnostics` gathers one run into one directory, `cache`/`merge --runs`/`run --run-id` name what a sharded run leaves, `rules` lists what a team can pin, and a stored run projects into markdown, JUnit, SARIF, a page that shows every survivor in place, and Stryker | the run-stream, explain, doctor, reached and diagnostics schemas, the two-way test between the command-line page and `--help`, the subcommand help golden, the README session recorded from the binary, the terminal browser's five recorded frames, and the ledger that keeps `schema/` and its page equal in both directions | a bug report is one command, every flag is on one page, and the README shows what the tool actually prints |
| E9 … | Answering without running | a run is counted in pairs of one mutant and one target rather than in seconds, what removed each pair is named and labelled a proof, a sufficient answer, a remembered one or a narrower question, and a remembered answer is keyed on the sources the build compiled, the manifests that chose its dependencies and the toolchain that compiled it rather than on the digest of a whole tree | `rust_mutants::work::Work` derived from the stored report alone, the `work` audit layer that holds it to the recording, `xtask/work_ceiling.txt` as a ratchet that may shrink and never grow, and the differential harness that runs four fixtures with every layer on and every layer off and holds the two answers to each other | the engine does less for the same question every release, and a test says so |

## Where E9 is going

Every layer this engine has removes work by proving the work would have
established nothing. The number that says whether that is getting better was
the one number nobody could see: a run said how long it took, and a second is
about the machine rather than about the engine.

A run is now counted in **pairs** — one mutant asked of one target, one test
process — and every pair short of a whole run is one something named removed.
The count is the same on every machine, so `xtask/work_ceiling.txt` can hold
it the way the seam allowlist holds seams: it may shrink and never grow.

And because a removal is a claim, the claims are checked. Four fixtures run
twice, once with the measurement and the proofs on and once with nothing
removed, and the two reports are held to each other mutant by mutant. A
discharged mutant that turns out to be killed when something actually runs it
fails a six-second test rather than somebody's report.

The first work removed under that discipline was the cache key, which used to
be the digest of a whole tree: a note beside the code threw away every
remembered answer. It is now the sources the build actually compiled, the
manifests that chose its dependencies, and the toolchain — finer, and stricter
than before, since the toolchain was not in the old key at all.

## What E8 closed

The engine could establish things and could barely say them. A run printed
nothing until it was over, so a person watching a long one had no way to tell
a slow phase from a hung one, and a program had nothing to read but an exit
code. There is a line per phase and a line per mutant now, in completion
order, and `--json` is the same run as one object per line with a reader
shipped so a consumer does not have to write one.

What a run measured, it measured whole. Narrowing meant editing the
configuration or the command until the catalog was smaller, which changed the
identities and threw away the cache. The filters cut after the catalog
instead: the digest does not move, a dropped mutant is `unselected` rather
than missing, and `--from-report --outcome survived` is the loop a person
actually works in — fix a test, ask the same survivors again. `--dry-run`
says what any of it would cost before paying for it.

A report could be read by this engine's own readers and by Stryker's. A team
already has a test view and a code scanning view, and a pull request already
has a place for a summary; JUnit, SARIF and Markdown are those, with the
mapping between six outcomes and four states written down rather than
guessed. The page grew the part that makes a survivor worth reading: the file
it is in, whole, with the mutation on its line — and it shows a file only when
the recorded digest says it is the one the run measured, because showing the
bytes that are there now would be a lie about what was established.

When something went wrong there was no first step. `doctor` answered about
four things and graded none of them; now it answers about thirteen, says
whether each is `ok`, `warn` or `fail`, and carries the next step for every
one that is not `ok`. `diagnostics` is the second step: one command, one
directory, everything a reader re-decides the run from, and the names of the
environment variables with none of their values. Both are the only commands
that report a reserved variable instead of refusing to start under it, which
is the one situation they exist for.

## What E7 closed

The walk had places it passed over in silence. A `const fn` body was counted
as a `const-context`, a condition that binds with `let` and a range with no
end were counted as nothing at all, and a return type the syntax cannot
default produced a candidate the compiler would refuse — a refusal that reads
as a fact about the program rather than a decision the tool made. Each is a
reason now, and `crates/rust-mutants/tests/census.rs` is what holds it: for
every file it walks, the decisions are exactly the candidates plus the skips.
`cargo xtask engine-audit --sites` re-derives the same count from a real run's
recording.

The operator table grew from fifty-one rules in twelve families to sixty-nine
in fifteen. A `match` had no operator of its own, though two questions about
one are worth asking: does anything notice when an arm stops matching, and
does anything notice when its guard stops narrowing it. An arm without a guard
has nothing to replace, so Form M writes one — the only guard shape that adds
syntax rather than replacing it. A function returns from every branch of a
returned `if` or `match`, and only the whole expression was a site, so a suite
that noticed the whole could notice nothing about one branch and the run said
it was fine.

A reviewer who had decided a place was not worth measuring had nowhere to
write it down. `// rust-mutants: skip <reason>` is where, and
`[[mutation.skip]]` is the same decision from the configuration file. Both
require the reason, because a skip nobody explained is one nobody can review,
and both report a claim that hides nothing rather than letting it quietly stop
meaning anything when the code under it moves. An expectation can now name its
mutant by where it is rather than by an identity minted from the whole file's
digest, so a claim survives an edit anywhere else in the file and the run says
where the code went.

## What E6 closed

Two products' worth of documentation described an engine that did not exist.
`--trace` had shipped, but the engine still could not be told what to compile:
a project's features, target triple, and profile were nobody's to choose, so a
run measured the defaults whatever the project ships. It compiles what it is
told to now, the report says which program was measured, and a stored outcome
is only reused for a run compiled the same way.

Reading the rest of what the contracts promised found a run's worth of wrong
answers. A tree that configures `build.rustflags` lost coverage routing
entirely, because the coverage build replaces them and nobody read them. A
crate that forbids one of the lints a guard's own attribute turns off had
every mutant refused with nothing saying why. A tree that type-checks and
fails to link was accepted, instrumented, and failed every round, where the
failure reads as a mutation the compiler refused. `Ctrl-C` during a round was
read as a tree that does not compile, so bisection condemned mutants nothing
had refused. Attribution read only a diagnostic's primary span, so every type
error whose primary span is the definition cost a bisection to rediscover what
the message already said. And bisection, having found the offenders, said "the
compiler refused this mutant, and no diagnostic named it".

The proof layers were the other half. The compiler had been vouching for
branch proofs and the probe had been recording what each test infects, and
nothing read either back: a mutation the tests run and cannot observe was
executed against every target that reached its line. `prove::discharges` puts
the lemma and the premise together as a pure function, coverage is on by
default, and 139 rows of the fixtures' fate ledgers moved from `survived` to
`unreached` with no kill lost. What a run keeps beside its report — the
measurement, the catalog with the branch bodies, every probe log — is what
lets `engine-audit` re-decide every discharge without the engine.

Turning coverage on turned up two more. The coverage build compiled the whole
workspace where the run was about some of its packages, and a test that wrote
into the tree during the coverage pass was absorbed by the reseal that takes
in the instrumentation, so the drift report said nothing about it.

## What E5 closed

The engine could not record what it did. Every path in its command line
handed the recorder a disabled one, so the only place a person could watch
the engine decide was the runner's copy of the trace, which stops at the
engine's edge. It records now, beside its own report, and reads one back with
`trace summary`, `trace check`, and `trace diff`.

What that made possible is the audit. `cargo xtask engine-audit` re-decides a
completed run in nine layers with code that never calls the engine's, and
running it on three real runs of the fixtures found three things the engine
was getting wrong: merging the parts of a sharded run turned a mutation
nothing reaches into a broken run rather than a gap in the tests, a mutant
nobody could decide was reported as a timeout that did not repeat whether or
not anything had timed out, and the recording of a route named a mutant one
way where the recording of its execution named it another. The ledger tests
found four pages that had stopped saying what the code does.

Running the things that were supposed to work found the rest. `RM5002` said
the pristine tree passes, which a run type-checks rather than runs. A coverage
export whose region ended before it began was read rather than refused. A test
process inherited `LLVM_PROFILE_FILE`, so a project measuring its own coverage
had a profile written into the tree the run was measuring and the drift report
blamed its tests for it. And nothing built the scripted cargo eight of these
tests drive, because `cargo test --all-targets` builds an example as a libtest
harness rather than as the program it is: a clean checkout failed seven tests,
the coverage job failed them all, and the engine could not verify its own
suite at all.

The suite was also paying for a toolchain it did not need: the inner loop is
now the half that starts no cargo, and the `toolchain_` half runs everything
it did before. With all of it in place the engine runs on its own `duration`
module and re-decides the result with no violations, and the one mutation that
survived was a `?` nothing exercised.

## What M9 closed

Each of these was a page promising something the code did not do, found by
reading the two against each other: a timeout that raised no finding, an
acceptance that answered for an outcome nobody could sign off, an infection
proof that never fired, a scoped run that built every package, `[execution]
jobs` that nothing read, a `replay` command that was in the help and was not a
command, a documentation target nothing ran, and a mutation reported as
reaching nothing on evidence that said no such thing.

A loaded machine is still a different machine from the one a budget was
calibrated on, which is why an expired budget now buys one measurement with
the machine to itself before a run decides that time really ran out.
