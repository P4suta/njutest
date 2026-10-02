// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

extern crate std;

use std::vec;
use std::vec::Vec;

use super::{Detection, Doubt, Execution, Found, Reason, Sealability, Sealed, Standing, standing};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Read {
    Killed { by: usize, how: Detection },
    Survived,
    Unreached,
    Unproven(Vec<Reason>),
}

fn every_execution() -> Vec<Execution> {
    let mut every = vec![
        Execution::Native,
        Execution::Sealed(Sealed::Passed),
        Execution::Sealed(Sealed::SetAside),
    ];
    every.extend(
        Detection::ALL
            .iter()
            .map(|how| Execution::Sealed(Sealed::Detected(*how))),
    );
    every.extend(
        Doubt::ALL
            .iter()
            .map(|doubt| Execution::Sealed(Sealed::Doubted(*doubt))),
    );
    for execution in &every {
        match execution {
            Execution::Native
            | Execution::Sealed(
                Sealed::Passed | Sealed::Detected(_) | Sealed::Doubted(_) | Sealed::SetAside,
            ) => {}
        }
    }
    every
}

fn every_sequence_up_to(length: usize) -> Vec<Vec<Execution>> {
    let every = every_execution();
    let mut sequences: Vec<Vec<Execution>> = vec![Vec::new()];
    let mut frontier: Vec<Vec<Execution>> = vec![Vec::new()];
    for _ in 0..length {
        let mut longer = Vec::new();
        for sequence in &frontier {
            for execution in &every {
                let mut next = sequence.clone();
                next.push(*execution);
                longer.push(next);
            }
        }
        sequences.extend(longer.iter().cloned());
        frontier = longer;
    }
    sequences
}

const fn doubt_reason(doubt: Doubt) -> Reason {
    match doubt {
        Doubt::ExitedEarly => Reason::ExitedEarly,
        Doubt::StackOverflow => Reason::StackOverflow,
        Doubt::Refused => Reason::Refused,
        Doubt::Unaccounted => Reason::Unaccounted,
        Doubt::Unmatched => Reason::Unmatched,
    }
}

fn read_by_the_rules(sealability: Sealability, executions: &[Execution]) -> Read {
    if sealability == Sealability::GuardAbsent {
        return Read::Unproven(vec![Reason::GuardAbsent]);
    }
    for (by, execution) in executions.iter().enumerate() {
        if let Execution::Sealed(Sealed::Detected(how)) = execution {
            return Read::Killed { by, how: *how };
        }
    }
    let mut reasons = Vec::new();
    if sealability == Sealability::TestAbsent {
        reasons.push(Reason::TestAbsent);
    }
    if sealability == Sealability::ReachDiffers {
        reasons.push(Reason::ReachDiffers);
    }
    for execution in executions {
        let reason = match execution {
            Execution::Native => Some(Reason::Native),
            Execution::Sealed(Sealed::Doubted(doubt)) => Some(doubt_reason(*doubt)),
            Execution::Sealed(Sealed::Passed | Sealed::Detected(_) | Sealed::SetAside) => None,
        };
        if let Some(reason) = reason
            && !reasons.contains(&reason)
        {
            reasons.push(reason);
        }
    }
    if !reasons.is_empty() {
        reasons.sort_by_key(|reason| Reason::ALL.iter().position(|each| each == reason));
        return Read::Unproven(reasons);
    }
    if executions.is_empty() {
        Read::Unreached
    } else if executions
        .iter()
        .all(|execution| *execution == Execution::Sealed(Sealed::SetAside))
    {
        Read::Unproven(vec![Reason::Declined])
    } else {
        Read::Survived
    }
}

fn read(standing: Standing) -> Read {
    match standing {
        Standing::Established(verdict) => match verdict.found() {
            Found::Killed { by, how } => Read::Killed { by, how },
            Found::Survived => Read::Survived,
            Found::Unreached => Read::Unreached,
        },
        Standing::Unproven(doubts) => Read::Unproven(
            Reason::ALL
                .iter()
                .copied()
                .filter(|reason| doubts.contains(*reason))
                .collect(),
        ),
    }
}

fn disagreements(
    decide: impl Fn(Sealability, &[Execution]) -> Standing,
) -> Vec<(Sealability, Vec<Execution>, Read, Read)> {
    let mut found = Vec::new();
    for sealability in Sealability::ALL {
        for executions in every_sequence_up_to(3) {
            let said = read(decide(sealability, &executions));
            let ruled = read_by_the_rules(sealability, &executions);
            if said != ruled {
                found.push((sealability, executions, said, ruled));
            }
        }
    }
    found
}

#[test]
fn every_standing_up_to_three_executions_is_the_one_the_rules_give() {
    let found = disagreements(standing);
    assert!(
        found.is_empty(),
        "{} of the cases disagree with the rules, the first {:?}",
        found.len(),
        found.first()
    );
}

#[test]
fn an_unproven_standing_always_names_a_reason() {
    for sealability in Sealability::ALL {
        for executions in every_sequence_up_to(3) {
            if let Standing::Unproven(doubts) = standing(sealability, &executions) {
                assert!(
                    Reason::ALL.iter().any(|reason| doubts.contains(*reason)),
                    "{sealability:?} {executions:?} is unproven for no reason"
                );
            }
        }
    }
}

#[test]
fn a_standing_that_reads_a_native_pass_as_sealed_is_caught() {
    let planted = |sealability: Sealability, executions: &[Execution]| {
        let believed: Vec<Execution> = executions
            .iter()
            .map(|execution| match execution {
                Execution::Native => Execution::Sealed(Sealed::Passed),
                Execution::Sealed(sealed) => Execution::Sealed(*sealed),
            })
            .collect();
        standing(sealability, &believed)
    };
    let found = disagreements(planted);
    assert!(
        found.iter().any(|(sealability, executions, said, _)| {
            *sealability == Sealability::Answerable
                && executions.as_slice() == [Execution::Native]
                && *said == Read::Survived
        }),
        "the rules did not catch a native pass read as sealed: {} disagreements",
        found.len()
    );
}

#[test]
fn a_standing_that_ignores_a_doubt_is_caught() {
    let planted = |sealability: Sealability, executions: &[Execution]| {
        let believed: Vec<Execution> = executions
            .iter()
            .map(|execution| match execution {
                Execution::Sealed(Sealed::Doubted(_)) => Execution::Sealed(Sealed::Passed),
                Execution::Native => Execution::Native,
                Execution::Sealed(sealed) => Execution::Sealed(*sealed),
            })
            .collect();
        standing(sealability, &believed)
    };
    assert!(
        !disagreements(planted).is_empty(),
        "the rules did not catch a doubt read as a pass"
    );
}
