<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Suite cost

**Status: implemented**.
The complete nextest suite runs once, without partitioning, through `mise run test:cost`.
The task records successful and failed test output in JUnit and joins every engine cost record to that inventory.
Measurements are written under `target/suite-cost/run-*`, with per-test and per-binary durations, actual Cargo processes, build requests and verified hits, fresh units, module preparations and cache hits/misses, and preparation versus execution time.
The command uses three nextest threads, three outer Cargo build jobs, one nested job per toolchain test, an empty compiler wrapper and a fresh fixture pool.
`machine.json` and `load.jsonl` record the command, host, CPU count, load and sampled running toolchain processes.
JUnit start instants and durations also reconstruct exact toolchain overlap, and each test is joined to its nearest load sample.
`mise run test:cost -- --record` updates this platform's reviewed `.config/suite-costs.json` count ledger after a passing complete suite.
A new platform needs its own measured ledger before the gate can pass there.
The CI jobs continue running the same complete suite.
Cranelift `None` was rejected as the default: the recursive Rust guest spends 121,950 fuel rather than the established `Speed` tier's 121,923, which changes its full transcript.

The gate refuses growth in toolchain test inventories, actual fixture Cargo processes, build requests, module compilations and requested modules, and refuses missing cost records or toolchain binaries.
Cost records keep per-key multiplicity under `njutest-test-cost-v2`: every bound input key counts its own requests, actual processes, hits, misses with a concrete `cold:` or `repair:` reason, and refused record writes; unbound commands publish an `unbound: ` reason and builds outside the compiler facade a `direct: ` one, never a fabricated key.
The reader sums multiplicity across every record, test and binary of the measured suite, and refuses inventories that do not close — requests into hits and misses, misses into processes and their classed reasons, processes into the build count, and requests into the request count.
One shared count boundary holds every aggregate — per record, per test, per binary, per key, per reason and the suite totals — inside the u64 width a record could measure, so no summed total accepts what no counter could hold.
The report carries the global key inventory, the unbound identities, the suite totals and the redundancy violations themselves, so a review reads the multiplicity without reconstructing it from binary rows.
A normal cold build repeated for one bound input is a redundancy error across the whole measured suite, whatever binary or test repeated it.
One refused record write explains exactly one later cold build — the write failed, so the next request found nothing — and no nonempty refusal mapping excuses repetition beyond that bound; a corruption repair keeps its own class and reason beside it, visible and not redundant.
A miss reason without its stable `cold:` or `repair:` class and a nonempty cause is refused, so no miss can leave the redundancy accounting unclassified.
Strict reading and `--record` refuse redundant work independently of the budget, and `--measure-only` reports the violations it observed instead of certifying them, keeping a truthful account of unfinished optimization.
A v1 record never measured multiplicity and is refused rather than read with invented counters, and the committed pre-v2 ledger must be re-recorded before the gate can pass again.
Each binary's Cargo ceiling is its measured process count; there is no per-binary guessed cold reserve, and the direct guest binary's builds are counted as the unbound processes they are.
The module compilation ceiling is the recorded request inventory; build and module requests are also gated so warm hits cannot hide new work.
Requested modules include platform requests even when an already built object supplies the probe's answer, so a warm cache cannot hide new module work.
Actual module preparations and cache hits/misses remain separately reported.
Preparing more modules than the recorded requests also fails the gate.
`python3 scripts/test-suite-cost.py` checks growth, incomplete/foreign record rejection, unclosed inventories, suite-wide redundancy, the refused-write bound, unclassed miss reasons, aggregate widths and refused historical schemas.
Direct native builds, hand-written validation, the edit oracle and bundle builds publish the same diagnostic shape with their own `direct: ` identity, and the gate refuses silent zero budgets for the direct-build binaries.
Observed Cargo commands are separated into build processes, native test executions and other commands; `unobserved_cargo` names the classes no record can see — the `cargo -vV` toolchain banners — so the inventory states its own missing coverage instead of implying a false zero.
A recorded ledger certifies exactly the observation gaps it was measured with: its `unobserved_cargo` classes and the still absent module-key, host-wait and resource meters, and the gate refuses a ledger whose gaps changed until it is re-recorded after the reviewed change.
The direct guest binary's per-build records still scale with its build count, so its ledger record floor subtracts them; that is incomplete bookkeeping kept explicit, not a structural normalization that proves omitted-record coverage.
The producer side remains open as the next bounded task: honest record origin and context, counting actual process starts rather than attempted launches, complete Cargo probe coverage, counting `fresh: true` beside `fresh: false` artifacts, and standalone product measurements, which today record no cost record at all — an absence, not a measured zero; none of that is claimed here.

