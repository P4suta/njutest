// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether each test binary a baseline measured is proven to run one thread: its touch record, held beside what every package it links can start.

use std::collections::BTreeMap;

use crate::concurrency::explore::{CONFIRMING_ROUNDS, Ended, chosen, clean, repeats};
use crate::concurrency::proof::{
    Evidence, Harness, PackageScan, Reach, Standing, Threads, standing, threads_of,
};
use crate::report::concurrency::{ConcurrencyRecord, Exploration, Unexplored};
use rust_mutants::execute::TargetKind;
use rust_mutants::session::{Conditions, Observing, Perturbation, Request};

/// How long a delayed guard holds each thread that reaches it, once.
pub const PAUSE_MS: u64 = 100;

/// One record per test binary the session measured, in binary order, each package of every closure read once; `harness_args` are what every libtest binary was run with.
#[must_use]
pub fn recorded(
    session: &rust_mutants::session::Session,
    harness_args: &[String],
) -> Vec<ConcurrencyRecord> {
    let threads = threads_of(harness_args);
    let mut binaries: BTreeMap<String, (&str, Harness)> = BTreeMap::new();
    for target in session.targets() {
        let harness = harness_of(target, threads);
        binaries.insert(target.id.clone(), (target.package.as_str(), harness));
    }
    let metadata = session.metadata();
    let touched = &session.verified().touched.targets;
    let mut read: BTreeMap<String, PackageScan> = BTreeMap::new();
    binaries
        .into_iter()
        .map(|(binary, (package, harness))| {
            let closure = metadata
                .members()
                .find(|member| member.name == package)
                .map_or_else(Vec::new, |member| metadata.closure(&member.id));
            let missing = closure.is_empty().then(|| PackageScan {
                package: package.to_owned(),
                links: false,
                found: Vec::new(),
                unread: vec!["Cargo.toml".to_owned()],
            });
            for id in &closure {
                if !read.contains_key(id) {
                    let scan = metadata.package(id).map_or_else(
                        || PackageScan {
                            package: id.clone(),
                            links: false,
                            found: Vec::new(),
                            unread: vec!["Cargo.toml".to_owned()],
                        },
                        crate::concurrency::read::package,
                    );
                    read.insert(id.clone(), scan);
                }
            }
            let packages: Vec<&PackageScan> = closure
                .iter()
                .filter_map(|id| read.get(id))
                .chain(missing.iter())
                .collect();
            let reach = match touched.get(&binary) {
                None => Reach::NotRecorded,
                Some(recorded) if recorded.reached.loose.is_empty() => Reach::OnItsTests,
                Some(_) => Reach::OffItsTests,
            };
            let standing = standing(Evidence {
                reach,
                harness,
                packages: &packages,
            });
            let explored = match standing {
                Standing::SingleThreaded => Exploration::Unexplored {
                    why: Unexplored::NotNeeded,
                },
                Standing::Concurrent { .. } | Standing::NotProven { .. } => {
                    Exploration::Unexplored {
                        why: Unexplored::NotAsked,
                    }
                }
            };
            ConcurrencyRecord {
                standing,
                explored,
                target: binary,
            }
        })
        .collect()
}

/// What runs `target`'s tests, when libtest runs them on `threads`.
const fn harness_of(target: &rust_mutants::execute::TestTarget, threads: Threads) -> Harness {
    match (target.kind, target.harness) {
        (TargetKind::Doc, _) => Harness::Doctest,
        (
            TargetKind::Lib
            | TargetKind::Bin
            | TargetKind::Test
            | TargetKind::Example
            | TargetKind::ProcMacro,
            true,
        ) => Harness::Libtest(threads),
        (
            TargetKind::Lib
            | TargetKind::Bin
            | TargetKind::Test
            | TargetKind::Example
            | TargetKind::ProcMacro,
            false,
        ) => Harness::Other,
    }
}

/// Explores what it can of every binary not proven to run one thread, and says why it cannot of the rest.
///
/// A binary whose baseline did not pass or reached no guard is said so whatever was asked; every other one has up to `explore` guards delayed, one schedule each.
///
/// # Errors
/// The engine's refusal to run a control.
pub fn explored(
    session: &rust_mutants::session::Session,
    records: &mut [ConcurrencyRecord],
    (explore, passing): (u32, &std::collections::BTreeSet<String>),
    cancel: &rust_mutants::runner::Cancel,
) -> Result<(), crate::error::RunnerError> {
    let touched = &session.verified().touched.targets;
    for record in records {
        match record.explored {
            Exploration::Unexplored {
                why: Unexplored::NotAsked,
            } => {}
            Exploration::Unexplored {
                why: Unexplored::NotNeeded | Unexplored::NotPassing | Unexplored::NoSite,
            }
            | Exploration::Sampled { .. }
            | Exploration::Undecided { .. }
            | Exploration::Broke { .. } => continue,
        }
        if !passing.contains(&record.target) {
            record.explored = Exploration::Unexplored {
                why: Unexplored::NotPassing,
            };
            continue;
        }
        let reached = touched
            .get(&record.target)
            .map(|touches| touches.reached.union())
            .unwrap_or_default();
        if reached.is_empty() {
            record.explored = Exploration::Unexplored {
                why: Unexplored::NoSite,
            };
            continue;
        }
        if explore == 0 {
            continue;
        }
        record.explored = schedules(
            session,
            &record.target,
            (&chosen(&reached, explore), explore),
            cancel,
        )?;
    }
    Ok(())
}

