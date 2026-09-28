// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Sealed executions: each test of each sealed module run alone on the deterministic host, first as its own control, then against a mutant (ADR 0046).

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rust_mutants_decision::evidence::Sealed;
use rust_mutants_decision::judgement::{Account, Ending, Observed, judged};
use rust_mutants_sealed::{
    Arguments, ClockPolicy, Environment, Invocation, Limits, OverlayState, Preopens, RefusalReason,
    SealedError, SealedModule, SealedRunner, SealedStop, Snapshot, Transcript, TrapKind,
    WasiFunction,
};

use super::{SealedBuild, Unsealed};
use crate::execute::TestTarget;
use crate::libtest::{Asked, account};

/// Where the runtime's records land inside an instance: a directory nothing but the runtime writes.
pub const RECORDS: &str = "/rust-mutants-sealed";

/// The touch log a control's runtime writes.
const TOUCH_LOG: &str = "/rust-mutants-sealed/touch.log";

/// The fuel a control may spend: far past any test a person writes, and still a bound.
pub const CONTROL_FUEL: u64 = 200_000_000_000;

/// How many times its control's fuel a mutant's execution may spend.
pub const FUEL_FACTOR: u64 = 10;

/// What a mutant's execution may spend beyond its multiple of the control's.
pub const FUEL_FLOOR: u64 = 100_000_000;

/// The memory every instance may hold, which a 32-bit guest cannot exceed anyway.
pub const MEMORY: u64 = 4 << 30;

/// How much of each output stream an instance keeps.
const OUTPUT_CAP: u64 = 1 << 20;

/// How much an instance may write before the overlay refuses it.
const OVERLAY_CAP: u64 = 256 << 20;

/// What the realtime clock reads when an instance starts: 2026-01-01T00:00:00Z.
const REALTIME_ORIGIN: u64 = 1_767_225_600_000_000_000;

/// How long the host may take over one invocation before its watchdog stops it, which is never a verdict.
pub const WATCHDOG: Duration = Duration::from_mins(15);

/// What the standard library prints when the sandbox refused what a test asked for.
const SANDBOX_WORDS: [&str; 3] = [
    "operation not supported on this platform",
    "failed to spawn thread",
    "memory allocation of",
];

/// Why a sealed execution could not be set up or run.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BenchError {
    /// A sealed module could not be read.
    #[error("{}: {path}: {source}", crate::error::SEALED_MODULE_UNREADABLE.code)]
    ModuleUnreadable {
        /// The module.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The host could not run an invocation.
    #[error("{}: {target}: {source}", crate::error::SEALED_HOST_FAILED.code)]
    Host {
        /// The target whose module it was.
        target: String,
        /// What the host said.
        source: SealedError,
    },
    /// A target's environment is not text.
    #[error("{}: {target}: {name}", crate::error::SEALED_ENVIRONMENT_NOT_TEXT.code)]
    EnvironmentNotText {
        /// The target.
        target: String,
        /// The variable, as far as it can be spelled.
        name: String,
    },
    /// A file of the instrumented tree could not be read into the snapshot.
    #[error("{}: {path}: {source}", crate::error::SEALED_TREE_UNREADABLE.code)]
    TreeUnreadable {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// The instrumented tree cannot be a snapshot.
    #[error("{}: {source}", crate::error::SEALED_TREE_UNREADABLE.code)]
    Snapshot {
        /// What the host said about it.
        source: SealedError,
    },
}

impl BenchError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> crate::error::ErrorCode {
        match self {
            Self::ModuleUnreadable { .. } => crate::error::SEALED_MODULE_UNREADABLE,
            Self::Host { .. } => crate::error::SEALED_HOST_FAILED,
            Self::EnvironmentNotText { .. } => crate::error::SEALED_ENVIRONMENT_NOT_TEXT,
            Self::TreeUnreadable { .. } | Self::Snapshot { .. } => {
                crate::error::SEALED_TREE_UNREADABLE
            }
        }
    }
}

/// What the instrumented tree is inside an instance: its files, preopened where the build knew them.
#[derive(Debug, Clone)]
pub struct Tree {
    /// The absolute path the build read the tree at, which the guest reaches it by.
    pub root: String,
    /// Its files, instrumented.
    pub snapshot: Snapshot,
}

