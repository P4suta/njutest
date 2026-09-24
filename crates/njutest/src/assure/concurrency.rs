// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether each test binary a baseline measured is proven to run one thread: its touch record, held beside what every package it links can start.

use std::collections::BTreeMap;

use super::schedule;
use crate::concurrency::explore::{CONFIRMING_ROUNDS, Ended, chosen, clean, repeats};
use crate::concurrency::proof::{
    Evidence, Harness, PackageScan, Reach, Standing, Threads, standing, threads_of,
};
use crate::report::concurrency::{ConcurrencyRecord, Exploration, Unexplored};
use rust_mutants::execute::TargetKind;
use rust_mutants::session::{Conditions, Observing, Perturbation, Request};

/// How long a delayed guard holds each thread that reaches it, once.
pub const PAUSE_MS: u64 = 100;

/// One record per test binary the session measured, in binary order, each package of every closure read once, at most `workers` at a time; `harness_args` are what every libtest binary was run with.
///
/// # Errors
/// A reading worker panicked, or the process ran out of descriptors or memory while reading.
pub fn recorded(
    session: &rust_mutants::session::Session,
    (harness_args, workers): (&[String], usize),
) -> Result<Vec<ConcurrencyRecord>, crate::error::RunnerError> {
    let threads = threads_of(harness_args);
    let mut binaries: BTreeMap<String, (&str, Harness)> = BTreeMap::new();
    for target in session.targets() {
        let harness = harness_of(target, threads);
        binaries.insert(target.id.clone(), (target.package.as_str(), harness));
    }
    let metadata = session.metadata();
    let touched = &session.verified().touched.targets;
    let closures: BTreeMap<String, Vec<String>> = binaries
        .iter()
        .map(|(binary, (package, _))| {
            let closure = metadata
                .members()
                .find(|member| member.name == *package)
                .map_or_else(Vec::new, |member| metadata.closure(&member.id));
            (binary.clone(), closure)
        })
        .collect();
    let every: std::collections::BTreeSet<&str> = closures
        .values()
        .flat_map(|closure| closure.iter().map(String::as_str))
        .collect();
    let every: Vec<&str> = every.into_iter().collect();
    let compiled = crate::concurrency::read::Compiled::read(session.target_dir())?;
    let read = scans(metadata, (&every, &compiled), workers)?;
    Ok(binaries
        .into_iter()
        .map(|(binary, (package, harness))| {
            let closure = closures.get(&binary).map_or(&[][..], Vec::as_slice);
            let unread: Vec<PackageScan> = closure
                .iter()
                .filter(|id| !read.contains_key(*id))
                .map(|id| unread_manifest(id))
                .chain(closure.is_empty().then(|| unread_manifest(package)))
                .collect();
            let packages: Vec<&PackageScan> = closure
                .iter()
                .filter_map(|id| read.get(id))
                .chain(unread.iter())
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
        .collect())
}

/// Every package named in `ids` read once, at most `workers` at a time and never more than there are packages, by id, each held to the files `compiled` says its crates were built from.
///
/// One the metadata does not hold, or no answer came back for, is read as a package whose manifest was not read.
///
/// # Errors
/// A reading worker panicked, or the process ran out of descriptors or memory while reading.
pub fn scans(
    metadata: &rust_mutants::cargo::Metadata,
    (ids, compiled): (&[&str], &crate::concurrency::read::Compiled),
    workers: usize,
) -> Result<BTreeMap<String, PackageScan>, crate::error::RunnerError> {
    let read = schedule::measure(ids, workers.min(ids.len()), |_at, id| {
        metadata.package(id).map_or_else(
            || Ok(unread_manifest(id)),
            |package| crate::concurrency::read::package(package, compiled),
        )
    })?;
    let mut answers = read.into_iter();
    let mut scanned = BTreeMap::new();
    for id in ids {
        let scan = match answers.next() {
            Some(answer) => answer?,
            None => unread_manifest(id),
        };
        scanned.insert((*id).to_owned(), scan);
    }
    Ok(scanned)
}

/// A package named `package` whose manifest was not read, which proves nothing about it.
fn unread_manifest(package: &str) -> PackageScan {
    PackageScan {
        package: package.to_owned(),
        links: false,
        found: Vec::new(),
        unread: vec!["Cargo.toml".to_owned()],
    }
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

/// Delays up to `explore` guards of every binary not proven to run one thread whose baseline passed, one schedule each, and records what that found.
///
/// # Errors
/// The engine's refusal to run a control.
pub fn explored(
    session: &rust_mutants::session::Session,
    records: &mut [ConcurrencyRecord],
    (explore, passing): (u32, &std::collections::BTreeSet<String>),
    cancel: &rust_mutants::runner::Cancel,
) -> Result<(), crate::error::RunnerError> {
    if explore == 0 {
        return Ok(());
    }
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
