// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Sealed executions: each test of each sealed module run alone on the deterministic host, first as its own control, then against a mutant (ADR 0046).

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU64;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use rust_mutants_decision::evidence::Sealed;
use rust_mutants_decision::judgement::{Account, Ending, Harness, Observed, judged};
use rust_mutants_sealed::{
    Arguments, ClockPolicy, Counted, Environment, Interrupt, Invocation, Limits, OverlayEntry,
    OverlayState, Preopen, Preopens, RefusalReason, SealedError, SealedModule, SealedRunner,
    SealedStop, Snapshot, Start, Transcript, Transcripts, TrapKind, WasiFunction,
};

use super::doctest::{Expects, NO_SUCH_INDEX, RUN_ONE, listed};
use super::{SealedBuild, Unsealed};
use crate::execute::TestTarget;
use crate::libtest::{Asked, Configured, Own, harness_report};

/// Where the runtime's records land inside an instance: a directory nothing but the runtime writes.
pub const RECORDS: &str = "/rust-mutants-sealed";

/// The touch log a control's runtime writes.
const TOUCH_LOG: &str = "/rust-mutants-sealed/touch.log";

/// Where a test that cannot measure on the sealed host says so, as ADR 0043 lets it.
const DECLINE_LOG: &str = "/rust-mutants-sealed/decline-notice";

/// Where an instance keeps what a test writes for itself: a directory of its own, which starts with an empty home and an empty temporary directory.
pub const SCRATCH: &str = "/rust-mutants-scratch";

/// The home an instance's `HOME` names, inside [`SCRATCH`].
pub const SCRATCH_HOME: &str = "/rust-mutants-scratch/home";

/// The temporary directory an instance's `TMPDIR` names, inside [`SCRATCH`].
pub const SCRATCH_TMP: &str = "/rust-mutants-scratch/tmp";

/// The directory cargo gives an integration test to write in, which a sealed instance holds, empty, at the path its build baked in.
const TARGET_TMPDIR: &str = "CARGO_TARGET_TMPDIR";

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

/// The ceilings every instance runs under.
pub const LIMITS: Limits = Limits {
    memory: MEMORY,
    stdout: OUTPUT_CAP,
    stderr: OUTPUT_CAP,
    overlay: OVERLAY_CAP,
};

/// How every instance's clocks read: from the realtime origin and from zero, each moved one nanosecond by every unit of fuel spent.
pub const CLOCK: ClockPolicy = ClockPolicy {
    realtime_origin: REALTIME_ORIGIN,
    monotonic_origin: 0,
    nanos_per_fuel: NonZeroU64::MIN,
};

/// How long the host may take over one invocation before its watchdog stops it, which is never a verdict.
pub const WATCHDOG: Duration = Duration::from_mins(15);

/// What the standard library prints when the sandbox refused what a test asked for: an operation the target does not support, a thread, an allocation, or a panic of its own platform layer, such as the one `std::env::temp_dir` raises on `wasm32-wasip1` whatever `TMPDIR` names.
const SANDBOX_WORDS: [&str; 4] = [
    "operation not supported on this platform",
    "failed to spawn thread",
    "memory allocation of",
    "/library/std/src/sys/",
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
    /// The host could not start.
    #[error("{}: the host could not start: {source}", crate::error::SEALED_HOST_FAILED.code)]
    Runner {
        /// What the host said.
        source: SealedError,
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
    /// A sealed execution's judgement broke the rule a pass keeps.
    #[error(
        "{}: {target}: the judgement of {test} broke the rule a pass keeps, so no verdict is written on it",
        crate::error::SEALED_JUDGEMENT_CONTRADICTED.code
    )]
    JudgementContradicted {
        /// The target whose module ran.
        target: String,
        /// The test it ran.
        test: String,
    },
    /// The run was interrupted while a sealed execution ran, which says nothing about what it ran.
    #[error("{}: the run was interrupted during a sealed execution", crate::error::INTERRUPTED.code)]
    Interrupted,
}

impl BenchError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> crate::error::ErrorCode {
        match self {
            Self::ModuleUnreadable { .. } => crate::error::SEALED_MODULE_UNREADABLE,
            Self::Runner { .. } | Self::Host { .. } => crate::error::SEALED_HOST_FAILED,
            Self::EnvironmentNotText { .. } => crate::error::SEALED_ENVIRONMENT_NOT_TEXT,
            Self::TreeUnreadable { .. } | Self::Snapshot { .. } => {
                crate::error::SEALED_TREE_UNREADABLE
            }
            Self::Interrupted => crate::error::INTERRUPTED,
            Self::JudgementContradicted { .. } => crate::error::SEALED_JUDGEMENT_CONTRADICTED,
        }
    }
}

/// What the instrumented tree is inside an instance: its files, preopened where the build knew them.
#[derive(Debug, Clone)]
pub struct Tree {
    /// The absolute path the build read the tree at, spelled as it baked it in, without a trailing separator, which the guest reaches it by.
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

    /// Where `directory` is in the tree, as `/`-separated names below its root, or nothing where it is outside the tree or its names are not text.
    #[must_use]
    pub fn within(&self, directory: &Path) -> Option<String> {
        let below = match directory.strip_prefix(&self.root) {
            Ok(below) => below,
            Err(_outside) => return None,
        };
        let mut names = Vec::new();
        for component in below.components() {
            match component {
                Component::Normal(name) => names.push(name.to_str()?),
                Component::CurDir => {}
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
            }
        }
        Some(names.join("/"))
    }
}

/// The directory a target's build script wrote, `OUT_DIR`, as the build left it, at the path the build gave the target.
#[derive(Debug, Clone)]
struct Built {
    path: String,
    snapshot: Snapshot,
}

