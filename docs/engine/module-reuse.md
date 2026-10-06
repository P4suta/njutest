<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Module reuse

**Status: implemented.**

One explicit process-local owner prepares one module once.
The first request for a module's exact bytes performs the actual preparation.
Every later request through that owner, from the same runner or a separate compatible runner, uses the prepared module.
This page records the ownership the [`rust-mutants-sealed`](../../crates/rust-mutants-sealed) runner gives that work; [sealed execution](sealed.md) keeps the host and its judgements, and the meter below extends the `run-end.sealed` fields that page names.

## The owner

Every runner constructor requires a `&ModuleOwner`; `SealedRunner::with_compiler(modules, watchdog, tier, directory)` additionally selects the compiler tier and disk-cache directory.
Both CLI composition environments retain one cloneable owner and pass it through workspace opening, sessions, platform probes and runner construction.
An omitted runner ownership argument is a compile error.
No static registry or replaceable test state supplies this ownership.
The owner key is the semantic compiler tier under the pinned host and Wasmtime version.
Every other engine setting is a constant of the crate.
Operational cache paths select no engine setting and cannot split the owner's module memory.
The first compatible constructor selects its durable disk cache; later constructors reuse that engine before building any candidate.
The retained engine's configuration digest binds Wasmtime's actual compilation compatibility hash and the host's identity.

The context holds one compatible engine per that identity: its `Engine`, linked host, Wasmtime cache and prepared modules, keyed by a digest of each module's exact bytes.
Module memory survives a runner while its composition retains the owner.
A later compatible runner passed the same owner finds the prepared module.
There is no arbitrary cap or eviction.
The last context clone and runner releasing an engine also release its retained module memory.

The epoch alarm is the runners', not the memory's.
The runners alive on one engine share one alarm, which advances the epoch when a registered deadline falls or a watching `Raised` flag is raised, and it stops and joins when the last of them drops — an owned stop boundary, not a thread a static registry keeps alive — so dropping one runner stops nothing another runner uses, and a later runner starts the alarm again on the same retained engine.
A successor waits for the prior alarm's owned stop and join to settle before starting a new one.
An engine whose invocations all ended, or that never ran one, advances nothing: there is no periodic work.
Every invocation, held module or not, runs in a fresh `Store` with its own host authority, fuel, wall-clock watchdog, cancellation and transcript; nothing of one invocation reaches another.
An interrupt flag a signal handler may store to has no raise that can wake the alarm, so while such a raw flag is armed the alarm keeps one typed real-OS backstop wake, counted beside its advances; the product's own cancellation goes through `Raised`, whose raise wakes the alarm itself.

## The one preparation

A request for bytes already held is answered by the held module and performs no preparation work.
A request for bytes not held claims the digest's slot, and the claim is settled however the preparation ends: with the prepared module held for every compatible request, or with the slot released and every waiter woken, including where the preparation panics.
Concurrent cold requests for one module have one owner and complete on it by lock and wake, with no duplicate preparation, no sleep and no poll.
A failed preparation is never held: a refused module cannot become a reusable success, and a later request for the same bytes attempts the preparation again.
An unwind is caught inside the actual-preparation lock, that guard is released normally, and the panic is then resumed through the RAII claim.
A poisoned owner lock is a sticky typed host failure; no path recovers its protected state as a valid value.

One actual preparation runs at a time per engine.
The serialization is the meter's, not a throughput claim: Wasmtime's `Cache::cache_hits` is one counter for the whole cache, so the before-and-after observation that says whether a preparation hit the compiled code on the disk is exact only when one actual preparation at a time observes it — otherwise one request's cold compilation can be counted a disk hit while another digest's load happens inside its interval, and true cold work could grow while its count did not.
Across processes and independently created contexts, the durable cache's keyed preparation lease binds the module bytes and semantic engine configuration.
The lease file sits beside the cache directory because Wasmtime's worker removes unrecognized cache entries.
The lease lasts through safe `Module::new` loading and synchronous publication of compiled code.
A waiter observes the operating system's lock release, then loads the published code through Wasmtime.
Wasmtime's separate worker can still update statistics after that release or after the engine drops.
`CompilationCache` therefore requires a durable directory whose disposal follows actual producer process completion.
A temporary-cache control runs its producer in an owned subprocess, and its parent observes process completion and pipe EOF before disposing its directory.
In-process cache controls use `njutest_devkit::temporary::CacheDirectory` under the verified parent root supplied by `cargo xtask tidy`.
They remain owned by that parent until the entire producing command has ended.
The product cache selector uses that parent root, an explicitly supplied fixture-build cache or the supplied absolute user cache.
It never places Wasmtime under a session snapshot or temporary build directory.
A missing retained root returns RS1008 and retains the refusal.
The engine that compiles a module and the host that runs it stay the safe public Wasmtime interfaces; nothing deserializes native code, and the crate forbids `unsafe`.

