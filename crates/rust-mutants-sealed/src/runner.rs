// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The runner: one deterministic engine, a module validated and compiled once, and a fresh store and instance for every invocation.

use std::collections::BTreeMap;
use std::fs::TryLockError;
use std::hash::{Hash as _, Hasher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use wasmtime::{
    Cache, CacheConfig, Config, Engine, FuncType, InstancePre, Linker, Module, OptLevel, Store,
    Trap, UpdateDeadline, WasmFeatures,
};

use crate::cache::CompilationCache;
use crate::digest::{Encoder, SealedDigest};
use crate::error::{Invariant, RuntimeStep, SealedError};
use crate::host::{BYTE_FUEL, CALL_FUEL, Host, HostStop, RESOLUTION, TABLE_ELEMENTS};
use crate::imports::{IMPORT_MODULE, WasiFunction};
use crate::interrupt::Interrupt;
use crate::invocation::Invocation;
use crate::transcript::{KEPT_REQUESTS, Parts, SealedStop, Transcript, TrapClass, classify};
use crate::transcripts::Reuse;
use crate::validate;

/// The wasmtime every digest of this crate is taken under, which `Cargo.toml` pins exactly.
pub const WASMTIME_VERSION: &str = "48.0.3";

/// The suffix naming a cache domain's preparation leases, as a sibling of the cache directory itself: Wasmtime's cache worker removes anything inside its directory it does not recognize.
const PREPARATION_LEASES: &str = ".preparations-v1";

/// The compiler strategy, whose configuration is part of every module and transcript identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum CompilerTier {
    /// The established optimizing compiler.
    Optimized,
    /// The same compiler without optimization, subject to the fixture differential oracle.
    Unoptimized,
}

impl CompilerTier {
    /// The established tier, retained while the cheaper tier changes any complete observation.
    #[must_use]
    pub const fn faithful() -> Self {
        Self::Optimized
    }

    /// The Cranelift level corresponding to this tier.
    const fn level(self) -> OptLevel {
        match self {
            Self::Optimized => OptLevel::Speed,
            Self::Unoptimized => OptLevel::None,
        }
    }
}

/// The native stack a guest may use, in bytes.
const MAX_WASM_STACK: usize = 512 * 1024;

/// How long the alarm waits between looks at a raw interrupt flag, which no raise can wake: a signal handler may only store it.
/// This is the typed real-OS backstop, counted by the alarm, and it runs only while such a flag is armed.
const RAW_BACKSTOP: Duration = Duration::from_millis(10);

/// The WebAssembly proposals sealed execution leaves off: each is nondeterministic, or a shape the host does not run.
const REFUSED_FEATURES: WasmFeatures = WasmFeatures::RELAXED_SIMD
    .union(WasmFeatures::MEMORY64)
    .union(WasmFeatures::MULTI_MEMORY)
    .union(WasmFeatures::THREADS)
    .union(WasmFeatures::SHARED_EVERYTHING_THREADS)
    .union(WasmFeatures::CUSTOM_PAGE_SIZES)
    .union(WasmFeatures::WIDE_ARITHMETIC)
    .union(WasmFeatures::STACK_SWITCHING)
    .union(WasmFeatures::GC)
    .union(WasmFeatures::EXCEPTIONS)
    .union(WasmFeatures::LEGACY_EXCEPTIONS)
    .union(WasmFeatures::COMPONENT_MODEL);

/// What an engine's alarm holds: the deadlines of the invocations running on it, and which of them gave it a raw flag it cannot be woken about.
#[derive(Debug, Default)]
struct AlarmState {
    /// Set to stop the thread.
    stopped: bool,
    /// Whether a watching raise pinged the alarm since it last looked.
    pinged: bool,
    /// The deadlines of the invocations running on the engine.
    due: Vec<Instant>,
    /// One entry for each live invocation whose interrupt carries a raw flag.
    raw: Vec<RawInterrupt>,
}

/// A live invocation whose interrupt flag has no wake of its own.
#[derive(Debug, Clone, Copy)]
struct RawInterrupt;

/// The epoch alarm one engine's live runners share: it advances the epoch when a registered deadline falls or a watching raise pings it, and it wakes for nothing else.
#[derive(Debug, Default)]
pub(crate) struct Advances {
    state: Mutex<AlarmState>,
    /// Wakes the alarm when a deadline is registered or removed, a raise pings it, or it is stopped.
    changed: Condvar,
    /// How many times the alarm advanced the engine's epoch: a diagnostic, never part of a transcript or evidence.
    advanced: AtomicU64,
    /// How many of the alarm's wakes were its typed raw-flag backstop rather than an observed event.
    backstops: AtomicU64,
    /// A counter overflow permanently refuses answers from this alarm's engine.
    overflowed: AtomicBool,
}

impl Advances {
    /// Registers one invocation's deadline and interrupt, and says when to unregister.
    fn arm(advances: &Arc<Self>, deadline: Option<Instant>, interrupt: &Interrupt) -> Armed {
        interrupt.watch(advances);
        let raw = interrupt.raw();
        {
            let mut state = advances.lock();
            if let Some(deadline) = deadline {
                state.due.push(deadline);
            }
            if raw {
                state.raw.push(RawInterrupt);
            }
        }
        advances.changed.notify_all();
        Armed {
            advances: Arc::clone(advances),
            deadline,
            raw,
        }
    }

    /// Wakes the alarm: a watching flag was raised, so the engine's stores look at their interrupts.
    pub(crate) fn ping(&self) {
        self.lock().pinged = true;
        self.changed.notify_all();
    }

    /// Stops the alarm thread and waits for it.
    fn halt(&self) {
        self.lock().stopped = true;
        self.changed.notify_all();
    }

    /// Acquires the alarm state or terminates after an irrecoverable ownership failure.
    fn lock(&self) -> MutexGuard<'_, AlarmState> {
        match self.state.lock() {
            Ok(state) => state,
            Err(_poisoned) => {
                eprintln!(
                    "the sealed alarm state was poisoned; its registrations cannot be recovered"
                );
                std::process::abort();
            }
        }
    }

    /// Advances once after checked diagnostic accounting, waking guests even on a sticky width failure.
    fn advance(&self, engine: &Engine, backstop: bool) -> bool {
        let advanced = self
            .advanced
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                count.checked_add(1)
            });
        let counted = match advanced {
            Ok(_previous) => true,
            Err(_exhausted) => false,
        };
        let counted = if backstop {
            match self
                .backstops
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                    count.checked_add(1)
                }) {
                Ok(_previous) => counted,
                Err(_exhausted) => false,
            }
        } else {
            counted
        };
        if !counted {
            self.overflowed.store(true, Ordering::Release);
        }
        engine.increment_epoch();
        counted
    }

    /// How many times the alarm advanced the epoch, and how many of its wakes were the typed raw-flag backstop.
    fn counts(&self) -> (u64, u64) {
        if self.overflowed.load(Ordering::Acquire) {
            eprintln!(
                "the sealed alarm counter overflowed; no complete diagnostic count is available"
            );
            std::process::abort();
        }
        (
            self.advanced.load(Ordering::Relaxed),
            self.backstops.load(Ordering::Relaxed),
        )
    }

    /// Refuses a guest answer after any alarm accounting width failure.
    fn checked(&self) -> Result<(), SealedError> {
        if self.overflowed.load(Ordering::Acquire) {
            Err(broken(Invariant::Width))
        } else {
            Ok(())
        }
    }

    /// Installs this alarm's sticky failure, interrupt and watchdog checks before guest entry.
    fn watch_store(
        advances: &Arc<Self>,
        store: &mut Store<Host>,
        stopping: Interrupt,
        deadline: Option<Instant>,
    ) {
        store.set_epoch_deadline(1);
        let advances = Arc::clone(advances);
        store.epoch_deadline_callback(move |mut context| {
            if advances.checked().is_err() {
                context.data_mut().stop = Some(HostStop::Broken(Invariant::Width));
                return Err(wasmtime::Error::msg("the sealed alarm counter overflowed"));
            }
            if stopping.raised() {
                context.data_mut().stop = Some(HostStop::Interrupted);
                return Err(wasmtime::Error::msg("the guest was interrupted"));
            }
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                context.data_mut().stop = Some(HostStop::WatchdogExpired);
                return Err(wasmtime::Error::msg("the wall-clock watchdog expired"));
            }
            Ok(UpdateDeadline::Continue(1))
        });
    }
}

/// One invocation's registration on an engine's alarm, removed however the invocation ends.
#[derive(Debug)]
pub(crate) struct Armed {
    advances: Arc<Advances>,
    deadline: Option<Instant>,
    raw: bool,
}

