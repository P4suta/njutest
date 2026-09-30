// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A mutant's standing from the tests that reach it natively and the sealed executions of the tests whose controls reached it (ADR 0046).

use std::path::Path;

use rust_mutants_decision::evidence::{Execution, Sealability, Sealed, Standing, standing};

use super::SealedBuild;
use super::bench::{Bench, BenchError};
use crate::catalog::Mutant;
use crate::session::Route;

/// One sealed execution a standing rests on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Put {
    /// The target whose module ran.
    pub target: String,
    /// The test it ran.
    pub test: String,
    /// What the execution came to.
    pub came_to: Sealed,
}

/// A mutant's standing, and the sealed executions it rests on, in the order they ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    /// The standing.
    pub standing: Standing,
    /// Every sealed execution, in the order they ran; the first detection ends them.
    pub puts: Vec<Put>,
}

/// What `bench` establishes about `mutant`, whose source is `file` in the instrumented tree and which `route` reaches natively.
///
/// # Errors
/// A host that cannot run an execution, or an environment that is not text.
pub fn answer(
    (bench, sealed): (&Bench<'_>, &SealedBuild),
    mutant: &Mutant,
    file: &Path,
    route: &Route,
) -> Result<Answer, BenchError> {
    if !sealed.holds(file) {
        return Ok(Answer {
            standing: standing(Sealability::GuardAbsent, &[]),
            puts: Vec::new(),
        });
    }
    let (sealability, natives) = parity(bench, mutant, route);
    let mut executions = vec![Execution::Native; natives];
    let mut puts = Vec::new();
    'stations: for (target, station) in &bench.stations {
        for (test, control) in &station.controls {
            let Ok(control) = control else { continue };
            if !bench.compiles(mutant.index) && !control.reached.contains(&mutant.index) {
                continue;
            }
            let Some(came_to) = bench.put(target, test, mutant.id.as_str())? else {
                continue;
            };
            executions.push(Execution::Sealed(came_to));
            puts.push(Put {
                target: target.clone(),
                test: test.clone(),
                came_to,
            });
            if matches!(came_to, Sealed::Detected(_)) {
                break 'stations;
            }
        }
    }
    Ok(Answer {
        standing: standing(sealability, &executions),
        puts,
    })
}

/// Whether the sealed build answers for every test `route` reaches `mutant` with natively, and how many of them only a native run holds.
fn parity(bench: &Bench<'_>, mutant: &Mutant, route: &Route) -> (Sealability, usize) {
    let mut absent = 0_usize;
    let mut differs = false;
    for target in route.reaching() {
        let Some(station) = bench.stations.get(target) else {
            absent = absent.saturating_add(1);
            continue;
        };
        let named = route.tests_of(target);
        let asked: Vec<&String> = if named.is_empty() {
            station.controls.keys().collect()
        } else {
            named.iter().collect()
        };
        for test in asked {
            match station.controls.get(test.as_str()) {
                Some(Ok(control))
                    if bench.compiles(mutant.index) || control.reached.contains(&mutant.index) => {}
                Some(Ok(_)) => differs = differs || !named.is_empty(),
                Some(Err(_)) | None => absent = absent.saturating_add(1),
            }
        }
    }
    let sealability = match (absent, differs) {
        (0, false) => Sealability::Answerable,
        (0, true) => Sealability::ReachDiffers,
        (_, _) => Sealability::TestAbsent,
    };
    (sealability, absent)
}