impl Tree {
    /// The tree at `root` holding `files`, each read now, by its `/`-separated path relative to `root`.
    ///
    /// # Errors
    /// A file that cannot be read, a root that is not text, or a path the snapshot refuses.
    pub fn read<'a>(
        root: &Path,
        files: impl IntoIterator<Item = &'a str>,
    ) -> Result<Self, BenchError> {
        let mut builder = Snapshot::builder();
        for relative in files {
            let path = root.join(relative);
            let bytes = std::fs::read(&path).map_err(|source| BenchError::TreeUnreadable {
                path: path.clone(),
                source,
            })?;
            builder = builder
                .file(relative, bytes)
                .map_err(|source| BenchError::Snapshot { source })?;
        }
        let snapshot = builder
            .build()
            .map_err(|source| BenchError::Snapshot { source })?;
        let root = match root.to_str() {
            Some(root) => root.to_owned(),
            None => {
                return Err(BenchError::EnvironmentNotText {
                    target: String::new(),
                    name: root.display().to_string(),
                });
            }
        };
        Ok(Self { root, snapshot })
    }
}

/// How one test's control ran: what a mutant's execution of the same test is judged against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Control {
    /// The fuel it spent.
    pub fuel: u64,
    /// Every guard it reached.
    pub reached: BTreeSet<u32>,
    refusals: BTreeSet<(WasiFunction, RefusalReason)>,
    sandbox: BTreeSet<&'static str>,
}

/// Why a listed test has no control a mutant can be judged against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Uncontrolled {
    /// What its control came to.
    pub came_to: Sealed,
}

/// One sealed module, ready: its tests and each one's control, or why it has none.
#[derive(Debug)]
pub struct Station<'runner> {
    module: SealedModule<'runner>,
    target: TestTarget,
    /// Each test its harness lists, and its control.
    pub controls: BTreeMap<String, Result<Control, Uncontrolled>>,
}

/// Every sealed module of a session, prepared, listed and controlled.
#[derive(Debug)]
pub struct Bench<'runner> {
    /// Each target's station, by target identity.
    pub stations: BTreeMap<String, Station<'runner>>,
    /// Each target with no station, and why.
    pub unsealed: BTreeMap<String, Unsealed>,
    tree: Tree,
    catalog: String,
    bounds: crate::touch::Bounds,
}

impl<'runner> Bench<'runner> {
    /// Prepares every module of `sealed` on `runner`, lists its tests and runs each one's control inside `tree`.
    ///
    /// # Errors
    /// A module that cannot be read, an environment that is not text, or a host that cannot run what it is given.
    pub fn assemble(
        runner: &'runner SealedRunner,
        sealed: &SealedBuild,
        tree: Tree,
        (catalog, bounds): (&str, crate::touch::Bounds),
    ) -> Result<Self, BenchError> {
        let mut bench = Self {
            stations: BTreeMap::new(),
            unsealed: sealed.unsealed.clone(),
            tree,
            catalog: catalog.to_owned(),
            bounds,
        };
        for (id, module) in &sealed.modules {
            let path = &module.target.executable;
            let bytes = std::fs::read(path).map_err(|source| BenchError::ModuleUnreadable {
                path: path.clone(),
                source,
            })?;
            let prepared = runner.prepare(&bytes).map_err(|source| BenchError::Host {
                target: id.clone(),
                source,
            })?;
            let mut station = Station {
                module: prepared,
                target: module.target.clone(),
                controls: BTreeMap::new(),
            };
            let Some(tests) = bench.listed(&station)? else {
                bench.unsealed.insert(id.clone(), Unsealed::NotListed);
                continue;
            };
            for test in tests {
                let control = bench.control(&station, &test)?;
                station.controls.insert(test, control);
            }
            bench.stations.insert(id.clone(), station);
        }
        Ok(bench)
    }

    /// What `test` of `target` comes to with `mutant` active, judged against its control, or nothing where there is no control to judge it against.
    ///
    /// # Errors
    /// An environment that is not text, or a host that cannot run the invocation.
    pub fn put(
        &self,
        target: &str,
        test: &str,
        mutant: &str,
    ) -> Result<Option<Sealed>, BenchError> {
        let Some(station) = self.stations.get(target) else {
            return Ok(None);
        };
        let Some(Ok(control)) = station.controls.get(test) else {
            return Ok(None);
        };
        let budget = control
            .fuel
            .saturating_mul(FUEL_FACTOR)
            .saturating_add(FUEL_FLOOR);
        let invocation = self.invocation(station, one_test(test), (Some(mutant), budget))?;
        let transcript = invoke(station, &invocation)?;
        Ok(Some(judged(observed(&transcript, test, Some(control)))))
    }