impl Built {
    /// What `target`'s build script left in its `OUT_DIR`, or nothing where no build script ran for it.
    ///
    /// # Errors
    /// A directory that cannot be listed, a file that cannot be read, or a name that is not text.
    fn of(target: &TestTarget) -> Result<Option<Self>, BenchError> {
        let Some(out_dir) = target.cargo_env.var("OUT_DIR") else {
            return Ok(None);
        };
        let not_text = |name: String| BenchError::EnvironmentNotText {
            target: target.id().to_owned(),
            name,
        };
        let root = Path::new(out_dir);
        let path = root
            .to_str()
            .ok_or_else(|| not_text(root.display().to_string()))?
            .to_owned();
        let unreadable = |path: &Path| {
            let path = path.to_path_buf();
            move |source| BenchError::TreeUnreadable { path, source }
        };
        let snapshot = |source| BenchError::Snapshot { source };
        let mut builder = Snapshot::builder();
        let mut pending = vec![(root.to_path_buf(), String::new())];
        while let Some((directory, relative)) = pending.pop() {
            for entry in std::fs::read_dir(&directory).map_err(unreadable(&directory))? {
                let entry = entry.map_err(unreadable(&directory))?;
                let file_name = entry.file_name();
                let name = file_name
                    .to_str()
                    .ok_or_else(|| not_text(entry.path().display().to_string()))?;
                let below = if relative.is_empty() {
                    name.to_owned()
                } else {
                    format!("{relative}/{name}")
                };
                let path = entry.path();
                if std::fs::metadata(&path)
                    .map_err(unreadable(&path))?
                    .is_dir()
                {
                    builder = builder.directory(&below).map_err(snapshot)?;
                    pending.push((path, below));
                } else {
                    let bytes = std::fs::read(&path).map_err(unreadable(&path))?;
                    builder = builder.file(&below, bytes).map_err(snapshot)?;
                }
            }
        }
        Ok(Some(Self {
            path,
            snapshot: builder.build().map_err(snapshot)?,
        }))
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
    /// The words it declined to measure in, where it declined (ADR 0043): it passed having measured nothing, and bounds nothing a mutant's execution is held to.
    pub declined: Option<Vec<u8>>,
}

/// Why a listed test has no control a mutant can be judged against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Uncontrolled {
    /// Its control detected something with nothing active: it fails sealed.
    Detected(rust_mutants_decision::evidence::Detection),
    /// Its control established nothing, for this reason.
    Doubted(rust_mutants_decision::evidence::Doubt),
    /// It runs natively and did not build for the sealed target.
    Unbuilt,
    /// It runs natively and the sealed build does not hold it.
    Unsealed,
}

impl Uncontrolled {
    /// The name a report spells why with.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Detected(how) => super::record::Came::of(Sealed::Detected(how)).name(),
            Self::Doubted(why) => super::record::Came::of(Sealed::Doubted(why)).name(),
            Self::Unbuilt => "unbuilt",
            Self::Unsealed => "not-held",
        }
    }
}

/// How the sealed build answers for one native target, as a run found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answering {
    /// It has a station, and these tests its native baseline ran have no sealed control.
    Station {
        /// Each test with no control, by name, and why.
        uncontrolled: Vec<(String, Uncontrolled)>,
    },
    /// It has no station, and why.
    Unsealed(Unsealed),
}

impl Answering {
    /// How a run that assembled no bench answers for each target `sealed` was given: each by why it has no module.
    #[must_use]
    pub fn unassembled(sealed: &SealedBuild) -> BTreeMap<String, Self> {
        sealed
            .unsealed
            .iter()
            .map(|(id, why)| (id.clone(), Self::Unsealed(*why)))
            .collect()
    }
}

/// What one target's native baseline ran, which its station has to hold.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ran {
    /// Every test it passed, as its harness named them.
    pub tests: Vec<String>,
    /// Whether those are every test it ran: the names come to the count its summaries said.
    pub whole: bool,
    /// How many tests it ignored.
    pub ignored: u32,
    /// Every test its harness said should panic, which is how a doctest a merged binary lists is to pass.
    pub should_panic: Vec<String>,
}

/// How one test of a station runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Run {
    /// libtest runs it by name.
    Libtest,
    /// rustdoc's `main` runs one doctest: the one at `index` of a merged binary, or the only one its binary holds.
    Doctest {
        /// Its index, where its binary is merged.
        index: Option<usize>,
        /// What it passes by.
        expects: Expects,
    },
}

/// What one invocation asks of a module.
struct Asking {
    arguments: Vec<String>,
    index: Option<usize>,
}

impl Run {
    /// What running it as `name` asks, libtest's run given `harness` as the native one is, less the options it sets itself.
    fn asking(self, name: &str, harness: &Configured) -> Asking {
        match self {
            Self::Libtest => Asking {
                arguments: [
                    vec!["--exact".to_owned(), name.to_owned()],
                    harness.beside(&Own::ALL),
                ]
                .concat(),
                index: None,
            },
            Self::Doctest { index, .. } => Asking {
                arguments: Vec::new(),
                index,
            },
        }
    }
}

/// One module of a station, and how each test it holds runs.
#[derive(Debug)]
struct Holding<'runner> {
    module: SealedModule<'runner>,
    tests: BTreeMap<String, Run>,
}

/// One sealed target, ready: the modules that hold its tests, each test and its control, or why it has none.
#[derive(Debug)]
pub struct Station<'runner> {
    holdings: Vec<Holding<'runner>>,
    target: TestTarget,
    program: String,
    built: Option<Built>,
    /// Each test the native baseline ran, and its control, or why it has none.
    pub controls: BTreeMap<String, Result<Control, Uncontrolled>>,
}

