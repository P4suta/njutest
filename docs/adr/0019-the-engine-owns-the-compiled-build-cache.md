<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0019 — The engine owns the compiled build cache

## Status

Accepted, 2026-09-11.
Supersedes [ADR 0005](0005-build-cache-njutest-owns.md) and [ADR 0010](0010-target-directories-are-the-cache-layers.md).

## Context

The runner once owned a second machine-wide build-cache hierarchy, its configuration, marker, status, and collector.
The implemented verification path no longer compiled through those layers: rust-mutants prepares the instrumented snapshot and builds it into a target directory whose name is stable for the source root.
Keeping a second owner left configuration and garbage collection for artifacts no run produced.

Cargo target directories are compiled state.
Two programs collecting the same class of state cannot agree about liveness from their own locks and markers, and a status line from the program that did not build the artifacts does not identify their owner.

## Decision

1. rust-mutants' stable target directory is the only persistent compiled build cache used by a verification.
   Its lifecycle, locking, and collection belong to the engine.
   Native builds are keyed to the source root; sealed builds are keyed to the tree's content, so linker object paths, preopens and environment values stay identical across source roots ([sealed execution](../engine/sealed.md#remembering-what-one-execution-established)).
2. njutest owns no persistent compiled layer.
   Its `[cache]` table controls only outcome answers: `max_bytes`, `ttl`, export, and import.
3. Cargo started from a test process remains isolated with `CARGO_TARGET_DIR` under that run's `Scratch`.
   This is disposable process isolation, not a cache shared across runs.
4. `njutest cache` neither reports nor collects compiled artifacts.
   The retired `build_dir` and `build_max_bytes` keys are rejected as unknown fields, and an upgrade removes them.

Compiler inputs are held by canonical filesystem identity before the complete key is minted.
Every actual dep-info path is resolved through that same identity boundary before graph coverage is accepted.
An included source's climbing spelling and a shared target spelling therefore name the same verified input.
Resolution failures and genuinely external inputs remain refusals with their original I/O cause.

## Consequences

- There is one owner and one command surface for persistent compiled state.
- A runner run still removes builds started by the project under test when its scratch closes.
- Existing configuration carrying either retired key stops with a named parse error instead of silently keeping a setting that does nothing.

## Amendment, 2026-09-26: a unit is fresh only for the bytes it was built from

### Context

Cargo decides whether a unit is fresh by comparing the modification times of the files it read with the unit's own.
The copy a run builds keeps the time each file was written, on purpose, so that a second run compiles only what changed.
The engine rewrites files the time does not describe: an instrumented file is the same file with other bytes.
A member one run instrumented and a later run leaves as written therefore carries a time older than the instrumented unit in the shared directory, and cargo links that unit.

Measured on storage-scout: a run cataloging only `crates/cli/src/watch.rs` linked a `storage-scout-core` an earlier run had instrumented with another catalog, built by an older release.
Every test that entered it stopped at the old runtime's step check, which exited 94 and said nothing; nine mutants were errored, and a baseline could fail for reasons no source held.
The fixture reproduction is two runs of `fixture-witness-downstream` sharing a target directory: the whole catalog, then `--include crates/downstream/src/lib.rs`, whose two mutants errored with exit 94.

### Decision

1. Every target directory a build writes into keeps `rust-mutants-built-v1.json`: for each workspace member, the digest of every file of the copy under its directory as the last build that could write its units found them, and the moment that digest was recorded.
2. Before cargo runs, `compile` settles the directory: a member whose digest differs from the record, or that the record does not name, loses every fingerprint cargo keeps for it, under every profile and target triple, and only then is the record rewritten with the new digest and the present moment.
   A unit without a fingerprint is one cargo compiles again, and a unit that depends on it follows.
3. Settling then gives every file of every member the moment its member's digest was recorded.
   That moment is older than every unit built from those bytes, since the member's older units were removed at it, and newer than every unit built from any others.
   So a file's time says what cargo needs to know whoever wrote it, and bytes the engine writes again the same, as instrumenting an unchanged tree does, compile nothing.
4. `CompileOptions` names its target directory as a `BuildDir`, which carries the members, so no build into a shared directory can skip the settling.
 5. A directory inside another that keeps its own record, such as `witness`, `coverage` or `pristine`, is another target directory; settling the outer one passes over it.
    The copy as it was written is checked in `pristine`, apart from the instrumented builds, because one directory given the two trees in turn would compile each of them every run.
 6. Since 2026-09-29 the record is `rust-mutants-built-v2`, and a member's files are also every file of the copy outside its directory its units read, which a `#[path]` can name: after each build the record keeps, from the dep-info of that build, every such file each member's units read, and the digest settling compares takes them in with the member's own.
    A file no member's directory holds therefore moves the member that reads it, where before it moved nothing, and only a clock that disagreed with its time kept cargo from reusing what it built.
    A `rust-mutants-built-v1` record kept no such file, so it is read as no record: every member is compiled again once, and the next build writes the record this release reads.
 7. A record that cannot be read, or that is not one this release writes, is `RM1022`, rather than a record the run trusts or silently replaces.