impl Drop for Armed {
    fn drop(&mut self) {
        {
            let mut state = self.advances.lock();
            if let Some(deadline) = self.deadline
                && let Some(at) = state.due.iter().position(|due| *due == deadline)
            {
                state.due.swap_remove(at);
            }
            if self.raw {
                match state.raw.pop() {
                    Some(RawInterrupt) => {}
                    None => {
                        eprintln!("a sealed raw interrupt registration was lost before settlement");
                        std::process::abort();
                    }
                }
            }
        }
        self.advances.changed.notify_all();
    }
}

/// Runs one engine's alarm until it is halted: waiting for the next registered deadline, a raise's ping or the stop, advancing the epoch when one arrives.
fn alarm(engine: &Engine, advances: &Advances) {
    let mut state = advances.lock();
    loop {
        if state.stopped {
            drop(state);
            return;
        }
        let now = Instant::now();
        let due = state.due.iter().min().copied();
        let backstop = if state.raw.is_empty() {
            None
        } else {
            match now.checked_add(RAW_BACKSTOP) {
                Some(bound) => Some(bound),
                None => {
                    drop(state);
                    advances.overflowed.store(true, Ordering::Release);
                    engine.increment_epoch();
                    return;
                }
            }
        };
        let bound = match (due, backstop) {
            (Some(due), Some(backstop)) => Some(due.min(backstop)),
            (Some(earliest), None) | (None, Some(earliest)) => Some(earliest),
            (None, None) => None,
        }
        .map(|at| at.saturating_duration_since(now));
        let timed_out = match bound {
            Some(bound) => match advances.changed.wait_timeout(state, bound) {
                Ok((next, waited)) => {
                    state = next;
                    waited.timed_out()
                }
                Err(_poisoned) => {
                    eprintln!(
                        "the sealed alarm wait was poisoned; owned registrations are unsettled"
                    );
                    std::process::abort();
                }
            },
            None => match advances.changed.wait(state) {
                Ok(next) => {
                    state = next;
                    false
                }
                Err(_poisoned) => {
                    eprintln!(
                        "the sealed alarm wait was poisoned; owned registrations are unsettled"
                    );
                    std::process::abort();
                }
            },
        };
        if state.stopped {
            drop(state);
            return;
        }
        let now = Instant::now();
        let fell = state.due.iter().any(|due| *due <= now);
        let pinged = state.pinged;
        state.pinged = false;
        if fell || pinged {
            state.due.retain(|due| *due > now);
            if !advances.advance(engine, false) {
                drop(state);
                return;
            }
        } else if timed_out && !state.raw.is_empty() && !advances.advance(engine, true) {
            drop(state);
            return;
        }
    }
}

/// The alarm of one shared engine, which the last runner alive on that engine stops and joins, so no alarm thread outlives the runners whose stores stand on it.
#[derive(Debug)]
struct Ticking {
    /// The engine whose ticker slot stays occupied until stop and join complete.
    owner: Arc<Owner>,
    /// The alarm's shared state, which the live invocations of every runner on the engine arm.
    advances: Arc<Advances>,
    /// The thread, until the last runner sharing it drops this.
    alarm: Option<AlarmThread>,
}

/// The alarm thread of one [`Ticking`], joined by the last runner that shares it.
#[derive(Debug)]
struct AlarmThread {
    handle: Option<JoinHandle<()>>,
    advances: Arc<Advances>,
}

impl AlarmThread {
    /// Starts the alarm of `engine` on `advances`.
    fn start(engine: Engine, advances: Arc<Advances>) -> std::io::Result<Self> {
        let running = Arc::clone(&advances);
        let handle = std::thread::Builder::new()
            .name("rust-mutants-sealed-alarm".to_owned())
            .spawn(move || alarm(&engine, &running))?;
        Ok(Self {
            handle: Some(handle),
            advances,
        })
    }

    /// Stops the thread and waits for it.
    fn stop(&mut self) {
        self.advances.halt();
        if let Some(handle) = self.handle.take() {
            match handle.join() {
                Ok(()) => {}
                Err(panic) => {
                    drop(panic);
                    eprintln!("the owned sealed alarm panicked before settlement");
                    std::process::abort();
                }
            }
        }
    }
}

impl Drop for AlarmThread {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Drop for Ticking {
    fn drop(&mut self) {
        {
            let live = self.owner.ticking.lock();
            if let Some(mut alarm) = self.alarm.take() {
                alarm.stop();
            }
            match live {
                Ok(mut live) => {
                    *live = None;
                    drop(live);
                }
                Err(_poisoned) => {
                    eprintln!("the sealed ticker owner was poisoned before release");
                    std::process::abort();
                }
            }
        }
        self.owner.ticker_changed.notify_all();
    }
}

/// The requested compiler settings and operational disk-cache directory.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Identity {
    /// The Cranelift level the engine compiles at.
    tier: CompilerTier,
    /// The directory Wasmtime's compiled-module cache lives in, or none where the engine keeps no cache.
    directory: Option<CompilationCache>,
}

/// An owner key: the semantic engine configuration, which the tier alone selects because every other engine setting is a constant of this crate under one pinned Wasmtime, under the pinned host and Wasmtime; operational cache paths select no semantic setting.
#[derive(Debug, PartialEq, Eq)]
struct OwnerKey {
    tier: CompilerTier,
}

/// One engine compatible runners of an explicit module owner share, holding its modules prepared once.
#[derive(Debug)]
struct Owner {
    /// The deterministic engine.
    engine: Engine,
    /// Every function of the table, linked.
    linker: Linker<Host>,
    /// The digest of everything about the engine and the host that a transcript depends on.
    configuration: SealedDigest,
    /// Wasmtime's compiled-module cache, where the identity names one.
    cache: Option<Cache>,
    /// The modules this process prepared on the engine, by the digest of their exact bytes, one preparation at a time.
    slots: Mutex<BTreeMap<SealedDigest, Slot>>,
    /// Wakes every waiter for a slot of `slots` that changed.
    changed: Condvar,
    /// One actual preparation at a time, which owns the engine's whole observation of the shared cache counter.
    preparing: Mutex<()>,
    /// Where this domain's cross-process preparation leases live, under the cache directory Wasmtime's own files sit beside.
    leases: Option<PathBuf>,
    /// The alarm the engine's live runners share, which no runner outlives.
    ticking: Mutex<Option<Weak<Ticking>>>,
    /// Wakes a successor after the previous last-runner alarm has stopped and joined.
    ticker_changed: Condvar,
}

/// One module's place on an owner: held for every compatible request, or being prepared by one request.
#[derive(Debug)]
enum Slot {
    /// A preparation this process started, which no waiter may use yet.
    Preparing(Arc<Preparation>),
    /// The module one actual preparation made, for every compatible request.
    Held(Arc<Prepared>),
}

/// The identity of a live preparation claim, distinct from a waiter's later retry.
#[derive(Debug)]
struct Preparation;

/// Events at the owned preparation boundary, observable through immutable argument hooks.
#[derive(Clone, Copy)]
enum PreparationStage {
    /// The request is about to wait on an existing claim.
    Waiting,
    /// This operation has sampled the disk-cache counter before calling `Module::new`.
    Observed,
}

/// An argument hook for observing preparation events without replacing compilation.
struct PreparationHooks<F> {
    observe: F,
}

impl PreparationHooks<fn(PreparationStage)> {
    /// A preparation with no observer beyond its owned work meter.
    fn quiet() -> Self {
        Self {
            observe: |_stage| {},
        }
    }
}

/// A module one compatible engine prepared once, ready to instantiate, with what its one preparation cost.
struct Prepared {
    /// The module with the host linked.
    pre: InstancePre<Host>,
    /// Whether Wasmtime's cache held the compiled code when this module's one preparation ran.
    disk: bool,
    /// How long the one actual preparation of this module took.
    preparation: Duration,
}

impl std::fmt::Debug for Prepared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Prepared")
            .field("disk", &self.disk)
            .field("preparation", &self.preparation)
            .finish_non_exhaustive()
    }
}

/// One refused preparation: the refusal the caller is told, and the actual preparation work it had done when it failed, which is nothing where it was refused before an attempt began.
#[derive(Debug)]
struct Refused {
    /// The refusal.
    error: SealedError,
    /// The elapsed work of an attempt that had reached `Module::new`, or nothing where none did.
    attempted: Option<Attempt>,
}

/// The physical work and disk-cache observation of one actual preparation attempt.
#[derive(Debug)]
struct Attempt {
    duration: Duration,
    disk: bool,
}