impl Station<'_> {
    /// Holds this station to exactly the tests `ran` says its native baseline ran, each named as `named` spells it for the station or not at all where it never runs: a test the native run does not run is no test of the suite's, and one the sealed build does not hold is uncontrolled.
    ///
    /// # Errors
    /// The reason there is no station, where no baseline ran or its account does not name every test it ran.
    fn owes<F>(&mut self, ran: Option<&Ran>, named: F) -> Result<(), Unsealed>
    where
        F: Fn(&str) -> Option<String>,
    {
        let ran = match ran {
            None => return Err(Unsealed::NotVerified),
            Some(ran) if !ran.whole => return Err(Unsealed::NativeUnnamed),
            Some(ran) => ran,
        };
        let owed: BTreeSet<String> = ran.tests.iter().filter_map(|test| named(test)).collect();
        self.hold_to(&owed);
        Ok(())
    }

    /// Holds this station to exactly `tests`: a control of any other test is dropped, and a test among them that none of its modules holds is uncontrolled.
    pub(super) fn hold_to(&mut self, tests: &BTreeSet<String>) {
        self.controls.retain(|test, _| tests.contains(test));
        for test in tests {
            self.controls
                .entry(test.clone())
                .or_insert(Err(Uncontrolled::Unsealed));
        }
    }

    /// The module that holds `test`, and how it runs.
    fn holding(&self, test: &str) -> Option<(&SealedModule<'_>, Run)> {
        self.holdings
            .iter()
            .find_map(|holding| holding.tests.get(test).map(|run| (&holding.module, *run)))
    }

    /// Whether any of its modules holds a test named `test`.
    fn names(&self, test: &str) -> bool {
        self.holdings
            .iter()
            .any(|holding| holding.tests.contains_key(test))
    }
}

/// Which of the tests a module holds have their control run.
#[derive(Debug, Clone, Copy)]
pub(super) enum Controlled<'named> {
    /// Every one it holds, which the native baseline then narrows to the suite's.
    Every,
    /// These alone, which recorded executions named.
    Only(&'named BTreeSet<String>),
}

impl Controlled<'_> {
    /// Whether the control of `test` is run.
    fn asks(self, test: &str) -> bool {
        match self {
            Self::Every => true,
            Self::Only(tests) => tests.contains(test),
        }
    }
}

/// Every sealed module of a session, prepared, listed and controlled.
#[derive(Debug)]
pub struct Bench<'runner> {
    runner: &'runner SealedRunner,
    pub(crate) cancel: crate::runner::Cancel,
    compiled: Option<u32>,
    /// Each target's station, by target identity.
    pub stations: BTreeMap<String, Station<'runner>>,
    /// Each target with no station, and why.
    pub unsealed: BTreeMap<String, Unsealed>,
    tree: Tree,
    harness: Configured,
    catalog: String,
    bounds: crate::touch::Bounds,
    interrupt: Interrupt,
    /// Where what sealed invocations established in earlier runs is remembered, and this run's own answers are kept for the next one.
    transcripts: Transcripts,
    /// What the host spends on sealed executions, shared with every bench of one run.
    counted: Counted,
}

/// `path`, read, its standard library made to answer the temporary and the home directory from the environment, and prepared on `runner` as a module of `target`, the compile counted.
fn prepared<'runner>(
    (runner, counted): (&'runner SealedRunner, &Counted),
    path: &Path,
    target: &str,
) -> Result<SealedModule<'runner>, BenchError> {
    let bytes = std::fs::read(path).map_err(|source| BenchError::ModuleUnreadable {
        path: path.to_path_buf(),
        source,
    })?;
    let host = |source| BenchError::Host {
        target: target.to_owned(),
        source,
    };
    let answering =
        rust_mutants_sealed::redirected(&bytes, &super::platform::REDIRECTS).map_err(host)?;
    let module = runner
        .prepare_counted(&answering.bytes, counted)
        .map_err(host)?;
    Ok(module)
}