## Measurements on 2026-10-01

The paired command uses the pinned Rust 1.98.0 toolchain, `CARGO_BUILD_JOBS=6`, and two nextest threads on the same Mac.
The storage-scout watcher was paused during measurements because its scans can hold engine owner locks and interfere with cache reclamation.
The original complete suite took 2,737.029 seconds for 4,341 executed tests: 4,340 passed and one existing task-declaration test failed because it retained YAML quotes around the already available Miri command.
The task detector now strips paired outer quotes.
A separate instrumented run of all 805 toolchain tests took 2,438.805 seconds; two expected stored trace-shape tests and the fuzz lockfile check detected the newly recorded fields and dependency declaration.
A final 805-test count baseline took 2,567.466 seconds with the original dependency profile and both the module cache and fixture pool disabled.
It includes the engine facade, direct native builds, the native edit oracle, hand-written validation and bundle builds.
Its only failure was the new shared-pool ownership assertion added to an existing test, which correctly refuses the disabled pool after recording its preparation work.
The filtered inventory exactly matches the original 805 toolchain tests and is read with `--measure-only`; it cannot establish a passing ledger.
Correctness repairs and diagnostics remain in that execution mode, and the original complete-suite wall time remains the unchanged-code baseline.

| Baseline toolchain work | Count or summed time |
| --- | ---: |
| Prepared Wasm modules | 1,777 |
| Compiled-code cache hits | 0 |
| Module preparation | 451.041 s |
| Guest execution | 6.026 s |
| Fixture Cargo build calls | 6,598 |
| Fresh compiler artifacts | 12,253 |
| Fixture Cargo elapsed time | 1,665.903 s |

| Slowest baseline binary | Summed test seconds | Modules | Cargo builds |
| --- | ---: | ---: | ---: |
| njutest::toolchain_verify | 682.116 | 235 | 1,042 |
| njutest::toolchain_matrix | 533.351 | 15 | 52 |
| rust-mutants-cli::toolchain_fates | 342.893 | 263 | 583 |
| njutest::toolchain_mutation | 294.305 | 112 | 414 |
| rust-mutants-cli::toolchain_run_report | 253.323 | 146 | 474 |

The baseline's slowest individual tests were the two whole-v1 faulted matrix dimensions, each about 256 seconds, followed by the proof-layer differential oracle at 81 seconds and the edit operator oracle at 72 seconds.
The const-initializer sealed-control test alone prepared 30 modules in 10.109 seconds while executing guests for 0.014 seconds and making 118 Cargo build calls.
The unchanged fixture-simple trace measured two bench modules in 723.633 milliseconds against 4.352 milliseconds of guest execution.

The separate-process reuse regression measured two cold module preparations in 701.601 milliseconds and two cache hits in 18.597 milliseconds, with zero compilation misses and no transcript-cache answers on the second run.
The identical-copy regression measured 12 fresh Cargo artifacts initially, zero on the next run, and zero for an identical copy; changing the copied manifest forced fresh artifacts again.