/// Compatible engines retained by one explicit preparation owner.
type Owners = Vec<(OwnerKey, Arc<Owner>)>;

/// Explicit process-local compiled-module ownership, shared by compatible runners and retained by their composition.
#[derive(Debug, Clone, Default)]
pub struct ModuleOwner {
    owners: Arc<Mutex<Owners>>,
}

/// An owner's lock, or a sticky host failure where a panic interrupted its protected state.
fn held<T>(lock: std::sync::LockResult<T>) -> Result<T, SealedError> {
    lock.map_err(|poisoned| {
        drop(poisoned);
        broken(Invariant::ModuleOwnerPoisoned)
    })
}

/// The claim one request holds on a digest's slot while it performs the actual preparation, settled exactly once however the preparation ends: with the module held, with the slot released, or by its own drop where the preparation unwinds.
struct Claim<'owner> {
    /// The owner whose slot is claimed.
    owner: &'owner Owner,
    /// The digest of the bytes being prepared.
    digest: SealedDigest,
    /// The operation whose slot this claim settles.
    preparation: Arc<Preparation>,
    /// Whether the claim still owes a settlement.
    unsettled: bool,
}

impl Claim<'_> {
    /// Settles the claim with the prepared module held for every compatible request.
    fn held(mut self, prepared: &Arc<Prepared>) -> Result<(), SealedError> {
        self.settle(Some(Arc::clone(prepared)))
    }

    /// Settles the claim with the slot released, so a later request attempts the preparation again.
    fn released(mut self) -> Result<(), SealedError> {
        self.settle(None)
    }

    /// Settles the claim once: `held` names the module to keep, and every waiter is woken either way.
    fn settle(&mut self, prepared: Option<Arc<Prepared>>) -> Result<(), SealedError> {
        if !self.unsettled {
            return Ok(());
        }
        let mut slots = match held(self.owner.slots.lock()) {
            Ok(slots) => slots,
            Err(error) => {
                self.unsettled = false;
                self.owner.changed.notify_all();
                return Err(error);
            }
        };
        if !matches!(slots.get(&self.digest), Some(Slot::Preparing(preparation)) if Arc::ptr_eq(preparation, &self.preparation))
        {
            return Ok(());
        }
        match prepared {
            Some(prepared) => {
                slots.insert(self.digest, Slot::Held(prepared));
            }
            None => {
                slots.remove(&self.digest);
            }
        }
        self.unsettled = false;
        drop(slots);
        self.owner.changed.notify_all();
        Ok(())
    }
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        if let Err(error) = self.settle(None) {
            drop(error);
        }
    }
}

impl ModuleOwner {
    /// The compatible engine for `identity`, retained by this explicit owner for its runners, looking the owner up by its key before any engine is built.
    fn shared(&self, identity: &Identity) -> Result<Arc<Owner>, SealedError> {
        let key = OwnerKey {
            tier: identity.tier,
        };
        let mut owners = held(self.owners.lock())?;
        if let Some((_known, owner)) = owners.iter().find(|(known, _owner)| known == &key) {
            return Ok(Arc::clone(owner));
        }
        let owner = Arc::new(Owner::built(identity)?);
        owners.push((key, Arc::clone(&owner)));
        drop(owners);
        Ok(owner)
    }
}

impl Owner {
    /// Builds the engine of `identity`, its linked host, its configuration digest, its lease directory and its empty module memory.
    fn built(identity: &Identity) -> Result<Self, SealedError> {
        let canonical = identity.directory.as_ref().map(CompilationCache::directory);
        let cache = canonical
            .map(|directory| {
                let mut configuration = CacheConfig::new();
                configuration.with_directory(directory.to_path_buf());
                Cache::new(configuration)
            })
            .transpose()
            .map_err(|source| SealedError::Engine { source })?;
        let mut config = Config::new();
        config
            .cache(cache.clone())
            .consume_fuel(true)
            .epoch_interruption(true)
            .cranelift_nan_canonicalization(true)
            .cranelift_opt_level(identity.tier.level())
            .relaxed_simd_deterministic(true)
            .max_wasm_stack(MAX_WASM_STACK)
            .memory_init_cow(false)
            .wasm_features(REFUSED_FEATURES, false);
        let engine = Engine::new(&config).map_err(|source| SealedError::Engine { source })?;
        let linker = link(&engine)?;
        let configuration = configuration(&engine);
        let leases = canonical.map(|directory| {
            let name = match directory.file_name() {
                Some(name) => {
                    let mut name = name.to_os_string();
                    name.push(PREPARATION_LEASES);
                    name
                }
                None => std::ffi::OsString::from(format!("wasmtime-modules{PREPARATION_LEASES}")),
            };
            directory.with_file_name(name)
        });
        Ok(Self {
            engine,
            linker,
            configuration,
            cache,
            slots: Mutex::new(BTreeMap::new()),
            changed: Condvar::new(),
            preparing: Mutex::new(()),
            leases,
            ticking: Mutex::new(None),
            ticker_changed: Condvar::new(),
        })
    }

    /// The alarm this engine's live runners share, started where none is alive, so the thread lives exactly as long as the runners whose stores stand on it.
    fn ticking(self: &Arc<Self>) -> Result<Arc<Ticking>, SealedError> {
        let mut live = held(self.ticking.lock())?;
        while let Some(ticking) = live.as_ref() {
            if let Some(alive) = ticking.upgrade() {
                return Ok(alive);
            }
            live = held(self.ticker_changed.wait(live))?;
        }
        let advances = Arc::new(Advances::default());
        let ticking = Arc::new(Ticking {
            owner: Arc::clone(self),
            advances: Arc::clone(&advances),
            alarm: Some(
                AlarmThread::start(self.engine.clone(), advances)
                    .map_err(|source| SealedError::WatchdogUnavailable { source })?,
            ),
        });
        *live = Some(Arc::downgrade(&ticking));
        drop(live);
        Ok(ticking)
    }

    /// Claims or waits for these bytes, exposing boundary events through `hooks`.
    fn acquire_with_hooks(
        &self,
        digest: &SealedDigest,
        bytes: &[u8],
        hooks: &PreparationHooks<impl Fn(PreparationStage)>,
    ) -> Result<(Arc<Prepared>, Reuse), Refused> {
        let refused = |error| Refused {
            error,
            attempted: None,
        };
        let mut slots = held(self.slots.lock()).map_err(refused)?;
        let preparation = loop {
            match slots.get(digest) {
                Some(Slot::Held(prepared)) => {
                    return Ok((Arc::clone(prepared), Reuse::Process));
                }
                Some(Slot::Preparing(_preparation)) => {
                    (hooks.observe)(PreparationStage::Waiting);
                    slots = held(self.changed.wait(slots)).map_err(refused)?;
                }
                None => {
                    let preparation = Arc::new(Preparation);
                    slots.insert(*digest, Slot::Preparing(Arc::clone(&preparation)));
                    break preparation;
                }
            }
        };
        drop(slots);
        let claim = Claim {
            owner: self,
            digest: *digest,
            preparation,
            unsettled: true,
        };
        let answer = self.compile_with_hooks(digest, bytes, hooks);
        match answer {
            Ok(prepared) => {
                let prepared = Arc::new(prepared);
                claim.held(&prepared).map_err(|error| Refused {
                    error,
                    attempted: Some(Attempt {
                        duration: prepared.preparation,
                        disk: prepared.disk,
                    }),
                })?;
                let reuse = if prepared.disk {
                    Reuse::Disk
                } else {
                    Reuse::Cold
                };
                Ok((prepared, reuse))
            }
            Err(mut refused) => {
                if let Err(error) = claim.released() {
                    refused.error = error;
                }
                Err(refused)
            }
        }
    }

