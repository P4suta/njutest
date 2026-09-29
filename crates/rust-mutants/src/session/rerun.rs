// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A workspace prepared only to run a stored report's sealed executions again (ADR 0046, decision 7).

use std::path::PathBuf;

use super::{PrepareOptions, Session};
use crate::EngineError;
use crate::runner::Cancel;
use crate::sealed::rerun::{Now, Recorded, Reproduction, Reran, Rerun, Unmade, named, unstationed};
use crate::workspace::Workspace;

/// A workspace prepared only to run recorded sealed executions again: built for the sealed target as a run builds it, with no native baseline and no coverage, since the executions already name their tests, and holding a session nothing routes, benches or runs natively on.
#[derive(Debug)]
pub struct Rerunnable {
    session: Session,
}

impl Rerunnable {
    /// `workspace` prepared as `options` asks, the sealed build included whatever they say, with no test started natively and no coverage measured.
    ///
    /// # Errors
    /// Every failure of the phases it runs.
    pub fn prepared(
        workspace: Workspace,
        options: &PrepareOptions,
        cancel: &Cancel,
    ) -> Result<Self, EngineError> {
        let session = super::prepare(
            workspace,
            &PrepareOptions {
                verify: false,
                coverage: false,
                measurements: None,
                sealing: crate::sealed::Sealing::On,
                ..options.clone()
            },
            cancel,
        )?;
        Ok(Self { session })
    }

    /// Runs each of `recorded` again, in order and each test's control first, and says what each comes to now, stopping at the first that comes to something other than it was recorded as; every execution stops when `cancel` is raised.
    ///
    /// # Errors
    /// A tree or module that cannot be read, an environment that is not text, a host that cannot start or run an execution, or [`EngineError::Interrupted`].
    pub fn rerun(
        &self,
        recorded: &[Recorded],
        cancel: &Cancel,
    ) -> Result<Reproduction, EngineError> {
        let session = &self.session;
        let phase = session.trace().phase("rerun");
        let runner = crate::run::sealed_runner(session)?;
        let stations = match &runner {
            Some(runner) => Some(Rerun::assemble(
                (runner, rust_mutants_sealed::Interrupt::of(cancel.flags())),
                (&session.sealed, &named(recorded)),
                (session.sealed_tree()?, &session.harness_args),
                (session.catalog.digest(), session.touch_bounds()?),
            )?),
            None => None,
        };
        let mut agreed = Vec::new();
        for one in recorded {
            let again = Reran {
                recorded: one.clone(),
                now: self.now(one, stations.as_ref())?,
            };
            if !again.same() {
                phase.end();
                return Ok(Reproduction::Differed {
                    agreed,
                    first: again,
                });
            }
            agreed.push(again);
        }
        phase.end();
        Ok(Reproduction::Reproduced(agreed))
    }

    /// What `recorded` comes to now on `rerun`, or why it cannot be made again.
    fn now(&self, recorded: &Recorded, rerun: Option<&Rerun<'_>>) -> Result<Now, EngineError> {
        let session = &self.session;
        let Some(mutant) = session
            .catalog
            .mutants()
            .iter()
            .find(|mutant| mutant.id.as_str() == recorded.mutant)
        else {
            return Ok(Now::Unmade(Unmade::Uncataloged));
        };
        if !session.accepted().contains(&mutant.index) {
            return Ok(Now::Unmade(Unmade::Rejected));
        }
        if !session
            .sealed
            .holds(&session.snapshot_root().join(&mutant.candidate.path))
        {
            return Ok(Now::Unmade(Unmade::GuardAbsent));
        }
        let Some(rerun) = rerun else {
            return Ok(Now::Unmade(unstationed(
                &session.sealed.unsealed,
                &recorded.target,
            )));
        };
        let now = rerun.put(&recorded.target, &recorded.test, mutant)?;
        if let Now::Came(came_to) = now {
            session.trace().sealed_exec(crate::trace::SealedExecRecord {
                mutant: mutant.display_id.to_string(),
                index: mutant.index,
                target: recorded.target.clone(),
                test: recorded.test.clone(),
                came_to: came_to.name().to_owned(),
            });
        }
        Ok(now)
    }

    /// Removes the snapshot, or preserves it where the workspace was opened to keep it, and says what was kept.
    ///
    /// # Errors
    /// A snapshot directory that could not be removed.
    pub fn close(self) -> Result<Vec<PathBuf>, EngineError> {
        self.session.close()
    }
}