impl<'runner> Bench<'runner> {
    /// Prepares every module of `sealed` on `runner`, lists its tests and runs each one's control inside `tree`, every libtest invocation given the harness arguments `harness` as the native ones are, less the options it sets itself, and holds each station to every test its target's native baseline in `natives` ran; every execution, then and later, stops when `interrupt` is raised.
    ///
    /// # Errors
    /// A module that cannot be read, an environment that is not text, a host that cannot run what it is given, or [`BenchError::Interrupted`].
    pub fn assemble(
        (runner, cancel): (&'runner SealedRunner, crate::runner::Cancel),
        (sealed, natives): (&SealedBuild, &BTreeMap<String, Ran>),
        (tree, harness): (Tree, &Configured),
        (catalog, bounds, transcripts, counted): (&str, crate::touch::Bounds, Transcripts, Counted),
    ) -> Result<Self, BenchError> {
        counted.assembled();
        let mut bench = Self::unassembled(
            (runner, cancel),
            sealed,
            (tree, harness),
            (catalog, bounds, transcripts, counted),
        );
        for (id, module) in &sealed.modules {
            let Some(mut station) = bench.station(runner, (id, module), Controlled::Every)? else {
                bench.unsealed.insert(id.clone(), Unsealed::NotListed);
                continue;
            };
            match station.owes(natives.get(id), |test| Some(test.to_owned())) {
                Ok(()) => {
                    bench.stations.insert(id.clone(), station);
                }
                Err(why) => {
                    bench.unsealed.insert(id.clone(), why);
                }
            }
        }
        for (id, doctests) in &sealed.doctests {
            let panicking: BTreeSet<String> = match natives.get(id) {
                Some(ran) => ran.should_panic.iter().cloned().collect(),
                None => BTreeSet::new(),
            };
            let Some(mut station) =
                bench.documented(runner, (doctests, &panicking), Controlled::Every)?
            else {
                bench
                    .unsealed
                    .insert(id.clone(), Unsealed::DoctestsUnaccounted);
                continue;
            };
            match station.owes(natives.get(id), super::doctest::natively_run) {
                Ok(()) => {
                    bench.stations.insert(id.clone(), station);
                }
                Err(why) => {
                    bench.unsealed.insert(id.clone(), why);
                }
            }
        }
        Ok(bench)
    }

    /// A bench with no station yet, every target `sealed` built no module for unsealed for the reason it gives, whose executions will run inside `tree` with `harness`, reading touch logs against `catalog` within `bounds`, stop when `interrupt` is raised, are remembered through `transcripts`, and are counted into `counted`.
    pub(super) fn unassembled(
        (runner, cancel): (&'runner SealedRunner, crate::runner::Cancel),
        sealed: &SealedBuild,
        (tree, harness): (Tree, &Configured),
        (catalog, bounds, transcripts, counted): (&str, crate::touch::Bounds, Transcripts, Counted),
    ) -> Self {
        Self {
            runner,
            interrupt: cancel.interrupt(),
            cancel,
            compiled: None,
            stations: BTreeMap::new(),
            unsealed: sealed.unsealed.clone(),
            tree,
            harness: harness.clone(),
            catalog: catalog.to_owned(),
            bounds,
            transcripts,
            counted,
        }
    }

    /// Replaces the modules with one compiled mutant while retaining only the original tests and controls.
    pub(crate) fn rebuilt(&self, sealed: &SealedBuild, index: u32) -> Result<Self, BenchError> {
        let mut bench = Self::unassembled(
            (self.runner, self.cancel.clone()),
            sealed,
            (self.tree.clone(), &self.harness),
            (
                &self.catalog,
                self.bounds,
                self.transcripts.clone(),
                self.counted.clone(),
            ),
        );
        bench.compiled = Some(index);
        let none = BTreeSet::new();
        for (id, original) in &self.stations {
            let panicking = original
                .holdings
                .iter()
                .flat_map(|holding| &holding.tests)
                .filter_map(|(name, run)| match run {
                    Run::Doctest {
                        expects: Expects::Panic,
                        ..
                    } => Some(name.clone()),
                    Run::Libtest
                    | Run::Doctest {
                        expects: Expects::Return,
                        ..
                    } => None,
                })
                .collect();
            let station = if let Some(module) = sealed.modules.get(id) {
                bench.station(self.runner, (id, module), Controlled::Only(&none))?
            } else if let Some(doctests) = sealed.doctests.get(id) {
                bench.documented(self.runner, (doctests, &panicking), Controlled::Only(&none))?
            } else {
                continue;
            };
            if let Some(mut station) = station {
                station.controls = original
                    .controls
                    .iter()
                    .map(|(test, control)| {
                        (
                            test.clone(),
                            if station.names(test) {
                                control.clone()
                            } else {
                                Err(Uncontrolled::Unsealed)
                            },
                        )
                    })
                    .collect();
                bench.stations.insert(id.clone(), station);
            } else {
                bench.unsealed.insert(id.clone(), Unsealed::NotListed);
            }
        }
        Ok(bench)
    }

    /// Whether this bench holds the separately compiled constant selector of this index.
    #[must_use]
    pub(crate) fn compiles(&self, index: u32) -> bool {
        self.compiled == Some(index)
    }

    /// The station of `module`, the sealed module of target `id`, prepared on `runner`: every test its harness lists, with the control of each `controlled` asks for; nothing where its harness does not list its tests.
    ///
    /// # Errors
    /// A module that cannot be read, an environment that is not text, a host that cannot run what it is given, or [`BenchError::Interrupted`].
    pub(super) fn station(
        &self,
        runner: &'runner SealedRunner,
        (id, module): (&str, &super::Module),
        controlled: Controlled<'_>,
    ) -> Result<Option<Station<'runner>>, BenchError> {
        let program = match module
            .target
            .executable
            .file_name()
            .and_then(|name| name.to_str())
        {
            Some(program) => program.to_owned(),
            None => "test".to_owned(),
        };
        let module_of = prepared((runner, &self.counted), &module.target.executable, id)?;
        let mut station = Station {
            holdings: Vec::new(),
            target: module.target.clone(),
            program,
            built: Built::of(&module.target)?,
            controls: BTreeMap::new(),
        };
        let Some(tests) = self.listed(&station, &module_of)? else {
            return Ok(None);
        };
        for test in tests.iter().filter(|test| controlled.asks(test)) {
            let control = self.control(&station, &module_of, (test, Run::Libtest))?;
            station.controls.insert(test.clone(), control);
        }
        station.holdings.push(Holding {
            module: module_of,
            tests: tests.into_iter().map(|test| (test, Run::Libtest)).collect(),
        });
        Ok(Some(station))
    }

    /// How the sealed build answers for each target it was given, by target identity.
    #[must_use]
    pub fn answering(&self) -> BTreeMap<String, Answering> {
        let mut answering: BTreeMap<String, Answering> = self
            .unsealed
            .iter()
            .map(|(id, why)| (id.clone(), Answering::Unsealed(*why)))
            .collect();
        for (id, station) in &self.stations {
            let uncontrolled = station
                .controls
                .iter()
                .filter_map(|(test, control)| match control {
                    Ok(_) => None,
                    Err(why) => Some((test.clone(), *why)),
                })
                .collect();
            answering.insert(id.clone(), Answering::Station { uncontrolled });
        }
        answering
    }

    /// The station of one library's captured doctests, with the control of each `controlled` asks for, or nothing where a merged binary did not list the doctests it holds; a merged doctest passes by panicking where its name is among `panicking`, the doctests the native run said should panic, and by returning otherwise, and one the sealed target ignores, which runs as nothing when asked for by index, is not held at all.
    pub(super) fn documented(
        &self,
        runner: &'runner SealedRunner,
        (doctests, panicking): (&super::Doctests, &BTreeSet<String>),
        controlled: Controlled<'_>,
    ) -> Result<Option<Station<'runner>>, BenchError> {
        let id = doctests.target.id();
        let mut station = Station {
            holdings: Vec::new(),
            target: doctests.target.clone(),
            program: "rust_out.wasm".to_owned(),
            built: Built::of(&doctests.target)?,
            controls: BTreeMap::new(),
        };
        let mut ignored = BTreeSet::new();
        for binary in &doctests.captured.ignored {
            let module = prepared((runner, &self.counted), binary, id)?;
            let Some(names) = self.ignored(&station, &module)? else {
                return Ok(None);
            };
            ignored.extend(names);
        }
        let mut held = Vec::new();
        for binary in &doctests.captured.merged {
            let module = prepared((runner, &self.counted), binary, id)?;
            let Some(names) = self.merged(&station, &module)? else {
                return Ok(None);
            };
            let tests = names
                .into_iter()
                .enumerate()
                .filter(|(_, name)| !ignored.contains(name.as_str()))
                .map(|(index, name)| {
                    let expects = if panicking.contains(&name) {
                        Expects::Panic
                    } else {
                        Expects::Return
                    };
                    let run = Run::Doctest {
                        index: Some(index),
                        expects,
                    };
                    (name, run)
                })
                .collect();
            held.push(Holding { module, tests });
        }
        for alone in &doctests.captured.alone {
            let module = prepared((runner, &self.counted), &alone.binary, id)?;
            let run = Run::Doctest {
                index: None,
                expects: alone.expects,
            };
            let tests = BTreeMap::from([(alone.name.clone(), run)]);
            held.push(Holding { module, tests });
        }
        for holding in held {
            if holding.tests.keys().any(|name| station.names(name)) {
                return Ok(None);
            }
            for (name, run) in holding
                .tests
                .iter()
                .filter(|(name, _)| controlled.asks(name))
            {
                let control = self.control(&station, &holding.module, (name, *run))?;
                station.controls.insert(name.clone(), control);
            }
            station.holdings.push(holding);
        }
        for name in &doctests.captured.unbuilt {
            station
                .controls
                .insert(name.clone(), Err(Uncontrolled::Unbuilt));
        }
        Ok(Some(station))
    }

