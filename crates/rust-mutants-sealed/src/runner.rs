// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The runner: one deterministic engine, a module validated and compiled once, and a fresh store and instance for every invocation.

use std::hash::{Hash as _, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use wasmtime::{
    Config, Engine, FuncType, InstancePre, Linker, Module, OptLevel, Store, Trap, UpdateDeadline,
    WasmFeatures,
};

use crate::digest::{Encoder, SealedDigest};
use crate::error::{Invariant, RuntimeStep, SealedError};
use crate::host::{BYTE_FUEL, CALL_FUEL, Host, HostStop, RESOLUTION, TABLE_ELEMENTS};
use crate::imports::{IMPORT_MODULE, WasiFunction};
use crate::interrupt::Interrupt;
use crate::invocation::Invocation;
use crate::transcript::{KEPT_REQUESTS, Parts, SealedStop, Transcript, TrapClass, classify};
use crate::validate;

/// The wasmtime every digest of this crate is taken under, which `Cargo.toml` pins exactly.
pub const WASMTIME_VERSION: &str = "48.0.3";

/// The native stack a guest may use, in bytes.
const MAX_WASM_STACK: usize = 512 * 1024;

/// How often the watchdog's thread advances the engine's epoch.
const TICK: Duration = Duration::from_millis(10);

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

/// The thread that advances an engine's epoch every tick, stopped and joined when it is dropped.
#[derive(Debug)]
struct EpochTicker {
    /// Set to stop the thread.
    stopping: Arc<AtomicBool>,
    /// The thread, until it is joined.
    handle: Option<JoinHandle<()>>,
}

impl EpochTicker {
    /// Starts advancing `engine`'s epoch every tick.
    fn start(engine: Engine) -> std::io::Result<Self> {
        let stopping = Arc::new(AtomicBool::new(false));
        let asked = Arc::clone(&stopping);
        let handle = std::thread::Builder::new()
            .name("rust-mutants-sealed-epoch".to_owned())
            .spawn(move || {
                while !asked.load(Ordering::Acquire) {
                    std::thread::park_timeout(TICK);
                    engine.increment_epoch();
                }
            })?;
        Ok(Self {
            stopping,
            handle: Some(handle),
        })
    }

    /// Stops the thread and waits for it.
    fn stop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            handle.thread().unpark();
            match handle.join() {
                Ok(()) => {}
                Err(panic) => drop(panic),
            }
        }
    }
}

impl Drop for EpochTicker {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The one engine sealed guests are compiled and run by, with the host linked once.
#[derive(Debug)]
pub struct SealedRunner {
    /// The deterministic engine.
    engine: Engine,
    /// Every function of the table, linked.
    linker: Linker<Host>,
    /// The digest of everything about the engine and the host that a transcript depends on.
    configuration: SealedDigest,
    /// How long a guest may run by the wall clock before the watchdog stops it.
    watchdog: Duration,
    /// The thread advancing the epoch the watchdog is checked at.
    ticker: EpochTicker,
}

impl SealedRunner {
    /// The runner, its watchdog stopping a guest `watchdog` after it starts.
    ///
    /// # Errors
    /// [`SealedError::Engine`] where wasmtime refuses the configuration, [`SealedError::Link`] where the host cannot be linked, [`SealedError::WatchdogUnavailable`] where its thread cannot start.
    pub fn new(watchdog: Duration) -> Result<Self, SealedError> {
        let mut config = Config::new();
        config
            .consume_fuel(true)
            .epoch_interruption(true)
            .cranelift_nan_canonicalization(true)
            .cranelift_opt_level(OptLevel::Speed)
            .relaxed_simd_deterministic(true)
            .max_wasm_stack(MAX_WASM_STACK)
            .memory_init_cow(false)
            .wasm_features(REFUSED_FEATURES, false);
        let engine = Engine::new(&config).map_err(|source| SealedError::Engine { source })?;
        let linker = link(&engine)?;
        let configuration = configuration(&engine);
        let ticker = EpochTicker::start(engine.clone())
            .map_err(|source| SealedError::WatchdogUnavailable { source })?;
        Ok(Self {
            engine,
            linker,
            configuration,
            watchdog,
            ticker,
        })
    }

    /// The digest of everything about the engine and the host a transcript depends on.
    #[must_use]
    pub const fn configuration(&self) -> &SealedDigest {
        &self.configuration
    }

    /// Validates and compiles `bytes` once, for as many invocations as are asked of it.
    ///
    /// # Errors
    /// A module that is not a core WASI command with one plain memory importing only the table, or one wasmtime refuses to compile or link.
    pub fn prepare(&self, bytes: &[u8]) -> Result<SealedModule<'_>, SealedError> {
        validate::shape(bytes)?;
        let module =
            Module::new(&self.engine, bytes).map_err(|source| SealedError::Compile { source })?;
        validate::interface(&module)?;
        let pre = self
            .linker
            .instantiate_pre(&module)
            .map_err(|source| SealedError::Link { source })?;
        Ok(SealedModule {
            runner: self,
            pre,
            digest: SealedDigest::of(bytes),
        })
    }
}

impl Drop for SealedRunner {
    fn drop(&mut self) {
        self.ticker.stop();
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
    /// The runner that compiled it.
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
            .field("configuration", &self.runner.configuration)
            .finish_non_exhaustive()
    }
}

impl SealedModule<'_> {
    /// The digest of the module's bytes.
    #[must_use]
    pub const fn digest(&self) -> &SealedDigest {
        &self.digest
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
        if interrupt.raised() {
            return Err(SealedError::Interrupted);
        }
        let digest = invocation.digest(&self.digest, &self.runner.configuration);
        let watchdog = self.runner.watchdog;
        let deadline = Instant::now().checked_add(watchdog);
        let host = Host::new(invocation, (deadline, interrupt.clone())).map_err(broken)?;
        let mut store = Store::new(&self.runner.engine, host);
        store.limiter(|host| &mut host.limiter);
        store
            .set_fuel(invocation.fuel)
            .map_err(|source| runtime(RuntimeStep::Fuel, source))?;
        store.set_epoch_deadline(1);
        let stopping = interrupt.clone();
        store.epoch_deadline_callback(move |mut context| {
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
        let stop = match self.pre.instantiate(&mut store) {
            Ok(instance) => {
                let memory = instance.get_memory(&mut store, "memory").ok_or(
                    SealedError::HostInvariant {
                        invariant: Invariant::Memory,
                    },
                )?;
                store.data_mut().memory = Some(memory);
                let start = instance
                    .get_typed_func::<(), ()>(&mut store, "_start")
                    .map_err(|source| runtime(RuntimeStep::Entry, source))?;
                let called = start.call(&mut store, ());
                stopped(store.data(), (RuntimeStep::Entry, called), watchdog)?
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
