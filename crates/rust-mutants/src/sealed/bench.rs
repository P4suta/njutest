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
    Arguments, ClockPolicy, Environment, Interrupt, Invocation, Limits, OverlayState, Preopen,
    Preopens, RefusalReason, SealedError, SealedModule, SealedRunner, SealedStop, Snapshot,
    Transcript, TrapKind, WasiFunction,
};

use super::doctest::{Expects, Listed, NO_SUCH_INDEX, RUN_ONE, listed};
use super::{SealedBuild, Unsealed};
use crate::execute::TestTarget;
use crate::libtest::{Asked, Configured, Own, account};

/// Where the runtime's records land inside an instance: a directory nothing but the runtime writes.
pub const RECORDS: &str = "/rust-mutants-sealed";

/// The touch log a control's runtime writes.
const TOUCH_LOG: &str = "/rust-mutants-sealed/touch.log";

/// Where a test that cannot measure on the sealed host says so, as ADR 0043 lets it.
const DECLINE_LOG: &str = "/rust-mutants-sealed/decline-notice";

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
        let spelled: PathBuf = root.components().collect();
        let root = match spelled.to_str() {
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
pub enum Uncontrolled {
    /// Its control detected something with nothing active: it fails sealed.
    Detected(rust_mutants_decision::evidence::Detection),
    /// Its control established nothing, for this reason.
    Doubted(rust_mutants_decision::evidence::Doubt),
    /// Its control declined to measure on the sealed host (ADR 0043), so it passed having measured nothing.
    Declined,
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
            Self::Declined => "declined",
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
        self.controls.retain(|test, _| owed.contains(test));
        for test in owed {
            self.controls
                .entry(test)
                .or_insert(Err(Uncontrolled::Unsealed));
        }
        Ok(())
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

/// Every sealed module of a session, prepared, listed and controlled.
#[derive(Debug)]
pub struct Bench<'runner> {
    /// Each target's station, by target identity.
    pub stations: BTreeMap<String, Station<'runner>>,
    /// Each target with no station, and why.
    pub unsealed: BTreeMap<String, Unsealed>,
    tree: Tree,
    harness: Configured,
    catalog: String,
    bounds: crate::touch::Bounds,
    interrupt: Interrupt,
}

/// `path`, read and prepared on `runner` as a module of `target`.
fn prepared<'runner>(
    runner: &'runner SealedRunner,
    path: &Path,
    target: &str,
) -> Result<SealedModule<'runner>, BenchError> {
    let bytes = std::fs::read(path).map_err(|source| BenchError::ModuleUnreadable {
        path: path.to_path_buf(),
        source,
    })?;
    runner.prepare(&bytes).map_err(|source| BenchError::Host {
        target: target.to_owned(),
        source,
    })
}