    /// The doctests the merged binary `module` of `station` holds, in index order from the first, as its harness lists them when it runs with no index, where the binary refuses the index past the last it listed, which holds the listing to every doctest it holds.
    fn merged(
        &self,
        station: &Station<'_>,
        module: &SealedModule<'_>,
    ) -> Result<Option<Vec<String>>, BenchError> {
        let all = Asking {
            arguments: Vec::new(),
            index: None,
        };
        let transcript = self.invoke(
            station,
            module,
            &self.invocation(station, all, (Active::Control, CONTROL_FUEL))?,
        )?;
        if transcript.stop() != SealedStop::Returned {
            return Ok(None);
        }
        let Some(names) = listed(transcript.stdout().bytes()) else {
            return Ok(None);
        };
        let after = Asking {
            arguments: Vec::new(),
            index: Some(names.len()),
        };
        let beyond = self.invoke(
            station,
            module,
            &self.invocation(station, after, (Active::Control, CONTROL_FUEL))?,
        )?;
        let refused = matches!(
            beyond.stop(),
            SealedStop::Trapped {
                kind: TrapKind::Unreachable
            }
        ) && holds(beyond.stderr().bytes(), NO_SUCH_INDEX);
        Ok(refused.then_some(names))
    }

    /// The doctests the merged binary `module` of `station` holds that the sealed target ignores, as its harness lists them when it runs with no index, where the binary was built to list those alone.
    fn ignored(
        &self,
        station: &Station<'_>,
        module: &SealedModule<'_>,
    ) -> Result<Option<Vec<String>>, BenchError> {
        let all = Asking {
            arguments: Vec::new(),
            index: None,
        };
        let transcript = self.invoke(
            station,
            module,
            &self.invocation(station, all, (Active::Control, CONTROL_FUEL))?,
        )?;
        if transcript.stop() != SealedStop::Returned {
            return Ok(None);
        }
        Ok(listed(transcript.stdout().bytes()))
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
    ) -> Result<Option<super::standing::Put>, BenchError> {
        let Some(asked) = self.asked(target, test) else {
            return Ok(None);
        };
        let invocation = asked.invocation(self, Active::Mutant(mutant))?;
        let transcript = self.invoke(asked.station, asked.module, &invocation)?;
        let came_to = asked.judged(&transcript)?;
        Ok(Some(super::standing::Put {
            target: target.to_owned(),
            test: test.to_owned(),
            came_to,
            transcript: transcript.digest().to_string(),
        }))
    }

    /// Whether this bench answers for `test` of `target` at the guard `index`: its station holds the test, and the test's control passed and reached the guard.
    #[must_use]
    pub fn reaches(&self, target: &str, test: &str, index: u32) -> bool {
        self.asked(target, test)
            .is_some_and(|asked| asked.control.reached.contains(&index))
    }

    /// What `test` of `target` comes to with the crash `mutant` active, the runtime told to publish its notice under `nonce` at [`CRASH_NOTICE`], where the host halts it, or nothing where there is no control to judge it against (ADR 0035, amended by ADR 0046).
    ///
    /// # Errors
    /// An environment that is not text, or a host that cannot run the invocation.
    pub fn crash(
        &self,
        (target, test): (&str, &str),
        (mutant, nonce): (&str, &str),
    ) -> Result<Option<Crashed>, BenchError> {
        let Some(asked) = self.asked(target, test) else {
            return Ok(None);
        };
        let invocation = asked.invocation(self, Active::Crash { mutant, nonce })?;
        let transcript = self.invoke(asked.station, asked.module, &invocation)?;
        let ended = match transcript.stop() {
            SealedStop::Halted => Crashing::Halted,
            SealedStop::Returned
            | SealedStop::Exited { .. }
            | SealedStop::Trapped { .. }
            | SealedStop::FuelExhausted
            | SealedStop::MemoryExhausted => Crashing::Judged(asked.judged(&transcript)?),
        };
        let read = match written(&transcript, CRASH_NOTICE).map(<[u8]>::to_vec) {
            Some(bytes) => match String::from_utf8(bytes) {
                Ok(said) => Some(said),
                Err(_not_text) => None,
            },
            None => None,
        };
        let left: Vec<OverlayEntry> = transcript
            .overlay()
            .iter()
            .filter(|entry| inside(RECORDS, &entry.path).is_none())
            .cloned()
            .collect();
        let named = left
            .iter()
            .map(|entry| self.named(asked.station, entry))
            .collect();
        Ok(Some(Crashed {
            ended,
            exit: match transcript.stop() {
                SealedStop::Exited { code } => Some(code),
                SealedStop::Returned
                | SealedStop::Trapped { .. }
                | SealedStop::FuelExhausted
                | SealedStop::MemoryExhausted
                | SealedStop::Halted => None,
            },
            read,
            left,
            named,
        }))
    }