### Consequences

- A unit cargo judges fresh was compiled from the bytes the copy holds now, whatever an earlier run, an earlier release, or another catalog wrote there.
- A directory an earlier release filled has no record, so the first run after an upgrade compiles every member again, once; dependencies are not members and keep their units.
- A member whose bytes are the same as last time keeps every unit, even where the engine wrote them again, so a repeat run of an unchanged tree compiles nothing: on `fixture-simple` the third run rewrote no fingerprint, where it used to rewrite every member unit's.
- The record is per member and not per file, so an edit to one file compiles its whole member again, which cargo would have done for the unit that file belongs to.
- A file no member's directory holds keeps the time it was written, which only a person changes, and cargo reads that time as it always has.
- The pristine check moved into `pristine`, so a path under the target directory that a unit's dep-info names now begins `$target/pristine/`, and outcome keys that name one change once.
- Documentation examples run through `cargo test --doc` against the tree the last settled build compiled; nothing writes the tree between that build and them.

## Amendment, 2026-10-01: identical fixture copies share compiled content

An explicit `NJUTEST_FIXTURE_BUILD_CACHE` root allows copied fixtures to claim an engine-owned build slot addressed by the complete snapshot digest, toolchain identity and build environment.
One verified immutable source graph is published under its owned event lease, and every claim creates a separate mutable copy from that graph.
An arbitrary number of claims uses the same graph without a four-slot fallback or a second semantic source survey.
The final copied graph is loaded once, and build scripts and procedural macros require the full environment in the target directory identity; ordinary graphs rely on Cargo's dep-info for compile-time environment dependencies while diagnostic and scratch names do not fragment the pool.
The engine's owner lock holds the slot through execution, cleanup and the lifetime of every shared set of sealed modules.
Every invocation keeps its own scratch, runtime records and verdict state.
No target directory is copied.

Wasmtime's built-in cache separately shares compiled host modules by bytes, engine configuration and Wasmtime version.
It is an engine-owned compilation layer, independent of the outcome and transcript stores.
Count diagnostics distinguish module-cache hits from transcript-cache answers.

## Amendment, 2026-10-02: a verified build hit starts no Cargo

The engine's compiler facade records eligible locked builds by their complete source, graph, configuration, toolchain, selection, flag and environment content identity.
The record lives under the engine-owned target directory and includes digests for every returned artifact and dep-info file.
Reuse reconstructs the compilation only after verifying that inventory and its Cargo messages; doubt returns to Cargo.
Opaque build scripts and procedural macros cannot establish this record because they may read undeclared inputs.
Flag variables are classified under the argument protocol cargo actually splits them by: the encoded forms on the unit separator alone, the plain ones on whitespace, with every attached and separate `-C` form read alike.
The sealed target admits the engine's exact deterministic linker switches and its WebAssembly platform object, bound by content beside the toolchain-owned linker.
The object must be a valid core WebAssembly module with known linking metadata and the engine's `platform.o` filename.
That format carries symbols and relocations in its own bytes, rather than response arguments or filesystem members.
Only established scalar codegen options and configuration arguments are admitted otherwise.
Response files, archives, dynamic or unknown object metadata, other object formats, file-bearing compiler options, unknown switches and unsupported separate values fall back to Cargo with their exact cause.
The bound file inputs also add a content fingerprint to the actual compiler flags before the key is computed.
Cargo therefore recompiles when an object's bytes change at the same path, even when its dep-info omits that object.
The fingerprint also changes for corrupt object bytes before their format refusal, so fallback Cargo cannot publish an old fresh output for that change.
Cargo's absent uplifted library dep-info is tolerated only when the remaining compiler unit inventory still proves the complete input set.
A native target's link arguments remain refused, because its system linker is no toolchain input.
This layer applies to users' repeated runs as well as leased fixture slots, and its hit/miss notes always identify the key.
The suite count gate budgets unique bound build keys plus uncacheable requests per binary, rather than allowing a warm cache to hide new build requests.

## Amendment, 2026-10-02: complete products and independent witnesses have distinct owners

### Context

A content key alone did not own the mutable paths Cargo returned.
Another compilation could overwrite those paths, concurrent cold requests could each prepare them, and Cargo's fresh bit could be mistaken for complete source provenance.
An equivalence control could also receive its original cache entry as an independent reproducibility witness.
A later prepare must measure an altered execution copy without changing the immutable products used as compiler proof.

