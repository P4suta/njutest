// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A stored report's sealed executions, run again rather than trusted (ADR 0046, decision 7; ADR 0003: re-run, never replay).

use std::collections::{BTreeMap, BTreeSet};

use rust_mutants_sealed::{Counted, Interrupt, SealedRunner, Transcripts};

use super::bench::{Bench, BenchError, Controlled, Tree, Uncontrolled};
use super::record::{Came, SealedRun};
use super::{SealedBuild, Unsealed};
use crate::catalog::Mutant;
use crate::libtest::Configured;
use crate::outcome::Outcome;

/// One sealed execution a report recorded: the mutant put, by full identity, the target whose module ran, the test, and what it came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    /// The mutant, by full identity.
    pub mutant: String,
    /// The target whose sealed module ran.
    pub target: String,
    /// The test it ran.
    pub test: String,
    /// What it came to when it ran.
    pub came_to: Came,
}

/// Why a recorded execution could not be made again: what it rested on is not there now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unmade {
    /// This tree catalogs no mutant by that identity.
    Uncataloged,
    /// The compiler did not accept the mutant this time, so no guard in the tree holds it.
    Rejected,
    /// The sealed build does not hold the mutant's guard.
    GuardAbsent,
    /// This build has no such target.
    Untargeted,
    /// The target has no sealed station now, for this reason.
    Unsealed(Unsealed),
    /// The test has no sealed control now, for this reason.
    Uncontrolled(Uncontrolled),
    /// The test's control does not reach the mutant now, so putting it would run the original program.
    Unreached,
}

impl std::fmt::Display for Unmade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Uncataloged => write!(f, "this tree catalogs no such mutant"),
            Self::Rejected => write!(f, "the compiler did not accept the mutant this time"),
            Self::GuardAbsent => write!(f, "the sealed build does not hold the mutant's guard"),
            Self::Untargeted => write!(f, "this build has no such target"),
            Self::Unsealed(why) => write!(f, "the target has no sealed module ({})", why.name()),
            Self::Uncontrolled(why) => write!(f, "the test has no sealed control ({})", why.name()),
            Self::Unreached => write!(f, "the test's control does not reach the mutant"),
        }
    }
}

/// What a recorded execution comes to now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Now {
    /// It ran again, came to this, and rested on the transcript this digest names.
    Came {
        /// What it came to.
        came_to: Came,
        /// The digest of the transcript the verdict rests on.
        transcript: String,
    },
    /// It could not be made again, and why.
    Unmade(Unmade),
}

impl std::fmt::Display for Now {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Came { came_to, .. } => write!(f, "{}", came_to.name()),
            Self::Unmade(why) => write!(f, "nothing, since {why}"),
        }
    }
}

/// One recorded execution, and what it came to when it was run again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reran {
    /// The execution as the report recorded it.
    pub recorded: Recorded,
    /// What it came to now.
    pub now: Now,
}

impl Reran {
    /// Whether it came to what it was recorded as.
    #[must_use]
    pub fn same(&self) -> bool {
        matches!(&self.now, Now::Came { came_to, .. } if *came_to == self.recorded.came_to)
    }
}

/// What running recorded sealed executions again found, in the order they were given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reproduction {
    /// Every one came to what it was recorded as.
    Reproduced(Vec<Reran>),
    /// The ones in `agreed` came to what they were recorded as, `first` did not, and none after it ran.
    Differed {
        /// Every execution before `first`, each the same as recorded.
        agreed: Vec<Reran>,
        /// The first that came to something else.
        first: Reran,
    },
}

/// Why a kept sealed verdict about one mutant is not believed: where its executions first part from the ones this run's bench made of it, in the order they ran, or what they establish now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Departed {
    /// The execution kept at the place `at`, counted from one, and the one this run's bench made there are not the same test coming to the same.
    Differed {
        /// The place.
        at: usize,
        /// The execution the verdict kept there.
        kept: SealedRun,
        /// The execution this run's bench made there.
        now: SealedRun,
    },
    /// This run's bench made no execution at the place `at`, where the verdict kept one.
    Unmade {
        /// The place.
        at: usize,
        /// The execution the verdict kept there.
        kept: SealedRun,
    },
    /// This run's bench made an execution at the place `at`, past every one the verdict kept.
    Unkept {
        /// The place.
        at: usize,
        /// The execution this run's bench made there.
        now: SealedRun,
    },
    /// Each of this many came to what it was kept as, and they establish no verdict now.
    Unestablished {
        /// How many there are.
        executions: usize,
    },
    /// Each came to what it was kept as, and they establish another verdict than the one kept.
    Contradicted {
        /// The verdict kept.
        kept: Outcome,
        /// The verdict they establish.
        now: Outcome,
    },
}

/// The kind of the trace note that names where a kept sealed verdict's executions first parted from this run's.
pub const UNREPRODUCED: &str = "unreproduced";

impl Departed {
    /// Where `now`, the executions this run's bench made of a mutant, first part from `kept`, the ones a kept verdict about it recorded; nothing where each came to what it recorded, in the same order.
    #[must_use]
    pub fn of(kept: &[SealedRun], now: &[SealedRun]) -> Option<Self> {
        let longest = kept.len().max(now.len());
        (1..=longest).zip(0..longest).find_map(|(at, index)| {
            match (kept.get(index), now.get(index)) {
                (Some(was), Some(is)) if was == is => None,
                (Some(was), Some(is)) => Some(Self::Differed {
                    at,
                    kept: was.clone(),
                    now: is.clone(),
                }),
                (Some(was), None) => Some(Self::Unmade {
                    at,
                    kept: was.clone(),
                }),
                (None, Some(is)) => Some(Self::Unkept {
                    at,
                    now: is.clone(),
                }),
                (None, None) => None,
            }
        })
    }