    /// What `test` of `target` comes to with nothing active, in a fresh instance started from everything `crashed` left but the runtime's own records, judged against its control, or that what it left is no state such an instance starts in; nothing where there is no control to judge it against.
    ///
    /// # Errors
    /// An environment that is not text, or a host that cannot run the invocation.
    pub fn after(
        &self,
        (target, test): (&str, &str),
        crashed: &Crashed,
    ) -> Result<Option<After>, BenchError> {
        let Some(asked) = self.asked(target, test) else {
            return Ok(None);
        };
        let mut invocation = asked.invocation(self, Active::Next)?;
        invocation.preopens = match invocation.preopens.after(&crashed.left) {
            Ok(preopens) => preopens,
            Err(refused) => return Ok(Some(After::Unstartable(refused.to_string()))),
        };
        let transcript = self.invoke(asked.station, asked.module, &invocation)?;
        Ok(Some(After::Came(asked.judged(&transcript)?)))
    }

    /// The station, control, module and run of `test` of `target`, where the bench has a control of it to judge an execution against.
    fn asked<'a>(&'a self, target: &'a str, test: &'a str) -> Option<Answerable<'a>> {
        let station = self.stations.get(target)?;
        let (Some(Ok(control)), Some((module, run))) =
            (station.controls.get(test), station.holding(test))
        else {
            return None;
        };
        Some(Answerable {
            target,
            test,
            station,
            control,
            module,
            run,
        })
    }

    /// How `entry`, a change an instance of `station` made outside the runtime's records, is named among what a crash left: below its temporary directory relative to it, below its home from `~/`, below the tree from `./`, below `CARGO_TARGET_TMPDIR` from `$CARGO_TARGET_TMPDIR/`, a directory with a trailing `/`, and a removal after ` (removed)`.
    fn named(&self, station: &Station<'_>, entry: &OverlayEntry) -> String {
        let target_tmpdir = station
            .target
            .cargo_env
            .for_process()
            .find(|(name, _)| name.to_str() == Some(TARGET_TMPDIR))
            .and_then(|(_, value)| value.to_str().map(ToOwned::to_owned));
        let named = inside(SCRATCH_TMP, &entry.path)
            .map(ToOwned::to_owned)
            .or_else(|| inside(SCRATCH_HOME, &entry.path).map(|rest| format!("~/{rest}")))
            .or_else(|| {
                target_tmpdir.as_deref().and_then(|root| {
                    inside(root, &entry.path).map(|rest| format!("$CARGO_TARGET_TMPDIR/{rest}"))
                })
            })
            .or_else(|| inside(&self.tree.root, &entry.path).map(|rest| format!("./{rest}")));
        let below = match named {
            Some(below) => below,
            None => entry.path.clone(),
        };
        match entry.state {
            OverlayState::File { .. } => below,
            OverlayState::Directory { .. } if below.ends_with('/') => below,
            OverlayState::Directory { .. } => format!("{below}/"),
            OverlayState::Removed => format!("{below} (removed)"),
        }
    }

    /// What `module` of `station` did under `invocation`, unless the run was interrupted first: what an identical invocation established in an earlier run, where one is remembered, and what the host saw otherwise, remembered for the next one.
    fn invoke(
        &self,
        station: &Station<'_>,
        module: &SealedModule<'_>,
        invocation: &Invocation,
    ) -> Result<Transcript, BenchError> {
        if self.interrupt.raised() {
            return Err(BenchError::Interrupted);
        }
        let of = invocation.digest(module.digest(), module.configuration());
        if let Some(remembered) = self.transcripts.recall(&of) {
            self.counted.answered().map_err(|source| BenchError::Host {
                target: station.target.id().to_owned(),
                source,
            })?;
            return Ok(remembered);
        }
        let transcript = module
            .invoke_counted(invocation, &self.interrupt, &self.counted)
            .map_err(|source| {
                if matches!(source, SealedError::Interrupted) {
                    BenchError::Interrupted
                } else {
                    BenchError::Host {
                        target: station.target.id().to_owned(),
                        source,
                    }
                }
            })?;
        self.transcripts.remember(&of, &transcript);
        Ok(transcript)
    }

    fn listed(
        &self,
        station: &Station<'_>,
        module: &SealedModule<'_>,
    ) -> Result<Option<Vec<String>>, BenchError> {
        let asking = Asking {
            arguments: [vec!["--list".to_owned()], self.harness.beside(&[])].concat(),
            index: None,
        };
        let invocation = self.invocation(station, asking, (Active::Control, CONTROL_FUEL))?;
        let transcript = self.invoke(station, module, &invocation)?;
        if transcript.stop() != SealedStop::Returned {
            return Ok(None);
        }
        Ok(crate::libtest::listing(transcript.stdout().bytes()))
    }

    fn control(
        &self,
        station: &Station<'_>,
        module: &SealedModule<'_>,
        (name, run): (&str, Run),
    ) -> Result<Result<Control, Uncontrolled>, BenchError> {
        let invocation = self.invocation(
            station,
            run.asking(name, &self.harness),
            (Active::Control, CONTROL_FUEL),
        )?;
        let transcript = self.invoke(station, module, &invocation)?;
        let came_to = judged(observed(&transcript, (name, run), None));
        let unread = Uncontrolled::Doubted(rust_mutants_decision::evidence::Doubt::Unaccounted);
        if holds(transcript.stderr().bytes(), NO_SUCH_INDEX) {
            return Ok(Err(unread));
        }
        match came_to {
            Sealed::Passed => {}
            Sealed::Detected(_) if met_the_sandbox(&transcript) => {
                return Ok(Err(Uncontrolled::Doubted(
                    rust_mutants_decision::evidence::Doubt::Refused,
                )));
            }
            Sealed::Detected(how) => return Ok(Err(Uncontrolled::Detected(how))),
            Sealed::Doubted(why) => return Ok(Err(Uncontrolled::Doubted(why))),
            Sealed::SetAside => return Ok(Err(unread)),
        }
        let reached = match written(&transcript, TOUCH_LOG).map(std::str::from_utf8) {
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
            declined: declined(&transcript).map(<[u8]>::to_vec),
        }))
    }

    /// Every directory an instance of `station` is given: the tree, the records, its scratch, an empty `target_tmpdir` where cargo names one, what its build script wrote where one did, and the root, the instance starting in its package's directory where the tree holds it.
    fn preopens(
        &self,
        station: &Station<'_>,
        target_tmpdir: Option<String>,
    ) -> Result<Preopens, SealedError> {
        let scratch = Snapshot::builder()
            .directory("home")?
            .directory("tmp")?
            .build()?;
        let mut preopens = vec![
            Preopen::Tree {
                path: self.tree.root.clone(),
                snapshot: self.tree.snapshot.clone(),
            },
            Preopen::Tree {
                path: RECORDS.to_owned(),
                snapshot: Snapshot::builder().build()?,
            },
            Preopen::Tree {
                path: SCRATCH.to_owned(),
                snapshot: scratch,
            },
        ];
        if let Some(path) = target_tmpdir {
            preopens.push(Preopen::Tree {
                path,
                snapshot: Snapshot::builder().build()?,
            });
        }
        if let Some(built) = &station.built {
            preopens.push(Preopen::Tree {
                path: built.path.clone(),
                snapshot: built.snapshot.clone(),
            });
        }
        let start = self
            .tree
            .within(&station.target.cwd)
            .map(|directory| Start {
                tree: self.tree.root.clone(),
                directory,
            });
        preopens.push(Preopen::Root { start });
        Preopens::new(preopens)
    }

    fn invocation(
        &self,
        station: &Station<'_>,
        asking: Asking,
        (active, fuel): (Active<'_>, u64),
    ) -> Result<Invocation, BenchError> {
        let id = station.target.id().to_owned();
        let mut arguments = vec![station.program.clone()];
        arguments.extend(asking.arguments);
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
        variables.push((
            crate::decline::DECLINE_NOTICE_ENV.to_owned(),
            DECLINE_LOG.to_owned(),
        ));
        let (asked, halt): (Vec<(&str, &str)>, Option<String>) = match active {
            Active::Control => (vec![(crate::instrument::TOUCH_ENV, TOUCH_LOG)], None),
            Active::Mutant(mutant) => (vec![(crate::instrument::ACTIVE_ENV, mutant)], None),
            Active::Crash { mutant, nonce } => (
                vec![
                    (crate::instrument::ACTIVE_ENV, mutant),
                    (crate::instrument::CRASH_NOTICE_ENV, CRASH_NOTICE),
                    (crate::instrument::CRASH_NONCE_ENV, nonce),
                ],
                Some(CRASH_NOTICE.to_owned()),
            ),
            Active::Next => (Vec::new(), None),
        };
        variables.extend(
            asked
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value.to_owned())),
        );
        if let Some(index) = asking.index {
            variables.push((RUN_ONE.to_owned(), index.to_string()));
        }
        let target_tmpdir = variables
            .iter()
            .find(|(name, _)| name == TARGET_TMPDIR)
            .map(|(_, value)| value.clone());
        variables.retain(|(name, _)| name != "HOME" && name != "TMPDIR");
        variables.push(("HOME".to_owned(), SCRATCH_HOME.to_owned()));
        variables.push(("TMPDIR".to_owned(), SCRATCH_TMP.to_owned()));
        let host = |source| BenchError::Host {
            target: id.clone(),
            source,
        };
        Ok(Invocation {
            arguments: Arguments::new(arguments).map_err(host)?,
            environment: Environment::new(variables).map_err(host)?,
            preopens: self.preopens(station, target_tmpdir).map_err(host)?,
            seed: seed(station.target.id()),
            fuel,
            limits: LIMITS,
            clock: CLOCK,
            halt,
        })
    }
}

