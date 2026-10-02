<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Module reuse

**Status: implemented.** One explicit process-local owner prepares one module once.
The first request for a module's exact bytes performs the actual preparation.
Every later request through that owner, from the same runner or a separate compatible runner, uses the prepared module.
This page records the ownership the [`rust-mutants-sealed`](../../crates/rust-mutants-sealed) runner gives that work; [sealed execution](sealed.md) keeps the host and its judgements, and the meter below extends the `run-end.sealed` fields that page names.

## The owner

Every runner constructor requires a `&ModuleOwner`; `SealedRunner::with_compiler(modules, watchdog, tier, directory)` additionally selects the compiler tier and disk-cache directory.
Both CLI composition environments retain one cloneable owner and pass it through workspace opening, sessions, platform probes and runner construction.
An omitted runner ownership argument is a compile error.
No static registry or replaceable test state supplies this ownership.
Each constructor derives its key from an actual configured engine's `precompile_compatibility_hash`, encoded with the pinned Wasmtime 48.0.3 identity and the existing host configuration digest.
The configured disk-cache directory is part of the operational owner key.
A matching key retains the original engine and linker; candidate engine construction is still performed and is not a removed-work claim.
The context holds one compatible engine per that identity: its `Engine`, linked host, Wasmtime cache and prepared modules, keyed by a digest of each module's exact bytes.

Module memory survives a runner while its composition retains the owner.
A later compatible runner passed the same owner finds the prepared module.
There is no arbitrary cap or eviction.
The last context clone and runner releasing an engine also release its retained module memory.
A different compiler tier or cache directory selects a different owner — Wasmtime keys compiled code by the complete module bytes, compiler and target settings, tunables, features and its own version — and holds its own: a module prepared for one configuration is never handed to another.
A different cache directory is a different operational cache domain as well, and the same bytes asked of two such owners are prepared twice by design.

The epoch ticker is the runners', not the memory's.
The runners alive on one engine share one ticker, and it stops and joins when the last of them drops — an owned stop boundary, not a thread a static registry keeps alive — so dropping one runner stops nothing another runner uses, and a later runner starts the ticker again on the same retained engine.
A successor waits for the prior ticker's owned stop and join to settle before starting a new one.
Every invocation, held module or not, runs in a fresh `Store` with its own host authority, fuel, wall-clock watchdog, cancellation and transcript; nothing of one invocation reaches another.

## The one preparation

A request for bytes already held is answered by the held module and performs no preparation work.
A request for bytes not held claims the digest's slot, and the claim is settled however the preparation ends: with the prepared module held for every compatible request, or with the slot released and every waiter woken, including where the preparation panics.
Concurrent cold requests for one module have one owner and complete on it by lock and wake, with no duplicate preparation, no sleep and no poll.
A failed preparation is never held: a refused module cannot become a reusable success, and a later request for the same bytes attempts the preparation again.
An unwind is caught inside the actual-preparation lock, that guard is released normally, and the panic is then resumed through the RAII claim.
A poisoned owner lock is a sticky typed host failure; no path recovers its protected state as a valid value.

One actual preparation runs at a time per engine.
The serialization is the meter's, not a throughput claim: Wasmtime's `Cache::cache_hits` is one counter for the whole cache, so the before-and-after observation that says whether a preparation hit the compiled code on the disk is exact only when one actual preparation at a time observes it — otherwise one request's cold compilation can be counted a disk hit while another digest's load happens inside its interval, and true cold work could grow while its count did not.
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

## What this does not close

- Independently created owner contexts prepare the same bytes separately, even within one process.
Compatible runners reuse work when their composition passes the same owner.
- Two processes that prepare the same cold module at once each perform one preparation; the simultaneous cross-process cold owner needs a cross-process lease, which is a separate task this process-local owner does not pretend to close.
- Different configured cache-directory paths select separate operational owners and prepare the same bytes once each, including aliases spelled differently.
This boundary does not deduplicate those domains.
- The ticker is periodic; replacing the tick with an event-driven deadline is a separate task.

A disk-cache hit still validates and links the module, while a process-memory answer adds no preparation; every invocation starts a fresh store and instance; compilation reuse does not answer an invocation or reuse a verdict, and `--no-cache` still establishes each observation again.