impl<'runner> Bench<'runner> {
    /// Prepares every module of `sealed` on `runner`, lists its tests and runs each one's control inside `tree`, every libtest invocation given the harness arguments `harness` as the native ones are, less the options it sets itself, and holds each station to every test its target's native baseline in `natives` ran; every execution, then and later, stops when `interrupt` is raised.
    ///
    /// # Errors
    /// A module that cannot be read, an environment that is not text, a host that cannot run what it is given, or [`BenchError::Interrupted`].
    pub fn assemble(
        (runner, interrupt): (&'runner SealedRunner, Interrupt),
        (sealed, natives): (&SealedBuild, &BTreeMap<String, Ran>),
        (tree, harness): (Tree, &Configured),
        (catalog, bounds): (&str, crate::touch::Bounds),
    ) -> Result<Self, BenchError> {
        let mut bench = Self {
            stations: BTreeMap::new(),
            unsealed: sealed.unsealed.clone(),
            tree,
            harness: harness.clone(),
            catalog: catalog.to_owned(),
            bounds,
            interrupt,
        };
        for (id, module) in &sealed.modules {
            let program = match module
                .target
                .executable
                .file_name()
                .and_then(|name| name.to_str())
            {
                Some(program) => program.to_owned(),
                None => "test".to_owned(),
            };
            let module_of = prepared(runner, &module.target.executable, id)?;
            let mut station = Station {
                holdings: Vec::new(),
                target: module.target.clone(),
                program,
                controls: BTreeMap::new(),
            };
            let Some(tests) = bench.listed(&station, &module_of)? else {
                bench.unsealed.insert(id.clone(), Unsealed::NotListed);
                continue;
            };
            let mut controls = BTreeMap::new();
            for test in &tests {
                let control = bench.control(&station, &module_of, (test, Run::Libtest))?;
                controls.insert(test.clone(), control);
            }
            station.holdings.push(Holding {
                module: module_of,
                tests: tests.into_iter().map(|test| (test, Run::Libtest)).collect(),
            });
            station.controls = controls;
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
            let Some(mut station) = bench.documented(runner, doctests)? else {
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

    /// The station of one library's captured doctests, or nothing where a merged binary did not name the doctests it holds.
    fn documented(
        &self,
        runner: &'runner SealedRunner,
        doctests: &super::Doctests,
    ) -> Result<Option<Station<'runner>>, BenchError> {
        let id = doctests.target.id();
        let mut station = Station {
            holdings: Vec::new(),
            target: doctests.target.clone(),
            program: "rust_out.wasm".to_owned(),
            controls: BTreeMap::new(),
        };
        let mut held = Vec::new();
        for binary in &doctests.captured.merged {
            let module = prepared(runner, binary, id)?;
            let Some(listed) = self.merged(&station, &module)? else {
                return Ok(None);
            };
            let tests = listed
                .into_iter()
                .enumerate()
                .filter(|(_, doctest)| !doctest.ignored || doctest.expects == Expects::Panic)
                .map(|(index, doctest)| {
                    let run = Run::Doctest {
                        index: Some(index),
                        expects: doctest.expects,
                    };
                    (doctest.name, run)
                })
                .collect();
            held.push(Holding { module, tests });
        }
        for alone in &doctests.captured.alone {
            let module = prepared(runner, &alone.binary, id)?;
            let run = Run::Doctest {
                index: None,
                expects: alone.expects,
            };
            let tests = BTreeMap::from([(alone.name.clone(), run)]);
            held.push(Holding { module, tests });
        }
        for holding in held {
            let shared = holding.tests.keys().any(|name| station.names(name));
            let listed_twice =
                holding.tests.len() != holding.tests.keys().collect::<BTreeSet<_>>().len();
            if shared || listed_twice {
                return Ok(None);
            }
            for (name, run) in &holding.tests {
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

    /// The doctests the merged binary `module` of `station` holds, in index order, where it named them all when it ran them in one instance and holds no doctest past the last it named.
    fn merged(
        &self,
        station: &Station<'_>,
        module: &SealedModule<'_>,
    ) -> Result<Option<Vec<Listed>>, BenchError> {
        let all = Asking {
            arguments: Vec::new(),
            index: None,
        };
        let transcript = self.invoke(
            station,
            module,
            &self.invocation(station, all, (None, CONTROL_FUEL))?,
        )?;
        let ended = matches!(
            transcript.stop(),
            SealedStop::Returned | SealedStop::Exited { code: 101 }
        );
        let Some(listed) = listed(transcript.stdout().bytes()).filter(|_| ended) else {
            return Ok(None);
        };
        let past = Asking {
            arguments: Vec::new(),
            index: Some(listed.len()),
        };
        let beyond = self.invoke(
            station,
            module,
            &self.invocation(station, past, (None, CONTROL_FUEL))?,
        )?;
        let refused = matches!(
            beyond.stop(),
            SealedStop::Trapped {
                kind: TrapKind::Unreachable
            }
        ) && holds(beyond.stderr().bytes(), NO_SUCH_INDEX);
        Ok(refused.then_some(listed))
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
        let (Some(Ok(control)), Some((module, run))) =
            (station.controls.get(test), station.holding(test))
        else {
            return Ok(None);
        };
        let budget = control
            .fuel
            .saturating_mul(FUEL_FACTOR)
            .saturating_add(FUEL_FLOOR);
        let asking = run.asking(test, &self.harness);
        let invocation = self.invocation(station, asking, (Some(mutant), budget))?;
        let transcript = self.invoke(station, module, &invocation)?;
        if written(&transcript, DECLINE_LOG).is_some_and(|notice| !notice.is_empty()) {
            return Ok(Some(Sealed::Detected(
                rust_mutants_decision::evidence::Detection::Declined,
            )));
        }
        Ok(Some(judged(observed(
            &transcript,
            (test, run),
            Some(control),
        ))))
    }

    /// What `module` of `station` did under `invocation`, unless the run was interrupted first.
    fn invoke(
        &self,
        station: &Station<'_>,
        module: &SealedModule<'_>,
        invocation: &Invocation,
    ) -> Result<Transcript, BenchError> {
        module
            .invoke(invocation, &self.interrupt)
            .map_err(|source| {
                if matches!(source, SealedError::Interrupted) {
                    BenchError::Interrupted
                } else {
                    BenchError::Host {
                        target: station.target.id().to_owned(),
                        source,
                    }
                }
            })
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
        let invocation = self.invocation(station, asking, (None, CONTROL_FUEL))?;
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
            (None, CONTROL_FUEL),
        )?;
        let transcript = self.invoke(station, module, &invocation)?;
        let came_to = judged(observed(&transcript, (name, run), None));
        let unread = Uncontrolled::Doubted(rust_mutants_decision::evidence::Doubt::Unaccounted);
        if holds(transcript.stderr().bytes(), NO_SUCH_INDEX) {
            return Ok(Err(unread));
        }
        match came_to {
            Sealed::Passed => {}
            Sealed::Detected(how) => return Ok(Err(Uncontrolled::Detected(how))),
            Sealed::Doubted(why) => return Ok(Err(Uncontrolled::Doubted(why))),
        }
        if written(&transcript, DECLINE_LOG).is_some_and(|notice| !notice.is_empty()) {
            return Ok(Err(Uncontrolled::Declined));
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
        }))
    }

    fn invocation(
        &self,
        station: &Station<'_>,
        asking: Asking,
        (mutant, fuel): (Option<&str>, u64),
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
        let asked = match mutant {
            Some(mutant) => (crate::instrument::ACTIVE_ENV, mutant),
            None => (crate::instrument::TOUCH_ENV, TOUCH_LOG),
        };
        variables.push((asked.0.to_owned(), asked.1.to_owned()));
        if let Some(index) = asking.index {
            variables.push((RUN_ONE.to_owned(), index.to_string()));
        }
        let host = |source| BenchError::Host {
            target: id.clone(),
            source,
        };
        let records = Snapshot::builder().build().map_err(host)?;
        let mut preopens = vec![
            Preopen::Tree {
                path: self.tree.root.clone(),
                snapshot: self.tree.snapshot.clone(),
            },
            Preopen::Tree {
                path: RECORDS.to_owned(),
                snapshot: records,
            },
        ];
        if let Some(directory) = self.tree.within(&station.target.cwd) {
            preopens.push(Preopen::Working {
                tree: self.tree.root.clone(),
                directory,
            });
        }
        Ok(Invocation {
            arguments: Arguments::new(arguments).map_err(host)?,
            environment: Environment::new(variables).map_err(host)?,
            preopens: Preopens::new(preopens).map_err(host)?,
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
        SealedStop::Trapped { .. } | SealedStop::FuelExhausted | SealedStop::MemoryExhausted => {
            None
        }
    };
    let asked = [test.to_owned()];
    match account(transcript.stdout().bytes(), Asked::Exact(&asked), exit) {
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

/// Every message of the standard library's for a refusal of the sandbox that the instance printed.
fn sandbox(transcript: &Transcript) -> BTreeSet<&'static str> {
    let stderr = transcript.stderr().bytes();
    SANDBOX_WORDS
        .into_iter()
        .filter(|words| holds(stderr, words))
        .collect()
}

/// The bytes an instance left at `path` of the records it was given, where it wrote any.
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