/// Where a crashed instance publishes the notice of its stop, and where the host halts it: a file of the runtime's records, which the test does not see.
pub const CRASH_NOTICE: &str = "/rust-mutants-sealed/crash-notice";

/// What an instance runs with.
#[derive(Debug, Clone, Copy)]
enum Active<'a> {
    /// Nothing, recording every guard it reaches: a control, or a listing.
    Control,
    /// A mutant.
    Mutant(&'a str),
    /// A crash, whose runtime publishes its notice under the nonce at [`CRASH_NOTICE`], where the host halts the instance.
    Crash {
        /// The mutation that puts the crash.
        mutant: &'a str,
        /// The nonce issued to this instance alone.
        nonce: &'a str,
    },
    /// Nothing, over what a crash left: a next run.
    Next,
}

/// One test a bench can judge an execution of: its station, its control, and the module and run that hold it.
struct Answerable<'a> {
    target: &'a str,
    test: &'a str,
    station: &'a Station<'a>,
    control: &'a Control,
    module: &'a SealedModule<'a>,
    run: Run,
}

impl Answerable<'_> {
    /// The invocation of this test with `active`, allowed its multiple of the control's fuel.
    fn invocation(&self, bench: &Bench<'_>, active: Active<'_>) -> Result<Invocation, BenchError> {
        let budget = match self.control.declined {
            Some(_) => CONTROL_FUEL,
            None => self
                .control
                .fuel
                .saturating_mul(FUEL_FACTOR)
                .saturating_add(FUEL_FLOOR),
        };
        bench.invocation(
            self.station,
            self.run.asking(self.test, &bench.harness),
            (active, budget),
        )
    }

    /// What an execution of this test that left `transcript` came to, judged against its control.
    fn judged(&self, transcript: &Transcript) -> Result<Sealed, BenchError> {
        match (declined(transcript), self.control.declined.as_deref()) {
            (Some(said), Some(before)) if said == before => return Ok(Sealed::SetAside),
            (Some(_), Some(_) | None) => {
                return Ok(Sealed::Detected(
                    rust_mutants_decision::evidence::Detection::Declined,
                ));
            }
            (None, Some(_) | None) => {}
        }
        let observed = observed(transcript, (self.test, self.run), Some(self.control));
        let came_to = judged(observed);
        if judgement_keeps_the_pass_rule(observed, came_to) {
            Ok(came_to)
        } else {
            Err(BenchError::JudgementContradicted {
                target: self.target.to_owned(),
                test: self.test.to_owned(),
            })
        }
    }
}