### Decision

1. One kernel preparation lease covers input binding, settling, the actual process and publication into a target directory.
   A bound cold miss invalidates fingerprints for every package in the complete graph before the actual compiler runs.
   Publication owns a distinct immutable product inventory for each actual producer, rather than borrowing Cargo's mutable output paths.
   Source, dependency, configuration, executable, argument and environment inputs are checked again before publication and every reuse.
   Opaque input graphs continue through actual Cargo and cannot certify a complete bound identity.
2. The original actual producer has a private-constructor `CompilerObservation`.
   It retains complete or unbound input identity, typed product or independent-control purpose, actual execution and leader identity, raw stderr and stream digests.
   Reuse retains that observation and the original Cargo fresh bits without publishing another execution.
   `Compiler`, `VerifiedReuse` and `SharedRefusal` distinguish actual work, verified products and a waiting cohort's failed producer.
3. Preparation, process and publication failures have exhaustive typed stages.
   A publication generation lets requests that waited for the same producer share its actual refusal.
   A later request may recover, including after input A, input B and restored input A.
   A failed product publication never returns a bound mutable result.
4. Equivalence keeps an owned reproducibility pair for one complete input identity.
   The independent control must run a distinct actual compiler process, force the graph's fingerprints, recompile every source-reading unit and match the original immutable products.
   A pair retains both original observations and may answer further identical questions only while their complete identities still match the restored tree.
   An unbound graph requires another actual independent control, and a changed identity withdraws the old pair.
   A cache entry cannot witness itself, and a process that reused a source-reading unit still establishes no independent control.
5. Native execution receives separate `ExecutionProducts` copied only from the explicit compiler inventory under the same preparation lease.
   Their marker binds the immutable origin, while later prepares observe existing execution bytes without repairing them.
   Compiler proof continues to read the immutable inventory, so another compilation or an altered execution copy cannot change its witness.
6. Toolchain banners, target facts and locked metadata are reusable owned observations only for their exact executable bytes, environment, complete graph and configuration.
   The retained result carries the original actual process and raw captures.
   A hit starts no process, and actual standalone probes are counted once at their producer.
   Private standalone purpose types own an uncosted watch, while metadata retains its caller's actual execution recorder.
   Retained commands must match the executable, purpose, arguments, working directory and environment identity they claim.
   Unbound dynamic-loader search graphs remain opaque inputs and always use actual probes.
   Unknown executable selectors and incomplete metadata graphs use actual processes.
7. Unreadable compiler flag inputs expose a typed I/O cause and named input path.
   Refusal tests inspect that cause and identity rather than localized operating-system display text.

### Consequences

- Warm complete-input builds and repeated equivalence questions reuse verified products and one genuine independent pair.
- Freshness, source-reading and independent-process requirements remain enforced for actual compiler witnesses.
- An altered execution copy is remeasured without changing its compiler proof.
- Waiting failed requests retain the actual refusal, and later complete requests may recover.
- No target directory is copied, no repeated-build waiver is granted, and no reuse fabricates work.

## Amendment, 2026-10-03: doctest capture has an immutable preparation result

`PreparedDoctests` retains the original actual compiler observation, rustdoc report and captured inventory together.
The capture program has one cold preparation owner and a distinct immutable output path for each actual rustc producer.
Its source marker alone cannot certify a program; reuse verifies the original process and program bytes.
A changed program is observed as changed and recovery publishes another path.

Doctest preparation binds the complete source graph, actual rustdoc executable, capture program, compiler flags, requested arguments and full environment.
Only the internally owned output staging address varies with publication; the actual argv remains in the original observation.
Cargo message records and rustdoc report bytes are separated from the same raw stdout, and every reuse derives the report from that original stdout again.
The explicit compiler artifact and dep-info inventory is frozen beside the captured binaries, and source inputs are checked against those original dependency records.
A publication verifies complete capture accounting and every file digest.
Unaccounted reports remain actual observations and do not publish reusable products.

External include macros and unresolved documentation attributes cannot establish a complete doctest input graph.
They continue through an actual compiler and never receive a verified capture hit.
Prepared captures share the target's preparation lease, including settling, actual work, publication and failure evidence.
The legacy capture and emptying APIs retain their public behavior, while compiler preparation also holds its owned lease.
No successful removal or signal delivery certifies late-writer completion.

The private snapshot removal capability is constructed only after every observed producer's retained kernel generation has completed.
Lock release follows that completion, and production cleanup and Drop make one removal attempt.
A completion or removal refusal retains its typed cause and snapshot identity instead of being discarded or replaced by guessed retry sleeps.
The legacy cleanup adapter retains its explicitly injected clock and original controls, after the same producer completion boundary.