    /// Validates and compiles `bytes` on this engine, the one actual preparation a module's exact bytes get from this process, one at a time so the shared cache counter is observed by the preparation that moves it.
    fn compile_with_hooks(
        &self,
        digest: &SealedDigest,
        bytes: &[u8],
        hooks: &PreparationHooks<impl Fn(PreparationStage)>,
    ) -> Result<Prepared, Refused> {
        let preparing = held(self.preparing.lock()).map_err(|error| Refused {
            error,
            attempted: None,
        })?;
        let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.compile_owned(digest, bytes, hooks)
        }));
        drop(preparing);
        match answer {
            Ok(answer) => answer,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    /// Performs preparation under the caller's meter lock, which is released before any panic resumes.
    fn compile_owned(
        &self,
        digest: &SealedDigest,
        bytes: &[u8],
        hooks: &PreparationHooks<impl Fn(PreparationStage)>,
    ) -> Result<Prepared, Refused> {
        let started = Instant::now();
        validate::shape(bytes).map_err(|error| Refused {
            error,
            attempted: None,
        })?;
        (hooks.observe)(PreparationStage::Observed);
        let leased = self.leased(digest)?;
        let hits = self.cache.as_ref().map(Cache::cache_hits);
        let answer = (|| {
            let module = Module::new(&self.engine, bytes)
                .map_err(|source| SealedError::Compile { source })?;
            validate::interface(&module)?;
            self.linker
                .instantiate_pre(&module)
                .map_err(|source| SealedError::Link { source })
        })();
        let preparation = started.elapsed();
        drop(leased);
        let disk = self
            .cache
            .as_ref()
            .zip(hits)
            .is_some_and(|(cache, before)| cache.cache_hits() > before);
        match answer {
            Ok(pre) => Ok(Prepared {
                pre,
                disk,
                preparation,
            }),
            Err(error) => Err(Refused {
                error,
                attempted: Some(Attempt {
                    duration: preparation,
                    disk,
                }),
            }),
        }
    }

    /// Takes this module's keyed preparation lease for the cache domain, holding it while the preparation runs so exactly one preparation of these bytes compiles cold across the processes and contexts that share the domain.
    /// A lease another preparation holds is waited for through the operating system, which wakes this one when it is released; a preparation without a cache domain holds nothing.
    fn leased(&self, digest: &SealedDigest) -> Result<Lease, Refused> {
        let Some(root) = &self.leases else {
            return Ok(Lease(None));
        };
        let key = crate::preparation_key(digest, &self.configuration);
        let path = root.join(format!("{key}.lock"));
        let refused = |source: std::io::Error| Refused {
            error: SealedError::Preparation {
                path: path.clone(),
                source,
            },
            attempted: None,
        };
        std::fs::create_dir_all(root).map_err(refused)?;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(refused)?;
        match file.try_lock() {
            Ok(()) => Ok(Lease(Some(file))),
            Err(TryLockError::WouldBlock) => {
                file.lock().map_err(refused)?;
                Ok(Lease(Some(file)))
            }
            Err(TryLockError::Error(source)) => Err(refused(source)),
        }
    }
}

/// A held cross-process preparation lease, released when it is dropped.
#[derive(Debug)]
struct Lease(
    #[expect(
        dead_code,
        reason = "the file is held, not read: its lock is the lease"
    )]
    Option<std::fs::File>,
);

/// The engine sealed guests run on, shared by compatible runners of the explicit module owner.
#[derive(Debug)]
pub struct SealedRunner {
    /// The engine owner: the engine, the host linked on it and the modules this process holds on it.
    owner: Arc<Owner>,
    /// The engine's epoch alarm, alive exactly as long as the runners sharing it: this runner's own share, whose stop runs when the last sharing runner drops it.
    ticking: Arc<Ticking>,
    /// How long a guest may run by the wall clock before the watchdog stops it.
    watchdog: Duration,
    counted: crate::Counted,
}

impl SealedRunner {
    /// The runner, its watchdog stopping a guest `watchdog` after it starts.
    ///
    /// # Errors
    /// [`SealedError::Engine`] where wasmtime refuses the configuration, [`SealedError::Link`] where the host cannot be linked, [`SealedError::WatchdogUnavailable`] where its thread cannot start.
    pub fn new(modules: &ModuleOwner, watchdog: Duration) -> Result<Self, SealedError> {
        Self::with_compiler(modules, watchdog, CompilerTier::faithful(), None)
    }

    /// The default compiler with Wasmtime's content-addressed cache shared across processes.
    ///
    /// # Errors
    /// The cache, engine, linker or watchdog cannot be configured.
    pub fn cached(
        modules: &ModuleOwner,
        watchdog: Duration,
        directory: &CompilationCache,
    ) -> Result<Self, SealedError> {
        Self::with_compiler(modules, watchdog, CompilerTier::faithful(), Some(directory))
    }

    /// The compiler tier and disk-cache domain, sharing one engine and preparations with compatible runners of `modules`.
    ///
    /// # Errors
    /// The cache, engine, linker or watchdog cannot be configured.
    pub fn with_compiler(
        modules: &ModuleOwner,
        watchdog: Duration,
        tier: CompilerTier,
        directory: Option<&CompilationCache>,
    ) -> Result<Self, SealedError> {
        let owner = modules.shared(&Identity {
            tier,
            directory: directory.cloned(),
        })?;
        let ticking = owner.ticking()?;
        Ok(Self {
            owner,
            ticking,
            watchdog,
            counted: crate::Counted::default(),
        })
    }

    /// Prepares a module, recording the request, its answer and any failure on the run's counters.
    ///
    /// # Errors
    /// The validation, compilation and linking failures of [`Self::prepare`], or a count overflow.
    pub fn prepare_counted<'runner>(
        &'runner self,
        bytes: &[u8],
        counted: &crate::Counted,
    ) -> Result<SealedModule<'runner>, SealedError> {
        self.obtain(bytes, Some(counted))
    }

    /// The work performed by this runner, without transcript-cache answers.
    #[must_use]
    pub fn spent(&self) -> Option<crate::Spent> {
        self.counted.spent()
    }

    /// The digest of everything about the engine and the host a transcript depends on.
    #[must_use]
    pub fn configuration(&self) -> &SealedDigest {
        &self.owner.configuration
    }

    /// How many times the engine's alarm advanced the epoch, and how many of its wakes were the typed backstop for a raw interrupt flag rather than an observed event.
    /// A diagnostic of the alarm's own work, never part of a transcript or evidence.
    #[must_use]
    pub fn alarm_advances(&self) -> (u64, u64) {
        self.ticking.advances.counts()
    }

    /// Validates and compiles `bytes` once, for as many invocations as are asked of it.
    ///
    /// # Errors
    /// A module that is not a core WASI command with one plain memory importing only the table, or one wasmtime refuses to compile or link.
    pub fn prepare(&self, bytes: &[u8]) -> Result<SealedModule<'_>, SealedError> {
        self.obtain(bytes, None)
    }

    /// Prepares `bytes`, recording the request, its answer and any refusal on this runner's counters and `run`'s.
    fn obtain(
        &self,
        bytes: &[u8],
        run: Option<&crate::Counted>,
    ) -> Result<SealedModule<'_>, SealedError> {
        self.obtain_with_hooks(bytes, run, &PreparationHooks::quiet())
    }

    /// Prepares and meters a request with immutable preparation event hooks.
    fn obtain_with_hooks(
        &self,
        bytes: &[u8],
        run: Option<&crate::Counted>,
        hooks: &PreparationHooks<impl Fn(PreparationStage)>,
    ) -> Result<SealedModule<'_>, SealedError> {
        self.counted.assembled();
        let digest = SealedDigest::of(bytes);
        let answer = self.owner.acquire_with_hooks(&digest, bytes, hooks);
        match answer {
            Ok((prepared, reuse)) => {
                let worked = match reuse {
                    Reuse::Process => Duration::ZERO,
                    Reuse::Cold | Reuse::Disk => prepared.preparation,
                };
                self.counted
                    .prepared_module((&digest, self.configuration()), worked, reuse)?;
                if let Some(run) = run {
                    run.prepared_module((&digest, self.configuration()), worked, reuse)?;
                }
                Ok(SealedModule {
                    preparation: prepared.preparation,
                    reuse,
                    runner: self,
                    pre: prepared.pre.clone(),
                    digest,
                })
            }
            Err(refused) => {
                let attempted = refused.attempted.map(|work| (work.duration, work.disk));
                self.counted
                    .failed_module(&digest, self.configuration(), attempted)?;
                if let Some(run) = run {
                    run.failed_module(&digest, self.configuration(), attempted)?;
                }
                Err(refused.error)
            }
        }
    }
}

/// Links every function of the table through the one match that carries each out.
fn link(engine: &Engine) -> Result<Linker<Host>, SealedError> {
    let mut linker = Linker::new(engine);
    for function in WasiFunction::ALL {
        let signature =
            FuncType::try_new(engine, function.parameter_types(), function.result_types())
                .map_err(|source| SealedError::Link {
                    source: wasmtime::Error::new(source),
                })?;
        linker
            .func_new(
                IMPORT_MODULE,
                function.name(),
                signature,
                move |caller, params, results| crate::host::call(function, caller, params, results),
            )
            .map_err(|source| SealedError::Link { source })?;
    }
    Ok(linker)
}

