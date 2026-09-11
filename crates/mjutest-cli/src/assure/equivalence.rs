// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Which mutations nothing noticed are mutations nothing could have noticed.
//!
//! The engine answers one question and one only: does the compiler render this
//! mutation identically to the program it mutates
//! ([ADR 0013](../../../../docs/adr/0013-codegen-identity-is-the-equivalence-proof.md)).
//! Identical artifacts are the same program, and the same program makes the
//! same observations, so no test can tell the two apart.
//!
//! Turning that into `equivalent` needs premises the engine has no way to
//! check, and one of them carries the whole layer. A mutation of a function no
//! test calls is dropped by the linker, the artifacts come out identical, and
//! the reason is the opposite of reassuring: the code is untested. So a
//! mutation is `equivalent` only where the tests ran the position — where the
//! route was decided by region and named at least one target — and a mutation
//! nothing reached keeps its finding whatever the compiler did with it.

use std::collections::{BTreeMap, BTreeSet};

use crate::assure::route::Route;

/// Why a mutation the compiler renders identically is still not one this run calls equivalent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refused {
    /// No test executed the position, so identical artifacts say the code is untested rather than that the mutation is unobservable.
    NothingReached,
    /// The route was widened past the position, so what the tests executed is not known at the granularity this rests on.
    RouteWidened,
    /// The package holds `unsafe`, where "the same instructions" and "the same behaviour" are not the same sentence.
    PackageHoldsUnsafe,
    /// A test wrote into the tree while it was being measured, so the tree the layer built is not the tree the tests ran against.
    TreeWritten,
    /// A control stopped matching, which withdraws every answer this layer would give.
    ControlWithdrawn,
}

impl Refused {
    /// The wire name a trace uses.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::NothingReached => "nothing-reached",
            Self::RouteWidened => "route-widened",
            Self::PackageHoldsUnsafe => "package-holds-unsafe",
            Self::TreeWritten => "tree-written",
            Self::ControlWithdrawn => "control-withdrawn",
        }
    }
}

/// What this run holds about a mutation nothing noticed, beside the route it was measured with.
#[derive(Debug, Clone, Copy)]
pub struct Standing<'a> {
    /// How the mutation's tests were chosen.
    pub route: &'a Route,
    /// The package the mutation is in.
    pub package: &'a str,
    /// Every package this run's soundness inventory found `unsafe` in.
    pub unsafe_packages: &'a BTreeSet<String>,
    /// Whether a test wrote into the tree while it was being measured.
    pub tree_written: bool,
    /// Whether a control has already withdrawn this layer's answers.
    pub withdrawn: bool,
}

/// Whether this run may ask the compiler about the mutation at all, and why not when it may not.
///
/// Every premise is checked before the build rather than after it, because a
/// build is the expensive part and a premise that fails makes the answer
/// worthless either way.
///
/// # Errors
/// Returns the premise that failed, which is what a route record says.
pub fn askable(standing: Standing<'_>) -> Result<(), Refused> {
    if standing.withdrawn {
        return Err(Refused::ControlWithdrawn);
    }
    if standing.tree_written {
        return Err(Refused::TreeWritten);
    }
    if standing.unsafe_packages.contains(standing.package) {
        return Err(Refused::PackageHoldsUnsafe);
    }
    if crate::assure::route::ran_the_position(standing.route) {
        return Ok(());
    }
    if crate::assure::route::nothing_ran(standing.route) {
        return Err(Refused::NothingReached);
    }
    Err(Refused::RouteWidened)
}

/// What one pass of this layer needs: where the tree is, what to build it with, and what the run holds about the mutations it is asked about.
#[derive(Debug, Clone)]
pub struct Proving<'a> {
    /// The workspace root, which this layer copies a tree of its own from.
    pub root: &'a std::path::Path,
    /// How that copy is opened and which cargo builds it.
    pub open: rust_mutants::workspace::OpenOptions,
    /// What the copy is compiled as, which must be what the run measured: two programs built with different features are not the pair this layer is about.
    pub build: rust_mutants::cargo::BuildConfig,
    /// How long one build may take.
    pub timeout: Option<std::time::Duration>,
    /// Every package this run's soundness inventory found `unsafe` in.
    pub unsafe_packages: BTreeSet<String>,
    /// Whether a test wrote into the tree while it was being measured.
    pub tree_written: bool,
}

/// One mutation this layer is asked about.
#[derive(Debug, Clone)]
pub struct Asked {
    /// The mutation, as the engine splices it.
    pub candidate: rust_mutants::catalog::Candidate,
    /// The mutant a person types.
    pub display_id: String,
    /// The package the mutation is in.
    pub package: String,
    /// How its tests were chosen.
    pub route: Route,
}