    fn listed(&self, station: &Station<'_>) -> Result<Option<Vec<String>>, BenchError> {
        let invocation = self.invocation(
            station,
            vec![
                "--list".to_owned(),
                "--format".to_owned(),
                "terse".to_owned(),
            ],
            (None, CONTROL_FUEL),
        )?;
        let transcript = invoke(station, &invocation)?;
        if transcript.stop() != SealedStop::Returned {
            return Ok(None);
        }
        let Ok(text) = std::str::from_utf8(transcript.stdout().bytes()) else {
            return Ok(None);
        };
        Ok(Some(
            text.lines()
                .filter_map(|line| line.strip_suffix(": test"))
                .map(str::to_owned)
                .collect(),
        ))
    }

    fn control(
        &self,
        station: &Station<'_>,
        test: &str,
    ) -> Result<Result<Control, Uncontrolled>, BenchError> {
        let invocation = self.invocation(station, one_test(test), (None, CONTROL_FUEL))?;
        let transcript = invoke(station, &invocation)?;
        let came_to = judged(observed(&transcript, test, None));
        if came_to != Sealed::Passed {
            return Ok(Err(Uncontrolled { came_to }));
        }
        let unread = Uncontrolled {
            came_to: Sealed::Doubted(rust_mutants_decision::evidence::Doubt::Unaccounted),
        };
        let reached = match touched(&transcript).map(std::str::from_utf8) {
            None => BTreeSet::new(),
            Some(Err(_not_text)) => return Ok(Err(unread)),
            Some(Ok(log)) => match crate::touch::read(log, &self.catalog, self.bounds) {
                Ok(touches) => touches.reached.union(),
                Err(_unread) => return Ok(Err(unread)),
            },
        };
        Ok(Ok(Control {
            fuel: transcript.fuel_spent(),
            reached,
            refusals: refusals(&transcript),
            sandbox: sandbox(&transcript),
        }))
    }

    fn invocation(
        &self,
        station: &Station<'_>,
        harness: Vec<String>,
        (mutant, fuel): (Option<&str>, u64),
    ) -> Result<Invocation, BenchError> {
        let id = station.target.id().to_owned();
        let program = match station
            .target
            .executable
            .file_name()
            .and_then(|name| name.to_str())
        {
            Some(program) => program.to_owned(),
            None => "test".to_owned(),
        };
        let mut arguments = vec![program];
        arguments.extend(harness);
        let mut variables = Vec::new();
        for (name, value) in station.target.cargo_env.for_process() {
            let (Some(name), Some(value)) = (name.to_str(), value.to_str()) else {
                return Err(BenchError::EnvironmentNotText {
                    target: id,
                    name: crate::telling::LosslessBytes::new(name.as_encoded_bytes()).to_string(),
                });
            };
            variables.push((name.to_owned(), value.to_owned()));
        }
        variables.push((
            crate::instrument::CATALOG_ENV.to_owned(),
            self.catalog.clone(),
        ));
        let asked = match mutant {
            Some(mutant) => (crate::instrument::ACTIVE_ENV, mutant),
            None => (crate::instrument::TOUCH_ENV, TOUCH_LOG),
        };
        variables.push((asked.0.to_owned(), asked.1.to_owned()));
        let host = |source| BenchError::Host {
            target: id.clone(),
            source,
        };
        let records = Snapshot::builder().build().map_err(host)?;
        Ok(Invocation {
            arguments: Arguments::new(arguments).map_err(host)?,
            environment: Environment::new(variables).map_err(host)?,
            preopens: Preopens::new(vec![
                (self.tree.root.clone(), self.tree.snapshot.clone()),
                (RECORDS.to_owned(), records),
            ])
            .map_err(host)?,
            seed: seed(station.target.id()),
            fuel,
            limits: Limits {
                memory: MEMORY,
                stdout: OUTPUT_CAP,
                stderr: OUTPUT_CAP,
                overlay: OVERLAY_CAP,
            },
            clock: ClockPolicy {
                realtime_origin: REALTIME_ORIGIN,
                monotonic_origin: 0,
                nanos_per_fuel: NonZeroU64::MIN,
            },
        })
    }
}

/// What `station`'s module did under `invocation`.
fn invoke(station: &Station<'_>, invocation: &Invocation) -> Result<Transcript, BenchError> {
    station
        .module
        .invoke(invocation)
        .map_err(|source| BenchError::Host {
            target: station.target.id().to_owned(),
            source,
        })
}