/// The digest of everything about `engine` and the host a transcript depends on.
fn configuration(engine: &Engine) -> SealedDigest {
    let mut encoder = Encoder::new("rust-mutants-sealed/configuration/v2");
    encoder
        .text(env!("CARGO_PKG_VERSION"))
        .text(WASMTIME_VERSION)
        .number(CALL_FUEL)
        .number(BYTE_FUEL)
        .number(RESOLUTION)
        .count(TABLE_ELEMENTS)
        .count(KEPT_REQUESTS);
    engine
        .precompile_compatibility_hash()
        .hash(&mut Fingerprint(&mut encoder));
    encoder.finish()
}

/// A hasher that feeds everything hashed into an encoding, so wasmtime's own account of its compilation settings becomes part of a digest.
struct Fingerprint<'encoder>(&'encoder mut Encoder);

impl Hasher for Fingerprint<'_> {
    fn finish(&self) -> u64 {
        self.0.peek().leading_number()
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0.bytes(bytes);
    }
}

/// A module validated and compiled once by a runner, every invocation of it a fresh store and instance.
pub struct SealedModule<'runner> {
    preparation: Duration,
    reuse: Reuse,
    /// The runner that prepared it.
    runner: &'runner SealedRunner,
    /// The module with the host linked, ready to instantiate.
    pre: InstancePre<Host>,
    /// The digest of the module's bytes.
    digest: SealedDigest,
}

impl std::fmt::Debug for SealedModule<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SealedModule")
            .field("digest", &self.digest)
            .field("configuration", &self.runner.owner.configuration)
            .finish_non_exhaustive()
    }
}

impl SealedModule<'_> {
    /// The digest of the module's bytes.
    #[must_use]
    pub const fn digest(&self) -> &SealedDigest {
        &self.digest
    }

    /// How the preparation request that returned this module was answered.
    #[must_use]
    pub const fn reuse(&self) -> Reuse {
        self.reuse
    }

    /// How long the one actual preparation of this module's bytes took.
    #[must_use]
    pub const fn preparation(&self) -> Duration {
        self.preparation
    }

    /// The digest of everything about the engine and the host that a transcript of an invocation of this module depends on.
    #[must_use]
    pub fn configuration(&self) -> &SealedDigest {
        self.runner.configuration()
    }

    /// Runs the module's `_start` once in a fresh instance, as `invocation` says, until it ends or `interrupt` is raised.
    ///
    /// # Errors
    /// [`SealedError::WatchdogExpired`] where the wall clock ran out, [`SealedError::Interrupted`] where `interrupt` was raised, and any failure of the runtime or the host that is not an answer about the guest.
    pub fn invoke(
        &self,
        invocation: &Invocation,
        interrupt: &Interrupt,
    ) -> Result<Transcript, SealedError> {
        self.invoke_counted(
            invocation,
            interrupt,
            &crate::transcripts::Counted::default(),
        )
    }

    /// Runs one invocation as [`Self::invoke`] does, counting the instance when it starts.
    ///
    /// # Errors
    /// The same runtime, interruption and watchdog failures as [`Self::invoke`].
    pub fn invoke_counted(
        &self,
        invocation: &Invocation,
        interrupt: &Interrupt,
        counted: &crate::transcripts::Counted,
    ) -> Result<Transcript, SealedError> {
        self.runner.ticking.advances.checked()?;
        if interrupt.raised() {
            return Err(SealedError::Interrupted);
        }
        let began = Instant::now();
        let digest = invocation.digest(&self.digest, &self.runner.owner.configuration);
        let watchdog = self.runner.watchdog;
        let deadline = Some(
            began
                .checked_add(watchdog)
                .ok_or_else(|| broken(Invariant::Width))?,
        );
        let armed = Advances::arm(&self.runner.ticking.advances, deadline, interrupt);
        let halt = invocation.halting()?;
        let host = Host::new((invocation, halt), (deadline, interrupt.clone())).map_err(broken)?;
        let mut store = Store::new(&self.runner.owner.engine, host);
        store.limiter(|host| &mut host.limiter);
        store
            .set_fuel(invocation.fuel)
            .map_err(|source| runtime(RuntimeStep::Fuel, source))?;
        Advances::watch_store(
            &self.runner.ticking.advances,
            &mut store,
            interrupt.clone(),
            deadline,
        );
        self.runner.ticking.advances.checked()?;
        self.runner.counted.instantiated()?;
        counted.instantiated()?;
        let stop = match self.pre.instantiate(&mut store) {
            Ok(instance) => {
                let memory = instance.get_memory(&mut store, "memory").ok_or(
                    SealedError::HostInvariant {
                        invariant: Invariant::Memory,
                    },
                )?;
                store.data_mut().memory = Some(memory);
                started(
                    (&instance, memory),
                    &mut store,
                    invocation.preopens.start(),
                    watchdog,
                )?
            }
            Err(source) => stopped(
                store.data(),
                (RuntimeStep::Instantiation, Err(source)),
                watchdog,
            )?,
        };
        let left = store
            .get_fuel()
            .map_err(|source| runtime(RuntimeStep::Fuel, source))?;
        let fuel_spent = invocation
            .fuel
            .checked_sub(left)
            .ok_or(SealedError::HostInvariant {
                invariant: Invariant::Width,
            })?;
        let ended = store.into_data().end().map_err(broken)?;
        drop(armed);
        self.runner.ticking.advances.checked()?;
        let duration = began.elapsed();
        self.runner.counted.executed(duration)?;
        counted.executed(duration)?;
        Ok(Transcript::seal(Parts {
            invocation: digest,
            stop,
            fuel_spent,
            peak_memory: ended.peak_memory,
            stdout: ended.stdout,
            stderr: ended.stderr,
            refusals: ended.refusals,
            denials: ended.denials,
            waited: ended.waited,
            overlay: ended.overlay,
        }))
    }
}

/// The guest's stop from `_start`, entered first through its own `chdir` where `start` names a directory to start in.
fn started(
    (instance, memory): (&wasmtime::Instance, wasmtime::Memory),
    store: &mut Store<Host>,
    start: Option<&str>,
    watchdog: Duration,
) -> Result<SealedStop, SealedError> {
    let entered = match start {
        Some(path) => enter((instance, memory), store, path)?,
        None => Ok(()),
    };
    match entered {
        Ok(()) => {
            let begin = instance
                .get_typed_func::<(), ()>(&mut *store, "_start")
                .map_err(|source| runtime(RuntimeStep::Entry, source))?;
            let called = begin.call(&mut *store, ());
            stopped(store.data(), (RuntimeStep::Entry, called), watchdog)
        }
        Err(stopped_early) => stopped(
            store.data(),
            (RuntimeStep::Start, Err(stopped_early)),
            watchdog,
        ),
    }
}

/// Enters `path` as the guest's working directory through its own `chdir`, given the path in memory its own `malloc` made, before `_start`, as cargo starts a test in its package's directory: the calls spend the guest's fuel like any of its own code, and where one of them stops the guest, that stop is answered.
///
/// # Errors
/// [`SealedError::StartUnexported`] for a module without the exports, and [`SealedError::StartRefused`] where the guest's `malloc` gives nothing or its `chdir` refuses the path.
fn enter(
    (instance, memory): (&wasmtime::Instance, wasmtime::Memory),
    store: &mut Store<Host>,
    path: &str,
) -> Result<wasmtime::Result<()>, SealedError> {
    let malloc = instance
        .get_typed_func::<i32, i32>(&mut *store, "malloc")
        .map_err(|_absent| SealedError::StartUnexported { export: "malloc" })?;
    let chdir = instance
        .get_typed_func::<i32, i32>(&mut *store, "chdir")
        .map_err(|_absent| SealedError::StartUnexported { export: "chdir" })?;
    let refused = || SealedError::StartRefused {
        path: path.to_owned(),
    };
    let mut bytes = path.as_bytes().to_vec();
    bytes.push(0);
    let len = i32::try_from(bytes.len()).map_err(|_wide| refused())?;
    let at = match malloc.call(&mut *store, len) {
        Ok(0) => return Err(refused()),
        Ok(at) => at,
        Err(stopped_early) => return Ok(Err(stopped_early)),
    };
    let offset = usize::try_from(at.cast_unsigned()).map_err(|_wide| refused())?;
    memory
        .write(&mut *store, offset, &bytes)
        .map_err(|_outside| refused())?;
    match chdir.call(&mut *store, at) {
        Ok(0) => Ok(Ok(())),
        Ok(_refused) => Err(refused()),
        Err(stopped_early) => Ok(Err(stopped_early)),
    }
}