## The meter

`Counted::prepared(duration, reuse)` records one answered request, where `Reuse` is `Cold` (an actual preparation compiled the module), `Disk` (Wasmtime's content-addressed cache held the compiled code, which the preparation loaded) or `Process` (the process's own held module answered, and the request performed no preparation work).
The recording's `Spent` preserves `compiles` as successful requests and records refused requests separately.
Logical requests are their sum.
Actual preparations are counted separately:

- `compilation.hits` — requests answered by compiled code already held, on Wasmtime's cache or by this process;
- `compilation.process` — of those, the ones this process's held module answered (absent where none did, as in older recordings);
- `compilation.misses` — requests that performed an actual preparation because no held code answered;
- `compilation.attempts` — actual `Module::new` preparation attempts, including ones that then failed (absent where none did);
- `compilation.failed_cold` and `compilation.failed_disk` — failed actual attempts by their owned cache observation; neither is a successful answer;
- `compilation.duration_ns` — the time those actual preparations took; a `Process` answer and a refusal before `Module::new` add nothing to it;
- `failures` — preparation requests that were refused, whether before `Module::new` (a shape refusal, which attempts nothing) or after it (a compilation, interface or linking refusal, whose `attempts` count and measured work stand).

Only the new diagnostic fields are optional and omitted when none were observed, so historical records read unchanged.
Existing successful-request hit and miss equations remain unchanged.
A refusal before `Module::new` adds no actual attempt or preparation duration; a later refusal retains measured work.
The legacy suite-cost reader continues to budget successful-request hits and misses; failed cold and disk work is directly observable in these additional diagnostics and is not claimed as a newly gated budget.
These measurements belong to diagnostics, never to transcript digests or evidence.

`Spent.modules` records actual requests under the same physical key as the preparation lease.
`preparation_key(module, configuration)` binds the byte digest and the semantic configuration digest, which includes the host identity and pinned Wasmtime version.
Each `ModuleWork` retains requests, actual attempts, successful cold/disk/process answers, refusals, failed cold/disk attempts and measured preparation nanoseconds.
Validation refusal records one request and failure with no invented attempt or elapsed work.
Historical absence remains `None`; it does not imply zero measured work.
Keyed updates hold one owned lock, check every recorded width and publish each observation whole.

## What this does not close

- A shared `ModuleOwner` answers compatible constructors across different operational cache directories with the same preparation.
- Separate process owners with separate disk caches have no shared preparation to observe; their physical attempts remain separately counted.
- An engine without a cache directory holds no domain to lease, so independently created contexts without one each compile their own bytes: `--no-cache` establishes each observation again by design.
- A lease a dead process held is released by the operating system when the process ends, and a preparation that waited for it still loads what the domain published or compiles afresh; the lease serializes, it does not publish.

A disk-cache hit still validates and links the module, while a process-memory answer adds no preparation; every invocation starts a fresh store and instance; compilation reuse does not answer an invocation or reuse a verdict, and `--no-cache` still establishes each observation again.

## Cancellation observations

`runner::Cancel::subscribe()` retains a cancellation subscription before its work starts.
`Cancelled::wait()` observes that event, and `wait_timeout` bounds the caller's own wait.
A parent cancellation wakes each subscribed child, including a subscription created later.
A child cancellation changes nothing in its parent.
`Cancel::interrupt()` carries those same events into the sealed epoch alarm.
Exposing a raw signal-handler flag retains the required raw-flag backstop too.


## Inherited receipt corrections

The B4 commit `3b319d9d` replaced two deprecated atomic spellings without changing their orderings or checked arithmetic.
Its deprecation Red used `nightly-2026-10-01`; the unchanged pinned `nightly-2026-07-02` passed before the replacement.
Both original receipts remain retained, and no toolchain pin changed.
The B1 commit `7dadcb30` originally partitioned owners by physical cache domains.
This correction removes that operational partition and supersedes its temporary-cache disposal assumptions.
No inherited commit or receipt is rewritten.