/// What delaying each of `sites` of `target` found: the first site that broke it, or every site delayed and which of them settled nothing.
fn schedules(
    session: &rust_mutants::session::Session,
    target: &str,
    (sites, asked): (&[u32], u32),
    cancel: &rust_mutants::runner::Cancel,
) -> Result<Exploration, crate::error::RunnerError> {
    let mut undecided = Vec::new();
    for &site in sites {
        match ended(session, target, Started::Delayed(site), cancel)? {
            Ended::Passed => {}
            Ended::Unsettled => undecided.push(site),
            Ended::Failed(failed) => {
                if confirmed(session, (target, site), &failed, cancel)? {
                    let (path, line) = located(session, site)?;
                    return Ok(Exploration::Broke {
                        site,
                        path,
                        line,
                        failed,
                        rounds: CONFIRMING_ROUNDS,
                    });
                }
                undecided.push(site);
            }
        }
    }
    Ok(if undecided.is_empty() {
        Exploration::Sampled {
            asked,
            delayed: sites.to_vec(),
        }
    } else {
        Exploration::Undecided {
            asked,
            delayed: sites.to_vec(),
            undecided,
        }
    })
}

/// Whether every confirming round fails exactly `failed` with `site` delayed and passes without it, stopping at the first that does not.
fn confirmed(
    session: &rust_mutants::session::Session,
    (target, site): (&str, u32),
    failed: &[String],
    cancel: &rust_mutants::runner::Cancel,
) -> Result<bool, crate::error::RunnerError> {
    for _ in 0..CONFIRMING_ROUNDS {
        if !repeats(
            failed,
            &ended(session, target, Started::Delayed(site), cancel)?,
        ) {
            return Ok(false);
        }
        if !clean(&ended(session, target, Started::Confirming(site), cancel)?) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Where the guard at `site` is.
///
/// # Errors
/// [`RunInvariantError::UnplacedSite`](crate::assure::run::RunInvariantError::UnplacedSite) where the catalog holds no such site or no position for it: a site a baseline reached is always one of its catalog's, and a finding that could not name its place is not raised in its stead.
fn located(
    session: &rust_mutants::session::Session,
    site: u32,
) -> Result<(String, u32), crate::error::RunnerError> {
    let unplaced = || crate::error::RunnerError::RunInvariant {
        source: crate::assure::run::RunInvariantError::UnplacedSite { site },
    };
    let mutant = session
        .catalog()
        .mutants()
        .iter()
        .find(|mutant| mutant.index == site)
        .ok_or_else(unplaced)?;
    let line = session.position(mutant).ok_or_else(unplaced)?.line;
    Ok((mutant.candidate.path.clone(), line))
}

/// What one exploration control is started as.
#[derive(Debug, Clone, Copy)]
enum Started {
    /// Every thread paused once at the guard of this site.
    Delayed(u32),
    /// Nothing delayed: the undelayed half of a round confirming this site's delayed failure, named so a recording holds it.
    Confirming(u32),
}

/// How one control of `target` ended, started as `started`.
fn ended(
    session: &rust_mutants::session::Session,
    target: &str,
    started: Started,
    cancel: &rust_mutants::runner::Cancel,
) -> Result<Ended, crate::error::RunnerError> {
    let perturbation = match started {
        Started::Delayed(site) => Perturbation {
            delay: Some(rust_mutants::execute::Delay {
                site,
                pause_ms: PAUSE_MS,
            }),
            ..Perturbation::none()
        },
        Started::Confirming(site) => Perturbation {
            confirms: Some(site),
            ..Perturbation::none()
        },
    };
    let controlled = session.control_perturbed(
        &Request::new(String::new()).with_target(target),
        Conditions {
            observing: Observing::Nothing,
            perturbation: &perturbation,
        },
        cancel,
    )?;
    Ok(match controlled.result.outcome() {
        rust_mutants::outcome::Outcome::Survived => Ended::Passed,
        rust_mutants::outcome::Outcome::Killed => Ended::Failed(controlled.result.failed_tests),
        rust_mutants::outcome::Outcome::NotRun
        | rust_mutants::outcome::Outcome::StepLimitReached
        | rust_mutants::outcome::Outcome::Waited
        | rust_mutants::outcome::Outcome::Inconclusive
        | rust_mutants::outcome::Outcome::Errored => Ended::Unsettled,
    })
}