    /// That every one of `kept` came to what it recorded, and they establish no verdict now.
    #[must_use]
    pub const fn unestablished(kept: &[SealedRun]) -> Self {
        Self::Unestablished {
            executions: kept.len(),
        }
    }
}

impl std::fmt::Display for Departed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Differed { at, kept, now }
                if kept.target == now.target && kept.test == now.test =>
            {
                write!(
                    f,
                    "execution {at}, {} test {}, is kept as {} and came to {} when put again",
                    kept.target,
                    kept.test,
                    kept.came_to.name(),
                    now.came_to.name()
                )
            }
            Self::Differed { at, kept, now } => write!(
                f,
                "execution {at} is kept as {} test {}, which came to {}, and this run's bench put \
                 {} test {} there, which came to {}",
                kept.target,
                kept.test,
                kept.came_to.name(),
                now.target,
                now.test,
                now.came_to.name()
            ),
            Self::Unmade { at, kept } => write!(
                f,
                "execution {at}, {} test {}, is kept as {} and this run's bench could not make it \
                 again",
                kept.target,
                kept.test,
                kept.came_to.name()
            ),
            Self::Unkept { at, now } => write!(
                f,
                "the verdict is kept with fewer executions than this run's bench made, which went \
                 on at execution {at} to {} test {}, which came to {}",
                now.target,
                now.test,
                now.came_to.name()
            ),
            Self::Unestablished { executions } => write!(
                f,
                "all {executions} executions came to what they are kept as, and they establish no \
                 verdict now"
            ),
            Self::Contradicted { kept, now } => write!(
                f,
                "the verdict is kept as {} and its executions, each of which came to what it is \
                 kept as, establish {}",
                kept.name(),
                now.name()
            ),
        }
    }
}

/// Why `target` has no station, as `unsealed` says: the reason it has no sealed module, or that this build has no such target.
#[must_use]
pub(crate) fn unstationed(unsealed: &BTreeMap<String, Unsealed>, target: &str) -> Unmade {
    match unsealed.get(target) {
        Some(why) => Unmade::Unsealed(*why),
        None => Unmade::Untargeted,
    }
}

/// Every test recorded executions name, by the target whose module ran it.
#[must_use]
pub(crate) fn named(recorded: &[Recorded]) -> BTreeMap<String, BTreeSet<String>> {
    let mut named: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for one in recorded {
        named
            .entry(one.target.clone())
            .or_default()
            .insert(one.test.clone());
    }
    named
}

/// The stations of the targets recorded executions name, each holding exactly the tests they name with those tests' controls, and held to no native baseline: the executions already say which tests they ran, so no rule has to choose them.
#[derive(Debug)]
pub struct Rerun<'runner> {
    bench: Bench<'runner>,
}

impl<'runner> Rerun<'runner> {
    /// Prepares on `runner` the module of each target `named` names, lists its tests and runs the control of each test it names and of no other inside `tree`, every libtest invocation given `harness` as a run's are, and stops every execution when `interrupt` is raised.
    ///
    /// # Errors
    /// A module that cannot be read, an environment that is not text, a host that cannot run what it is given, or [`BenchError::Interrupted`].
    pub(crate) fn assemble(
        (runner, interrupt): (&'runner SealedRunner, Interrupt),
        (sealed, named): (&SealedBuild, &BTreeMap<String, BTreeSet<String>>),
        (tree, harness): (Tree, &Configured),
        (catalog, bounds, counted): (&str, crate::touch::Bounds, Counted),
    ) -> Result<Self, BenchError> {
        counted.assembled();
        let mut bench = Bench::unassembled(
            (interrupt, Transcripts::under(None), counted),
            sealed,
            (tree, harness),
            (catalog, bounds),
        );
        for (id, tests) in named {
            let only = Controlled::Only(tests);
            let station = if let Some(module) = sealed.modules.get(id) {
                bench
                    .station(runner, (id, module), only)?
                    .ok_or(Unsealed::NotListed)
            } else if let Some(doctests) = sealed.doctests.get(id) {
                bench
                    .documented(runner, (doctests, None), only)?
                    .ok_or(Unsealed::DoctestsUnaccounted)
            } else {
                continue;
            };
            match station {
                Ok(mut station) => {
                    station.hold_to(tests);
                    bench.stations.insert(id.clone(), station);
                }
                Err(why) => {
                    bench.unsealed.insert(id.clone(), why);
                }
            }
        }
        Ok(Self { bench })
    }

    /// What `test` of `target` comes to now with `mutant` active, judged against its control as every sealed execution is, or why it cannot be made again.
    ///
    /// # Errors
    /// An environment that is not text, a host that cannot run the invocation, or [`BenchError::Interrupted`].
    pub fn put(&self, target: &str, test: &str, mutant: &Mutant) -> Result<Now, BenchError> {
        let Some(station) = self.bench.stations.get(target) else {
            return Ok(Now::Unmade(unstationed(&self.bench.unsealed, target)));
        };
        let control = match station.controls.get(test) {
            Some(Ok(control)) => control,
            Some(Err(why)) => return Ok(Now::Unmade(Unmade::Uncontrolled(*why))),
            None => return Ok(Now::Unmade(Unmade::Uncontrolled(Uncontrolled::Unsealed))),
        };
        if !control.reached.contains(&mutant.index) {
            return Ok(Now::Unmade(Unmade::Unreached));
        }
        Ok(match self.bench.put(target, test, mutant.id.as_str())? {
            Some(put) => Now::Came {
                came_to: Came::of(put.came_to),
                transcript: put.transcript,
            },
            None => Now::Unmade(Unmade::Uncontrolled(Uncontrolled::Unsealed)),
        })
    }
}