/// How the guest stopped during `step`, instantiation included since its compiled startup spends the guest's fuel, from what the host recorded first and what wasmtime said after.
fn stopped(
    host: &Host,
    (step, called): (RuntimeStep, wasmtime::Result<()>),
    watchdog: Duration,
) -> Result<SealedStop, SealedError> {
    if host.limiter.overflowed {
        return Err(broken(Invariant::Width));
    }
    match host.stop {
        Some(HostStop::Exited { code }) => return Ok(SealedStop::Exited { code }),
        Some(HostStop::FuelExhausted) => return Ok(SealedStop::FuelExhausted),
        Some(HostStop::Halted) => return Ok(SealedStop::Halted),
        Some(HostStop::WatchdogExpired) => {
            return Err(SealedError::WatchdogExpired { limit: watchdog });
        }
        Some(HostStop::Interrupted) => return Err(SealedError::Interrupted),
        Some(HostStop::Broken(invariant)) => return Err(broken(invariant)),
        None => {}
    }
    let error = match called {
        Ok(()) => return Ok(SealedStop::Returned),
        Err(error) => error,
    };
    if host.limiter.reservation_failed {
        return Err(runtime(RuntimeStep::Memory, error));
    }
    let Some(trap) = error.downcast_ref::<Trap>().copied() else {
        return match step {
            RuntimeStep::Instantiation if host.limiter.denials.memory() > 0 => {
                Ok(SealedStop::MemoryExhausted)
            }
            RuntimeStep::Instantiation
            | RuntimeStep::Start
            | RuntimeStep::Entry
            | RuntimeStep::Fuel
            | RuntimeStep::Memory => Err(runtime(step, error)),
        };
    };
    match classify(trap) {
        Some(TrapClass::OutOfFuel) => Ok(SealedStop::FuelExhausted),
        Some(TrapClass::Interrupt) => Err(SealedError::WatchdogExpired { limit: watchdog }),
        Some(TrapClass::Kind(kind)) => {
            if host.limiter.denials.memory() > 0 {
                Ok(SealedStop::MemoryExhausted)
            } else {
                Ok(SealedStop::Trapped { kind })
            }
        }
        None => Err(SealedError::TrapUnclassified {
            trap: trap.to_string(),
        }),
    }
}

/// The error for a failure of the runtime during `during`.
const fn runtime(during: RuntimeStep, source: wasmtime::Error) -> SealedError {
    SealedError::Runtime { during, source }
}