An intermediate complete run took 2,597.787 seconds for 4,435 tests and exposed six regressions that were corrected before establishing the final ledger.
They were a missing documentation status, nullable deadline values compared as different trace shapes, stale static Cargo outputs treated as runtime files, a private-cache GC test inheriting the shared test cache, and two existing compiler-call ceilings detecting an unnecessary second metadata query.
The compiler-call ceilings were preserved.
The final measurement starts with a fresh explicit fixture pool and its module-cache directory, and clears this checkout's separate compiled raw-guest module cache.
The already built workspace and the raw Rust guest's Wasm build cache are retained, as in the original baseline.
The complete suite includes the new uncached compiler-tier differential oracle, whose final summed test cost is 336.083 seconds.

The final complete suite passed all 4,439 executed tests, with one configured skip, in 2,530.329 seconds (42.172 minutes).
It includes 98 new tests and is 7.6% faster than the original 2,737.029-second suite.
Workspace compilation before nextest took 35.84 seconds before and 21.79 seconds after, reported separately from the suite wall time.
Across the same 805 toolchain tests, module requests stayed at 1,892 while preparations fell from 1,777 to 1,557, misses fell to 708 and 849 preparations hit compiled code.
Preparation time fell from 451.041 to 238.157 seconds, while guest execution took 6.026 seconds before and 4.710 seconds after.
Cargo calls were 6,598 before and 6,602 after, with fresh artifacts falling from 12,253 to 7,426 and Cargo elapsed time from 1,665.903 to 1,314.498 seconds.
The work is cheaper even where Cargo still checks the same build inputs.

| Slowest final binary | Before summed seconds | After summed seconds | After modules | After Cargo calls |
| --- | ---: | ---: | ---: | ---: |
| njutest::toolchain_matrix | 534.639 | 529.692 | 15 | 52 |
| njutest::toolchain_verify | 677.392 | 501.206 | 202 | 1,042 |
| rust-mutants-cli::toolchain_compiler_tiers | new | 336.083 | 424 | 501 |
| rust-mutants-cli::toolchain_fates | 339.610 | 309.677 | 253 | 583 |
| njutest::toolchain_mutation | 292.422 | 224.232 | 98 | 414 |
| rust-mutants-cli::toolchain_run_report | 248.059 | 177.946 | 119 | 476 |

The two slowest matrix tests still take 254.348 and 254.236 seconds, and the next costs are repository-claim derivation at 87.206 seconds and the native edit oracle at 69.177 seconds.
Across the complete final inventory, 2,006 module preparations produced 948 hits and 1,058 misses, taking 335.395 seconds against 11.134 seconds of guest execution; 7,185 Cargo calls reported 8,476 fresh artifacts in 1,488.124 seconds.
The gate's seven behavioral regressions and the final recorded-ledger check pass.
The count baseline saved 85 one-minute load samples ranging from 3.77 to 8.61, and the final complete run saved 24 ranging from 3.53 to 8.20.
The shared machine was not reserved, so the same command, concurrency and watcher state do not guarantee identical external load.
The original unmodified complete run has no saved load series.

The cancelled macOS 26 workflow spent about 67.95 minutes in the suite after about 7.3 minutes of setup on an older 4,326-test head.
Using 1.5–2 times the local final suite duration plus seven minutes of setup gives an estimated 70–91-minute hosted job, not a measured hosted result or a guarantee that every timeout is resolved.
Windows and coverage have no calibrated multiplier, and cold level-3 dependencies can add setup cost.
No hosted run was started because this task forbids pushing.

The before and after complete-suite command is:

```sh
export CARGO_BUILD_JOBS=6
mise x -- cargo nextest run --locked --workspace --all-targets --all-features \
  --no-fail-fast --test-threads 2 --config-file /private/tmp/njutest-h/nextest.toml
```

The external configuration copies the repository configuration and adds successful/failure JUnit output at `/private/tmp/njutest-h/suite.xml`.
After instrumentation, `NJUTEST_TEST_COST_DIR` additionally selects a fresh diagnostics directory.
The count-only baseline adds an exact inventory filter that selects `binary(/^toolchain_/)` and excludes only tests introduced by this change.
That measurement filter collects counts; the complete-suite wall-time comparisons do not filter or partition.
