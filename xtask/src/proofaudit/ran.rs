// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every execution a row can rest on, native or sealed, in the one shape a layer is handed one in (ADR 0046).

use super::{Engine, MutantRow, SealedRun};

/// One execution of one mutation: a native one, which is a lead, or a sealed one, which a verdict rests on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Ran<'a> {
    /// A native execution the runner recorded.
    Native(&'a crate::route::Exec),
    /// A sealed execution a build's engine recorded.
    Sealed(&'a SealedRun),
}

impl Ran<'_> {
    /// The target whose tests ran.
    pub(super) fn target(&self) -> &str {
        match self {
            Self::Native(exec) => &exec.target,
            Self::Sealed(run) => &run.target,
        }
    }
}

/// Whether the run kept one kind of execution where this audit can read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kept {
    /// It did, and every one of them is among the executions.
    Held,
    /// The run kept no recording that holds that kind.
    Unrecorded,
    /// A recording holds one of that kind this audit cannot read.
    Unreadable,
}

/// Every execution the run recorded, native from the runner's recording and sealed from each build's engine recording, with the mutation each ran, in the order recorded.
#[derive(Debug)]
pub(super) struct Executions<'a> {
    ran: Vec<(&'a str, Ran<'a>)>,
    /// Whether every build's sealed executions are among them; the runner's native ones are where the run kept a recording.
    pub(super) sealed: Kept,
}

impl<'a> Executions<'a> {
    /// Every execution `routing` and `engines` hold, where the runner's own account of a sealed execution gives way to the engine's record of it, which is the witness.
    pub(super) fn read(routing: Option<&'a crate::route::Routing>, engines: &'a [Engine]) -> Self {
        let mut ran = Vec::new();
        for execution in routing.iter().flat_map(|routing| routing.executions()) {
            match execution {
                crate::route::Execution::Native(exec) => {
                    ran.push((exec.mutant.as_str(), Ran::Native(exec)));
                }
                crate::route::Execution::Sealed(_) => {}
            }
        }
        let sealed = if engines.is_empty() {
            Kept::Unrecorded
        } else if engines.iter().any(|engine| engine.sealed.is_none()) {
            Kept::Unreadable
        } else {
            for recorded in engines.iter().filter_map(|engine| engine.sealed.as_ref()) {
                for (mutant, run) in recorded {
                    ran.push((mutant.as_str(), Ran::Sealed(run)));
                }
            }
            Kept::Held
        };
        Self { ran, sealed }
    }

    /// Every execution, with the mutation it ran as the recording names it.
    pub(super) fn each(&self) -> impl Iterator<Item = (&'a str, Ran<'a>)> + '_ {
        self.ran.iter().copied()
    }

    /// Every execution of the mutation either `id` or `display_id` names, in the order recorded.
    pub(super) fn named(&self, id: &str, display_id: &str) -> Vec<Ran<'a>> {
        self.ran
            .iter()
            .filter(|(mutant, _)| {
                *mutant == id || (!display_id.is_empty() && *mutant == display_id)
            })
            .map(|(_, ran)| *ran)
            .collect()
    }

    /// Every execution of `mutant`, under either of its names, in the order recorded.
    pub(super) fn of(&self, mutant: &MutantRow) -> Vec<Ran<'a>> {
        self.named(&mutant.id, &mutant.display_id)
    }
}