/// The error for an invariant of the host that did not hold.
const fn broken(invariant: Invariant) -> SealedError {
    SealedError::HostInvariant { invariant }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;
    use std::sync::TryLockError;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;

    use std::sync::{Arc, Weak};
    use std::time::{Duration, Instant};

    use super::{
        CompilerTier, Identity, ModuleOwner, Owner, PREPARATION_LEASES, PreparationHooks,
        PreparationStage, RAW_BACKSTOP, SealedRunner, Slot,
    };
    use crate::error::Invariant;
    use crate::{Interrupt, Invocation, Reuse, SealedDigest, SealedStop};
    use njutest_devkit::temporary::CacheDirectory;

    /// A durable test cache outside every temporary owner, collected only after its producer process ends.
    fn cache_for(named: &std::path::Path) -> crate::CompilationCache {
        crate::CompilationCache::retained(named.to_path_buf())
            .expect("the parent-owned compilation cache")
    }

    /// A test's owned synchronization state, whose failure is a setup failure.
    fn held<T>(lock: std::sync::LockResult<T>) -> T {
        super::held(lock).expect("the owner lock is intact")
    }

    /// A WASI command that prints its exact module marker and returns.
    fn command(mark: &str) -> Vec<u8> {
        wat::parse_str(format!(
            r#"(module
                (import "wasi_snapshot_preview1" "fd_write"
                    (func $write (param i32 i32 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 100) "{mark}")
                (func (export "_start")
                    (i32.store (i32.const 0) (i32.const 100))
                    (i32.store (i32.const 4) (i32.const {}))
                    (drop (call $write (i32.const 1) (i32.const 0)
                        (i32.const 1) (i32.const 8)))))"#,
            mark.len()
        ))
        .expect("the WAT is valid")
    }

    /// An invocation with no preopens and independent host state.
    fn invocation() -> Invocation {
        Invocation {
            arguments: crate::Arguments::new(vec!["command".to_owned()]).expect("valid"),
            environment: crate::Environment::new(Vec::new()).expect("valid"),
            preopens: crate::Preopens::new(Vec::new()).expect("valid"),
            seed: 11,
            fuel: 1_000_000,
            limits: crate::Limits {
                memory: 1 << 20,
                stdout: 1 << 16,
                stderr: 1 << 16,
                overlay: 1 << 20,
            },
            clock: crate::ClockPolicy {
                realtime_origin: 1,
                monotonic_origin: 1,
                nanos_per_fuel: NonZeroU64::MIN,
            },
            halt: None,
        }
    }

    #[test]
    fn the_last_runner_stops_and_joins_its_shared_ticker() {
        let modules = ModuleOwner::default();
        let directory =
            CacheDirectory::make("sealed-module-").expect("a parent-owned cache directory");
        let first = SealedRunner::cached(
            &modules,
            Duration::from_secs(60),
            &cache_for(directory.path()),
        )
        .expect("the first runner");
        let last = SealedRunner::cached(
            &modules,
            Duration::from_secs(60),
            &cache_for(directory.path()),
        )
        .expect("the compatible runner");
        let ticker = Weak::clone(
            held(last.owner.ticking.lock())
                .as_ref()
                .expect("the live ticker slot"),
        );
        let first_ticker = Weak::clone(
            held(first.owner.ticking.lock())
                .as_ref()
                .expect("the first ticker slot"),
        );
        assert!(
            Weak::ptr_eq(&first_ticker, &ticker),
            "one ticker serves both runners"
        );
        let owner = Arc::clone(&last.owner);
        drop(first);
        assert!(
            ticker.upgrade().is_some(),
            "the remaining watchdog still has its ticker"
        );
        drop(last);
        assert!(
            ticker.upgrade().is_none(),
            "the last ticker owner was dropped"
        );
        assert!(
            held(owner.ticking.lock()).is_none(),
            "stop and join settled the ticker slot"
        );
        let successor = SealedRunner::cached(
            &modules,
            Duration::from_secs(60),
            &cache_for(directory.path()),
        )
        .expect("a later runner restarts the ticker");
        assert!(
            Arc::ptr_eq(&owner, &successor.owner),
            "the module engine outlives the ticker"
        );
        let transcript = successor
            .prepare(&command("successor"))
            .expect("a real succeeding module")
            .invoke(&invocation(), &Interrupt::of(Vec::new()))
            .expect("a fresh host execution");
        assert_eq!(transcript.stop(), SealedStop::Returned);
        assert_eq!(transcript.stdout().bytes(), b"successor");
        let released = Arc::downgrade(&owner);
        drop(owner);
        drop(successor);
        drop(modules);
        assert!(
            released.upgrade().is_none(),
            "the explicit context releases its engine and module memory"
        );
    }

    #[test]
    fn a_panicking_preparation_releases_its_claim_and_owned_waiters() {
        let modules = ModuleOwner::default();
        let directory =
            CacheDirectory::make("sealed-module-").expect("a parent-owned cache directory");
        let runner = SealedRunner::cached(
            &modules,
            Duration::from_secs(60),
            &cache_for(directory.path()),
        )
        .expect("the runner");
        let bytes = command("recovered");
        let digest = SealedDigest::of(&bytes);
        let (observed, observation) = mpsc::sync_channel(0);
        let (release, released) = mpsc::sync_channel(0);
        let (waiting, waiter_entered) = mpsc::sync_channel(0);
        let (stranded, answer) = std::thread::scope(|scope| {
            let runner = &runner;
            let bytes = &bytes;
            let panicking = njutest_devkit::thread::ScopedThread::launch(scope, move || {
                let hooks = PreparationHooks {
                    observe: |stage| match stage {
                        PreparationStage::Observed => {
                            observed.send(()).expect("the owner event is observed");
                            released.recv().expect("the owner is released");
                            panic!("injected panic at the actual preparation boundary");
                        }
                        PreparationStage::Waiting => {}
                    },
                };
                runner.obtain_with_hooks(bytes, None, &hooks)
            });
            observation
                .recv()
                .expect("the real owner reached preparation");
            let pending = match held(runner.owner.slots.lock()).get(&digest) {
                Some(Slot::Preparing(pending)) => Arc::clone(pending),
                Some(Slot::Held(_)) | None => panic!("the real claim is preparing"),
            };
            let waiter = njutest_devkit::thread::ScopedThread::launch(scope, move || {
                let hooks = PreparationHooks {
                    observe: |stage| match stage {
                        PreparationStage::Waiting => {
                            waiting.send(()).expect("the waiter event is observed");
                        }
                        PreparationStage::Observed => {}
                    },
                };
                runner
                    .obtain_with_hooks(bytes, None, &hooks)
                    .and_then(|module| module.invoke(&invocation(), &Interrupt::of(Vec::new())))
            });
            waiter_entered
                .recv()
                .expect("the owned waiter reached the keyed wait");
            release.send(()).expect("the owner is allowed to unwind");
            let panicked = panicking.join().is_err();
            let stranded = {
                let mut slots = held(runner.owner.slots.lock());
                let stranded = matches!(slots.get(&digest), Some(Slot::Preparing(live)) if Arc::ptr_eq(live, &pending));
                if stranded {
                    slots.remove(&digest);
                }
                drop(slots);
                if stranded {
                    runner.owner.changed.notify_all();
                }
                stranded
            };
            let answer = waiter.join().expect("the owned waiter is joined");
            assert!(panicked, "the injected owner panic is caught and joined");
            (stranded, answer)
        });
        let transcript = answer.expect("the waiter actually prepares and executes the guest");
        assert_eq!(transcript.stop(), SealedStop::Returned);
        assert_eq!(transcript.stdout().bytes(), b"recovered");
        assert!(
            !stranded,
            "the panicking owner left its original claim Preparing"
        );
    }

    /// Completes a real different-module disk load inside an unowned observation, or lets the cold owner finish first.
    fn overlap_if_unowned(owner: &Owner, start: &mpsc::SyncSender<()>, done: &mpsc::Receiver<()>) {
        let exclusive = match owner.preparing.try_lock() {
            Ok(guard) => {
                drop(guard);
                false
            }
            Err(TryLockError::WouldBlock) => true,
            Err(TryLockError::Poisoned(_poisoned)) => panic!("no operation has panicked"),
        };
        start
            .send(())
            .expect("the different module requests preparation");
        if !exclusive {
            done.recv()
                .expect("the disk load completes inside the unowned observation");
        }
    }

    /// Publishes real compiled code through a compatible temporary engine before the concurrent disk control.
    fn warm_disk(identity: &Identity, bytes: &[u8]) {
        let warming = Owner::built(identity).expect("the disk-warming engine");
        let digest = SealedDigest::of(bytes);
        let prepared = warming
            .compile_with_hooks(&digest, bytes, &PreparationHooks::quiet())
            .expect("actual preparation publishes disk code synchronously");
        assert!(!prepared.disk, "the warming preparation is truly cold");
    }

    #[test]
    fn concurrent_cold_and_disk_warm_modules_have_exact_work_attribution() {
        let modules = ModuleOwner::default();
        let directory = CacheDirectory::make("sm-").expect("cache");
        let identity = Identity {
            tier: CompilerTier::faithful(),
            directory: Some(cache_for(directory.path())),
        };
        let cold = command("cold");
        let warm = command("disk");
        warm_disk(&identity, &warm);
        let runner = SealedRunner::cached(
            &modules,
            Duration::from_secs(60),
            &cache_for(directory.path()),
        )
        .expect("the runner");
        let (start_warm, warm_requested) = mpsc::sync_channel(0);
        let (done, warm_done) = mpsc::sync_channel(1);
        let (cold_answer, warm_answer) = std::thread::scope(|scope| {
            let runner = &runner;
            let warm = &warm;
            let warming = njutest_devkit::thread::ScopedThread::launch(scope, move || {
                warm_requested.recv().expect("the cold counter was sampled");
                let answer = runner.prepare(warm).and_then(|module| {
                    let reuse = module.reuse();
                    module
                        .invoke(&invocation(), &Interrupt::of(Vec::new()))
                        .map(|transcript| (reuse, transcript))
                });
                done.send(()).expect("the warm completion has an owner");
                answer
            });
            let hooks = PreparationHooks {
                observe: |stage| match stage {
                    PreparationStage::Observed => {
                        overlap_if_unowned(&runner.owner, &start_warm, &warm_done);
                    }
                    PreparationStage::Waiting => {}
                },
            };
            let cold_answer = runner
                .obtain_with_hooks(&cold, None, &hooks)
                .and_then(|module| {
                    let reuse = module.reuse();
                    module
                        .invoke(&invocation(), &Interrupt::of(Vec::new()))
                        .map(|transcript| (reuse, transcript))
                });
            let warm_answer = warming
                .join()
                .expect("the concurrent warm worker is joined");
            (cold_answer, warm_answer)
        });
        let (cold_reuse, cold_transcript) = cold_answer.expect("actual cold guest execution");
        let (warm_reuse, warm_transcript) = warm_answer.expect("actual disk-warm guest execution");
        assert_eq!(cold_transcript.stop(), SealedStop::Returned);
        assert_eq!(cold_transcript.stdout().bytes(), b"cold");
        assert_eq!(warm_transcript.stop(), SealedStop::Returned);
        assert_eq!(warm_transcript.stdout().bytes(), b"disk");
        let spent = runner.spent().expect("the requests are measured");
        eprintln!(
            "attribution configuration={} cold={} warm={} spent={spent:?}",
            runner.configuration(),
            SealedDigest::of(&cold),
            SealedDigest::of(&warm)
        );
        assert_eq!(
            warm_reuse,
            Reuse::Disk,
            "the actual disk cache answered the warm module"
        );
        assert_eq!(
            cold_reuse,
            Reuse::Cold,
            "another digest's disk hit must not erase actual cold work"
        );
        let compilation = spent.compilation.expect("actual preparation is measured");
        assert_eq!(
            (compilation.hits, compilation.misses, compilation.attempts),
            (1, 1, Some(2))
        );
    }

    /// An alias of one cache directory is the same operational domain, so two runners spelled through it share one owner and prepare the bytes once.
    #[test]
    fn aliases_of_one_cache_directory_are_one_owner_and_one_preparation() {
        let modules = ModuleOwner::default();
        let directory =
            CacheDirectory::make("sealed-module-").expect("a parent-owned cache directory");
        let name = directory
            .path()
            .file_name()
            .expect("a temporary cache directory has a name")
            .to_owned();
        let alias = directory.path().join("..").join(&name);
        let spelled = SealedRunner::cached(
            &modules,
            Duration::from_secs(60),
            &cache_for(directory.path()),
        )
        .expect("the runner through the given spelling");
        let aliased = SealedRunner::cached(&modules, Duration::from_secs(60), &cache_for(&alias))
            .expect("the runner through the alias");
        assert!(
            Arc::ptr_eq(&spelled.owner, &aliased.owner),
            "two spellings of one physical cache directory retain one compatible owner, so no candidate engine or linker is built for the alias"
        );
        let bytes = command("aliased");
        let first = spelled.prepare(&bytes).expect("the first preparation");
        let second = aliased.prepare(&bytes).expect("the aliased preparation");
        assert_eq!(first.reuse(), Reuse::Cold, "the first preparation is cold");
        assert_eq!(
            second.reuse(),
            Reuse::Process,
            "the alias answers from the one prepared module"
        );
        let spelled_compilation = spelled
            .spent()
            .expect("the spelled runner's requests are measured")
            .compilation
            .expect("actual preparation is measured");
        let aliased_compilation = aliased
            .spent()
            .expect("the aliased runner's requests are measured")
            .compilation
            .expect("the aliased request is measured");
        assert_eq!(
            (
                spelled_compilation.attempts,
                aliased_compilation.attempts,
                aliased_compilation.process
            ),
            (Some(1), None, Some(1)),
            "one physical domain performs one actual preparation, and the alias's answer is a process hit"
        );
    }

    /// A command large enough that two cold preparations of it released together certainly overlap.
    fn large_command() -> Vec<u8> {
        let fill = "l".repeat(512 * 1024);
        wat::parse_str(format!(
            r#"(module
                (import "wasi_snapshot_preview1" "fd_write"
                    (func $write (param i32 i32 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 65536) "{fill}")
                (data (i32.const 100) "leased")
                (func (export "_start")
                    (i32.store (i32.const 0) (i32.const 100))
                    (i32.store (i32.const 4) (i32.const 6))
                    (drop (call $write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 6)))))"#
        ))
        .expect("the WAT is valid")
    }

    /// Two independently created contexts that share a cache domain prepare the bytes once: the keyed lease makes one the cold owner and answers the other from the disk.
    #[test]
    fn independent_contexts_prepare_one_cold_compilation_across_the_domain() {
        let directory =
            CacheDirectory::make("sealed-module-").expect("a parent-owned cache directory");
        let bytes = large_command();
        let digest = SealedDigest::of(&bytes);
        let (answers, answered) = mpsc::sync_channel(2);
        let together = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            let directory = &directory;
            let bytes = &bytes;
            for context in [ModuleOwner::default(), ModuleOwner::default()] {
                let together = &together;
                let answers = answers.clone();
                njutest_devkit::thread::ScopedThread::launch(scope, move || {
                    let runner = SealedRunner::cached(
                        &context,
                        Duration::from_secs(60),
                        &cache_for(directory.path()),
                    )
                    .expect("the independent context's runner");
                    let hooks = PreparationHooks {
                        observe: |stage| match stage {
                            PreparationStage::Observed => {
                                together.wait();
                            }
                            PreparationStage::Waiting => {}
                        },
                    };
                    let answer = runner
                        .obtain_with_hooks(bytes, None, &hooks)
                        .map(|module| (module.reuse(), *module.configuration()));
                    answers
                        .send(answer)
                        .expect("the context's answer has an owner");
                });
            }
        });
        let first = answered
            .recv()
            .expect("the first context answered")
            .expect("the first preparation succeeded");
        let second = answered
            .recv()
            .expect("the second context answered")
            .expect("the second preparation succeeded");
        assert_eq!(
            first.1, second.1,
            "both contexts bind the same engine settings"
        );
        let mut reuses = [first.0, second.0];
        reuses.sort_by_key(|reuse| match reuse {
            Reuse::Cold => 0,
            Reuse::Disk => 1,
            Reuse::Process => 2,
        });
        assert_eq!(
            reuses,
            [Reuse::Cold, Reuse::Disk],
            "the keyed domain lease lets one context compile the bytes cold and answers the other from the disk cache it published"
        );
        let cache = cache_for(directory.path());
        let mut name = cache
            .directory()
            .file_name()
            .expect("the cache has a name")
            .to_os_string();
        name.push(PREPARATION_LEASES);
        let leases = cache.directory().with_file_name(name);
        let mut key = super::Encoder::new("rust-mutants-sealed/preparation/v1");
        key.bytes(first.1.as_bytes()).bytes(digest.as_bytes());
        let filename = format!("{}.lock", key.finish());
        assert!(
            std::fs::read_dir(&leases)
                .expect("the lease directory exists")
                .any(|entry| entry.expect("the lease entry is read").file_name()
                    == filename.as_str()),
            "the physical lease binds module bytes and semantic engine configuration"
        );
    }

    /// The alarm of an engine with live runners but no running invocation advances nothing: an idle engine does no periodic work.
    #[test]
    fn an_idle_engine_advances_no_epoch() {
        let modules = ModuleOwner::default();
        let directory =
            CacheDirectory::make("sealed-module-").expect("a parent-owned cache directory");
        let runner = SealedRunner::cached(
            &modules,
            Duration::from_secs(60),
            &cache_for(directory.path()),
        )
        .expect("the runner");
        let window = RAW_BACKSTOP.saturating_mul(5);
        std::thread::sleep(window);
        let (advanced, backstops) = runner.alarm_advances();
        assert_eq!(
            (advanced, backstops),
            (0, 0),
            "an idle engine with no deadline and no raw flag does no periodic epoch work"
        );
        let bytes = command("woken");
        runner
            .prepare(&bytes)
            .expect("the preparation")
            .invoke(&invocation(), &Interrupt::of(Vec::new()))
            .expect("the guest runs and returns");
        let (advanced, backstops) = runner.alarm_advances();
        assert_eq!(
            (advanced, backstops),
            (0, 0),
            "a returned invocation leaves no armed deadline, so the alarm still advances nothing"
        );
    }

    /// A raised interrupt stops a running guest through the alarm's owned wake, with no periodic advance before it.
    #[test]
    fn a_raised_interrupt_stops_a_running_guest_through_the_owned_wake() {
        let modules = ModuleOwner::default();
        let directory =
            CacheDirectory::make("sealed-module-").expect("a parent-owned cache directory");
        let runner = SealedRunner::cached(
            &modules,
            Duration::from_secs(60),
            &cache_for(directory.path()),
        )
        .expect("the runner");
        let spinning = wat::parse_str(
            r#"(module
                (import "wasi_snapshot_preview1" "fd_write"
                    (func $write (param i32 i32 i32 i32) (result i32)))
                (memory (export "memory") 1)
                (data (i32.const 100) "spinning")
                (func (export "_start")
                    (i32.store (i32.const 0) (i32.const 100))
                    (i32.store (i32.const 4) (i32.const 8))
                    (drop (call $write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 8)))
                    (loop $again (br $again))))"#,
        )
        .expect("the WAT is valid");
        let module = runner.prepare(&spinning).expect("the spinning module");
        let mut asked = invocation();
        asked.fuel = 4_000_000_000;
        let raised = Arc::new(crate::Raised::new());
        let interrupt = Interrupt::raising(vec![Arc::clone(&raised)]);
        let started = Instant::now();
        let answer = std::thread::scope(|scope| {
            let waking = njutest_devkit::thread::ScopedThread::launch(scope, move || {
                let entered = Duration::from_millis(50);
                assert!(
                    !raised.wait_raised(entered),
                    "the interrupt is not raised before the guest entered its loop"
                );
                raised.raise();
            });
            let answer = module.invoke(&asked, &interrupt);
            waking.join().expect("the waking thread is joined");
            answer
        });
        let stopped = started.elapsed();
        match answer {
            Err(crate::SealedError::Interrupted) => {}
            other => panic!("the raised wake stops the spinning guest: {other:?}"),
        }
        assert!(
            stopped < Duration::from_secs(30),
            "the interrupt is delivered while the guest runs, not at the watchdog"
        );
        let (advanced, backstops) = runner.alarm_advances();
        assert_eq!(backstops, 0, "an owned raise requires no raw-flag backstop");
        assert!(
            advanced > 0,
            "the alarm advanced the epoch for the raise itself"
        );
    }

    #[test]
    fn an_exhausted_alarm_counter_is_sticky_and_cannot_answer_a_guest() {
        for backstop in [false, true] {
            let modules = ModuleOwner::default();
            let runner = SealedRunner::new(&modules, Duration::from_millis(100))
                .expect("the actual watchdog runner");
            let bytes = wat::parse_str(
                r#"(module (memory (export "memory") 1)
                    (func (export "_start") (loop $again (br $again))))"#,
            )
            .expect("the actual spinning guest");
            let module = runner.prepare(&bytes).expect("the spinning module");
            let mut asked = invocation();
            asked.fuel = u64::MAX;
            let counter = if backstop {
                &runner.ticking.advances.backstops
            } else {
                &runner.ticking.advances.advanced
            };
            counter.store(u64::MAX, Ordering::Relaxed);
            let interrupt =
                Interrupt::of(vec![Arc::new(std::sync::atomic::AtomicBool::new(false))]);
            for _invocation in 0..2 {
                let answer = module.invoke(&asked, &interrupt);
                assert!(
                    matches!(
                        answer,
                        Err(crate::SealedError::HostInvariant {
                            invariant: Invariant::Width
                        })
                    ),
                    "an exhausted physical alarm counter is a sticky host failure: {answer:?}"
                );
                assert_eq!(counter.load(Ordering::Relaxed), u64::MAX);
            }
        }
    }

    /// A watchdog deadline is the alarm's own wake: the engine's epoch advances when the deadline falls, with no periodic work before it.
    #[test]
    fn a_watchdog_deadline_is_advanced_by_the_alarm_itself() {
        let modules = ModuleOwner::default();
        let directory =
            CacheDirectory::make("sealed-module-").expect("a parent-owned cache directory");
        let runner = SealedRunner::cached(
            &modules,
            Duration::from_millis(100),
            &cache_for(directory.path()),
        )
        .expect("the runner");
        let spinning = wat::parse_str(
            r#"(module
                (memory (export "memory") 1)
                (func (export "_start") (loop $again (br $again))))"#,
        )
        .expect("the WAT is valid");
        let module = runner.prepare(&spinning).expect("the spinning module");
        let mut asked = invocation();
        asked.fuel = 4_000_000_000;
        match module.invoke(&asked, &Interrupt::of(Vec::new())) {
            Err(crate::SealedError::WatchdogExpired { limit }) => {
                assert_eq!(limit, Duration::from_millis(100));
            }
            other => panic!("the watchdog stops the spinning guest: {other:?}"),
        }
        let (advanced, backstops) = runner.alarm_advances();
        assert_eq!(
            backstops, 0,
            "no raw flag is armed, so no backstop wake runs"
        );
        assert!(
            advanced > 0,
            "the deadline itself woke the alarm and advanced the epoch"
        );
    }
}