## Amendment, 2026-10-03: retained source placement preserves Cargo discovery

A standalone manifest must not inherit an unrelated workspace from the retained cache's ancestors.
The exhaustive retained or isolated source owner uses the caller's explicit isolated temporary root when the retained placement would cross that boundary.
An isolated placement with the same foreign ancestor is refused with its input identity.
Both placements retain one immutable complete source graph and separate editable leases without changing the original manifest.
The unchanged FNV 1.0.7 Rust 2015 control binds every original source digest, the complete catalog and every report row to its actual trace.
Both actual compiler tiers must pass all original sealed baselines and agree on all eleven outcomes.
Its one declared doubt remains the exact typed StackOverflow mutation, while all nine killed and one unreached outcomes remain established.

## Amendment, 2026-10-03: repeated reproducibility questions retain their actual pair

The devkit's fixed reproducibility fixture retains an immutable source owner and three actual compiler observations.
Original, changed and restored stages have exhaustive identities and distinct retained kernel generations, process IDs and publication nonces.
Every stage invalidates Cargo's source fingerprints and requires non-fresh source-reading units.
The restored stage cannot borrow the original observation as its independent control.
Original Cargo output, exact arguments, environment, source graph and artifact inventories remain verified on reuse.

Semantic input identity contains verified bytes and modes, while filesystem change stamps only guard observation reuse.
Restored A inputs therefore recover the original A pair after a different B pair without treating timestamps as content.
One owned kernel lease covers preparation, all three actual processes and publication.
Concurrent questions retain that one pair, and a corrupt record or artifact requires new actual work.
Opaque source macros, build scripts, compiler selectors and configuration inputs retain the original actual compiler fallback.
No cache entry manufactures an independent compiler process or changes Cargo's original freshness evidence.

## Amendment, 2026-10-03: loader namespaces and acquisition identities are inputs

A macOS fallback-library search is reusable only after each absolute search namespace, absent directory, alias and regular-file content has been captured and checked unchanged.
The same owned loader observation binds compiler products, toolchain banners and metadata; unknown loader selectors still require actual processes.
Known link-file bytes alter Cargo freshness before complete-graph eligibility is decided, including when the rest of the graph is opaque.
That link-content fingerprint excludes source placement, while the complete cache key retains path, environment and source-graph identity.
Generated native extension imports resolve the standard-library alias in their own runtime module on every supported edition.

A doctest acquisition retains either its complete request or its original unbound refusal identity.
Requests, misses, actual launches, failed launches and compiler provenance carry that same owned identity.
Actual launch presence is recorded before interpreting the process result, including cancellation and supervision failures.
The existing accounting law, independent compiler requirements and original cost records remain authoritative.

## Amendment, 2026-10-04: owned observations exclude only explicit output boundaries

A command observation shares its owner's strong filesystem change-stamp and content memo across fresh captures and retained responses.
Executable resolution, directory aliases, loader namespaces and complete file content remain checked before publication and reuse.
Caller-owned snapshot exclusions and the actual report directory identify outputs explicitly; guessed directory names cannot establish that boundary.
Metadata must name only manifest and target source paths present in the bound source inventory, so an excluded source cannot certify a reusable response.

## Amendment, 2026-10-04: expression macros have one private lexical owner

Rust 2015 resolves a module's unqualified macro re-export at the crate root, which refused eight actual FNV 1.0.7 alternatives.
Each instrumented file instead declares one private expression macro before its first owning item, after the original crate attributes and source prefix.
The runtime module and macro share a collision-free name, and native and sealed alternatives use the same lexical declaration.
The macro expands to the original expression without another scope, inference boundary or exported crate item.
Every inserted byte maps through the existing splice offsets for branch and constant diagnostics, while source line counts remain unchanged.
Statement-only files emit no unused expression macro.
Generated imports retain their module-relative standard-library aliases under every supported edition.

## Retained toolchain input identities

The existing process identity memo is retained with its original toolchain observation.
A cache hit still opens the current input and checks its canonical path, filesystem object, change time, length, modification time, and permissions before reusing the original digest.
Changed or unreadable generations cannot use the retained identity.
Unknown selectors, loader inputs, and incomplete observations retain actual hashing or Cargo fallback.
Every capture still checks the complete source and loader namespace.
Later compiler and runtime captures publish their identities through the same owned observation lease.
The retained cursor restores only while it stays byte-identical to the original publication its own content key names.
A publication whose content key the cursor has superseded merges its captures into the cursor's record without reading input bytes, and the cursor keeps naming the newer publication.
The input memo cannot create a compiler unit, a fresh artifact, or an independent reproducibility witness.
Actual hash reads and completed bytes remain recorded separately from process starts.
An unchanged second listing must read zero toolchain bytes and start zero physical processes.