/// The harness's arguments that run `test` alone, one thread, its output uncaptured.
fn one_test(test: &str) -> Vec<String> {
    vec![
        "--exact".to_owned(),
        test.to_owned(),
        "--test-threads".to_owned(),
        "1".to_owned(),
        "--nocapture".to_owned(),
    ]
}

/// The seed of every instance of `target`, the same for its control and for every mutant's execution.
fn seed(target: &str) -> u64 {
    let digest = crate::id::digest(target.as_bytes());
    let mut seed: u64 = 0;
    for byte in digest.as_bytes().iter().take(16) {
        seed = seed.rotate_left(4) ^ u64::from(*byte);
    }
    seed
}

/// Whether `bytes` hold `words` anywhere, read as bytes so a line that is not text hides nothing.
fn holds(bytes: &[u8], words: &str) -> bool {
    let words = words.as_bytes();
    !words.is_empty() && bytes.windows(words.len()).any(|window| window == words)
}

/// What the host observed of one execution of `test`, against `control` where there is one.
fn observed(transcript: &Transcript, test: &str, control: Option<&Control>) -> Observed {
    let stderr = transcript.stderr().bytes();
    let ending = match transcript.stop() {
        SealedStop::Returned => Ending::Returned,
        SealedStop::Exited { code: 0 } => Ending::ExitedZero,
        SealedStop::Exited { code }
            if i32::try_from(code).is_ok_and(|code| code == crate::libtest::FAILURE_STATUS) =>
        {
            Ending::ExitedFailure
        }
        SealedStop::Exited { .. } => Ending::ExitedOther,
        SealedStop::Trapped {
            kind: TrapKind::Unreachable,
        } if holds(stderr, " panicked at ") => Ending::Panicked,
        SealedStop::Trapped {
            kind: TrapKind::Unreachable,
        } => Ending::Aborted,
        SealedStop::Trapped {
            kind: TrapKind::StackOverflow,
        } => Ending::StackOverflow,
        SealedStop::Trapped { .. } => Ending::Trapped,
        SealedStop::FuelExhausted => Ending::FuelExhausted,
        SealedStop::MemoryExhausted => Ending::MemoryExhausted,
    };
    let exit = match transcript.stop() {
        SealedStop::Returned => Some(0),
        SealedStop::Exited { code } => match i32::try_from(code) {
            Ok(code) => Some(code),
            Err(_wider) => None,
        },
        SealedStop::Trapped { .. } | SealedStop::FuelExhausted | SealedStop::MemoryExhausted => {
            None
        }
    };
    let asked = [test.to_owned()];
    let account = match account(transcript.stdout().bytes(), Asked::Exact(&asked), exit) {
        Ok(accounted) if accounted.summary.passed == 1 && accounted.summary.failed == 0 => {
            Account::Passed
        }
        Ok(accounted) if accounted.summary.failed == 1 && accounted.failed == asked => {
            Account::Failed
        }
        Ok(_) | Err(_) => Account::Other,
    };
    let beyond_control = match control {
        None => false,
        Some(control) => {
            !refusals(transcript).is_subset(&control.refusals)
                || !sandbox(transcript).is_subset(&control.sandbox)
        }
    };
    Observed {
        ending,
        account,
        beyond_control,
    }
}

/// Every function the host refused, and why.
fn refusals(transcript: &Transcript) -> BTreeSet<(WasiFunction, RefusalReason)> {
    transcript
        .refusals()
        .iter()
        .map(|refusal| (refusal.function, refusal.reason))
        .collect()
}

/// Every message of the standard library's for a refusal of the sandbox that the instance printed.
fn sandbox(transcript: &Transcript) -> BTreeSet<&'static str> {
    let stderr = transcript.stderr().bytes();
    SANDBOX_WORDS
        .into_iter()
        .filter(|words| holds(stderr, words))
        .collect()
}

/// The bytes of the touch log the control's runtime wrote, where it wrote one.
fn touched(transcript: &Transcript) -> Option<&[u8]> {
    transcript.overlay().iter().find_map(|entry| {
        if entry.path != TOUCH_LOG {
            return None;
        }
        match &entry.state {
            OverlayState::File { contents, .. } => Some(contents.as_slice()),
            OverlayState::Directory { .. } | OverlayState::Removed => None,
        }
    })
}