/// How an instance a crash was put to ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Crashing {
    /// The host halted it where its runtime published a notice: nothing it would have run after the call ran.
    Halted,
    /// It did not halt, and came to this, judged against the test's control.
    Judged(Sealed),
}

/// What a next instance over what a crash left came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum After {
    /// It ran, and came to this, judged against the test's control.
    Came(Sealed),
    /// What the crash left is no state an instance of the test can start in, such as one that removed the directory it runs in, and why.
    Unstartable(String),
}

/// What an instance a crash was put to came to, and what it left for a next instance to start over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crashed {
    /// How it ended.
    pub ended: Crashing,
    /// The status it exited with, where it ended by exiting.
    pub exit: Option<u32>,
    /// The notice its runtime published at [`CRASH_NOTICE`], as the instance left it, or nothing where it left none that is text.
    pub read: Option<String>,
    /// Every change it made outside the runtime's records, in the order its overlay lists them.
    left: Vec<OverlayEntry>,
    /// Each of those changes, named as a crash's `left` names one.
    pub named: Vec<String>,
}

/// Where `path` is below the directory `root`, where it is below it at all: empty for `root` itself.
fn inside<'a>(root: &str, path: &'a str) -> Option<&'a str> {
    let rest = path.strip_prefix(root.trim_end_matches('/'))?;
    if rest.is_empty() {
        return Some("");
    }
    rest.strip_prefix('/')
}

/// The seed of every instance of `target`, the same for its control and for every mutant's execution.
#[must_use]
pub fn seed(target: &str) -> u64 {
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

/// What the host observed of one execution of `test`, run as `run`, against `control` where there is one.
fn observed(
    transcript: &Transcript,
    (test, run): (&str, Run),
    control: Option<&Control>,
) -> Observed {
    let stderr = transcript.stderr().bytes();
    let failure = match run {
        Run::Libtest => crate::libtest::FAILURE_STATUS,
        Run::Doctest { .. } => super::doctest::FAILURE_STATUS,
    };
    let ending = match transcript.stop() {
        SealedStop::Returned => Ending::Returned,
        SealedStop::Exited { code: 0 } => Ending::ExitedZero,
        SealedStop::Exited { code } if i32::try_from(code).is_ok_and(|code| code == failure) => {
            Ending::ExitedFailure
        }
        SealedStop::Exited { .. } | SealedStop::Halted => Ending::ExitedOther,
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
    let harness = match run {
        Run::Libtest => Harness::Libtest(accounted(transcript, test)),
        Run::Doctest { expects, .. } => expects.harness(),
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
        harness,
        beyond_control,
        matched: control.is_none_or(|control| control.declined.is_none()),
    }
}

/// What libtest's account in `transcript` says of the one test `test` it was asked to run.
fn accounted(transcript: &Transcript, test: &str) -> Account {
    let exit = match transcript.stop() {
        SealedStop::Returned => Some(0),
        SealedStop::Exited { code } => match i32::try_from(code) {
            Ok(code) => Some(code),
            Err(_wider) => None,
        },
        SealedStop::Trapped { .. }
        | SealedStop::FuelExhausted
        | SealedStop::MemoryExhausted
        | SealedStop::Halted => None,
    };
    let asked = [test.to_owned()];
    match harness_report(transcript.stdout().bytes(), Asked::Exact(&asked), exit) {
        Ok(accounted) if accounted.summary.passed == 1 && accounted.summary.failed == 0 => {
            Account::Passed
        }
        Ok(accounted) if accounted.summary.failed == 1 && accounted.failed == asked => {
            Account::Failed
        }
        Ok(_) | Err(_) => Account::Other,
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

/// Whether the instance met a refusal of the sandbox: the host refused a call, or the standard library printed a message of its own for one.
fn met_the_sandbox(transcript: &Transcript) -> bool {
    !transcript.refusals().is_empty() || !sandbox(transcript).is_empty()
}

/// Every message of the standard library's for a refusal of the sandbox that the instance printed.
fn sandbox(transcript: &Transcript) -> BTreeSet<&'static str> {
    let stderr = transcript.stderr().bytes();
    SANDBOX_WORDS
        .into_iter()
        .filter(|words| holds(stderr, words))
        .collect()
}

/// The bytes an instance left at `path` of the records it was given, where it wrote any.
/// Whether `came_to` keeps the rule a pass keeps: an execution is passed exactly when it ended as its harness passes by and met nothing its control did not.
///
/// This is the rule `judged` is proved to keep, said again apart from it, so a judgement that broke it stops the run rather than become a verdict.
fn judgement_keeps_the_pass_rule(observed: Observed, came_to: Sealed) -> bool {
    let passing = !observed.beyond_control
        && match observed.harness {
            Harness::Libtest(harness_report) => {
                observed.ending == Ending::Returned && harness_report == Account::Passed
            }
            Harness::Doctest => observed.ending == Ending::Returned,
            Harness::ShouldPanic => matches!(
                observed.ending,
                Ending::ExitedFailure | Ending::Panicked | Ending::Aborted | Ending::Trapped
            ),
        };
    passing == (came_to == Sealed::Passed)
}

/// The words the test declined to measure in, where it wrote any (ADR 0043).
fn declined(transcript: &Transcript) -> Option<&[u8]> {
    written(transcript, DECLINE_LOG).filter(|notice| !notice.is_empty())
}

fn written<'transcript>(
    transcript: &'transcript Transcript,
    path: &str,
) -> Option<&'transcript [u8]> {
    transcript.overlay().iter().find_map(|entry| {
        if entry.path != path {
            return None;
        }
        match &entry.state {
            OverlayState::File { contents, .. } => Some(contents.as_slice()),
            OverlayState::Directory { .. } | OverlayState::Removed => None,
        }
    })
}

#[cfg(test)]
mod tests;
