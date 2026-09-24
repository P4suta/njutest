// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether each test binary a baseline measured is proven to run one thread: its touch record, held beside what every package it links can start.

use std::collections::BTreeMap;

use super::schedule::{self, ScheduleError};
use crate::concurrency::explore::{Delayed, Ended, chosen, delayed};
use crate::concurrency::proof::{Evidence, PackageScan, Reach, Standing, standing};
use crate::report::concurrency::{ConcurrencyRecord, Exploration, Unexplored};
use rust_mutants::session::{Conditions, Observing, Perturbation, Request};

/// How long a delayed guard holds each thread that reaches it, once.
pub const PAUSE_MS: u64 = 100;

/// One record per test binary the session measured, in binary order, each package of every closure read once, at most `workers` at a time.
///
/// # Errors
/// A reading worker panicked.
pub fn recorded(
    session: &rust_mutants::session::Session,
    workers: usize,
) -> Result<Vec<ConcurrencyRecord>, ScheduleError> {
    let mut binaries: BTreeMap<String, (&str, bool)> = BTreeMap::new();
    for target in session.targets() {
        binaries.insert(target.id.clone(), (target.package.as_str(), target.harness));
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
    let read = scans(metadata, &every, workers)?;
    Ok(binaries
        .into_iter()
        .map(|(binary, (package, libtest))| {
            let closure = closures.get(&binary).map_or(&[][..], Vec::as_slice);
            let missing = closure.is_empty().then(|| unread_manifest(package));
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
                libtest,
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

/// Every package named in `ids` read once, at most `workers` at a time, by id; one the metadata does not hold is read as a package whose manifest was not read.
///
/// # Errors
/// A reading worker panicked.
pub fn scans(
    metadata: &rust_mutants::cargo::Metadata,
    ids: &[&str],
    workers: usize,
) -> Result<BTreeMap<String, PackageScan>, ScheduleError> {
    let read = schedule::measure(ids, workers, |_at, id| {
        metadata
            .package(id)
            .map_or_else(|| unread_manifest(id), crate::concurrency::read::package)
    })?;
    Ok(ids.iter().map(|id| (*id).to_owned()).zip(read).collect())
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

/// Delays up to `explore` guards of every binary not proven to run one thread whose baseline passed, one schedule each, and records what that found.
///
/// # Errors
/// The engine's refusal to run a control.
pub fn explored(
    session: &rust_mutants::session::Session,
    records: &mut [ConcurrencyRecord],
    (explore, passing): (u32, &std::collections::BTreeSet<String>),
    cancel: &rust_mutants::runner::Cancel,
) -> Result<(), rust_mutants::EngineError> {
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
        record.explored = schedules(session, &record.target, &chosen(&reached, explore), cancel)?;
    }
    Ok(())
}

/// What delaying each of `sites` of `target` found: the first site that broke it, or every site delayed.
fn schedules(
    session: &rust_mutants::session::Session,
    target: &str,
    sites: &[u32],
    cancel: &rust_mutants::runner::Cancel,
) -> Result<Exploration, rust_mutants::EngineError> {
    let mut undecided = Vec::new();
    for &site in sites {
        let first = ended(session, target, Some(site), cancel)?;
        let (repeats, undelayed) = match first {
            Ended::Failed(_) => (
                vec![
                    ended(session, target, Some(site), cancel)?,
                    ended(session, target, Some(site), cancel)?,
                ],
                Some(ended(session, target, None, cancel)?),
            ),
            Ended::Passed | Ended::Unsettled => (Vec::new(), None),
        };
        match delayed(&first, &repeats, undelayed.as_ref()) {
            Delayed::Passed => {}
            Delayed::Undecided => undecided.push(site),
            Delayed::Broke { failed } => {
                let (path, line) = session
                    .catalog()
                    .mutants()
                    .iter()
                    .find(|mutant| mutant.index == site)
                    .map_or_else(
                        || (String::new(), 0),
                        |mutant| {
                            (
                                mutant.candidate.path.clone(),
                                session.position(mutant).map_or(0, |position| position.line),
                            )
                        },
                    );
                return Ok(Exploration::Broke {
                    site,
                    path,
                    line,
                    failed,
                });
            }
        }
    }
    Ok(Exploration::Sampled {
        delayed: sites.to_vec(),
        undecided,
    })
}

/// How one control of `target` ended, with the guard at `site` delayed or with nothing delayed.
fn ended(
    session: &rust_mutants::session::Session,
    target: &str,
    site: Option<u32>,
    cancel: &rust_mutants::runner::Cancel,
) -> Result<Ended, rust_mutants::EngineError> {
    let perturbation = Perturbation {
        delay: site.map(|site| rust_mutants::execute::Delay {
            site,
            pause_ms: PAUSE_MS,
        }),
        ..Perturbation::none()
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