/// What this layer decided about one mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decided {
    /// The mutant a person types.
    pub display_id: String,
    /// Whether it is equivalent.
    pub equivalent: bool,
    /// What the compiler said, or why it was never asked.
    pub detail: String,
}

/// Asks the compiler about every mutation whose premises hold, and says what it answered about each.
///
/// # Errors
/// Whatever stopped the tree being copied or built. A mutation whose premises
/// do not hold costs no build and is decided here.
pub fn prove(
    proving: &Proving<'_>,
    asked: &[Asked],
    cancel: &rust_mutants::runner::Cancel,
    trace: &rust_mutants::trace::Recorder,
) -> Result<Vec<Decided>, crate::error::RunnerError> {
    let mut decided = Vec::with_capacity(asked.len());
    if asked.iter().all(|one| {
        askable(Standing {
            route: &one.route,
            package: &one.package,
            unsafe_packages: &proving.unsafe_packages,
            tree_written: proving.tree_written,
            withdrawn: false,
        })
        .is_err()
    }) {
        for one in asked {
            decided.push(refused(one, &proving.unsafe_packages, proving.tree_written));
        }
        return Ok(decided);
    }
    let mut prover = rust_mutants::equivalence::Prover::open(
        proving.root,
        &rust_mutants::equivalence::ProveOptions {
            build: proving.build.clone(),
            open: proving.open.clone(),
            timeout: proving.timeout,
        },
        cancel,
        trace,
    )?;
    for one in asked {
        let standing = Standing {
            route: &one.route,
            package: &one.package,
            unsafe_packages: &proving.unsafe_packages,
            tree_written: proving.tree_written,
            withdrawn: prover.withdrawn(),
        };
        if let Err(why) = askable(standing) {
            decided.push(Decided {
                display_id: one.display_id.clone(),
                equivalent: false,
                detail: why.name().to_owned(),
            });
            continue;
        }
        let answer = prover.identical(&one.candidate, cancel)?;
        decided.push(Decided {
            display_id: one.display_id.clone(),
            equivalent: answer == rust_mutants::equivalence::Identity::Identical,
            detail: answer.name().to_owned(),
        });
    }
    prover.close()?;
    Ok(decided)
}

/// What a mutation whose premises do not hold is decided as, without a build.
fn refused(one: &Asked, unsafe_packages: &BTreeSet<String>, tree_written: bool) -> Decided {
    let why = askable(Standing {
        route: &one.route,
        package: &one.package,
        unsafe_packages,
        tree_written,
        withdrawn: false,
    })
    .err();
    Decided {
        display_id: one.display_id.clone(),
        equivalent: false,
        detail: why.map_or_else(|| "not-asked".to_owned(), |why| why.name().to_owned()),
    }
}

/// Every mutation nothing noticed, as this layer would ask about it.
///
/// Only a survival is asked about. A mutation a test noticed has been noticed,
/// and asking the compiler whether it could have been is asking a question
/// this run already answered.
#[must_use]
pub fn asked(
    session: &rust_mutants::session::Session,
    judged: &[crate::assure::mutation::Judged],
) -> Vec<Asked> {
    let catalog = session.catalog();
    let by_id: BTreeMap<&str, &rust_mutants::catalog::Mutant> = catalog
        .mutants()
        .iter()
        .map(|mutant| (mutant.id.as_str(), mutant))
        .collect();
    judged
        .iter()
        .filter_map(|one| {
            let crate::assure::mutation::Disposition::Survived { route } = &one.disposition else {
                return None;
            };
            let mutant = by_id.get(one.id.as_str())?;
            Some(Asked {
                candidate: mutant.candidate.clone(),
                display_id: one.display_id.clone(),
                package: session.package_of(mutant.index)?.to_owned(),
                route: route.clone(),
            })
        })
        .collect()
}

/// Turns every survival the compiler renders identically into an equivalence, and records what the layer said about each.
pub fn settle(
    judged: &mut [crate::assure::mutation::Judged],
    decided: &[Decided],
    watch: crate::watch::Watch<'_>,
) {
    let mut answers: BTreeMap<&str, &Decided> = BTreeMap::new();
    for answer in decided {
        let _first = answers.entry(answer.display_id.as_str()).or_insert(answer);
    }
    for one in judged.iter_mut() {
        let Some(answer) = answers.get(one.display_id.as_str()) else {
            continue;
        };
        watch.trace.note(
            "equivalence",
            &format!("{} {}", one.display_id, answer.detail),
        );
        if !answer.equivalent {
            continue;
        }
        if let crate::assure::mutation::Disposition::Survived { route } = &one.disposition {
            one.disposition = crate::assure::mutation::Disposition::Equivalent {
                route: route.clone(),
            };
        }
    }
}
