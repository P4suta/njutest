// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the carry rule reads of a prepared session: its tree, the route a mutant would take, and whether a target's reach holds (ADR 0041).

use std::collections::{BTreeMap, BTreeSet};

use super::{Asking, Chosen, Observing, Request, Session};
use crate::EngineError;
use crate::carry::{Body, Entered, Execution, Locus, Now, Planned, Sealing};
use crate::catalog::Mutant;
use crate::execute::MutantResult;
use crate::runner::Cancel;
use crate::touch::{ItemRef, Steadiness};
use crate::workspace::SessionError;

/// A session's tree as the carry rule reads it.
#[derive(Debug)]
pub struct Tree {
    /// The skeleton every target of the tree is held to.
    pub skeleton: String,
    /// Every item's body, by the name a record keeps.
    pub items: BTreeMap<ItemRef, Body>,
    /// Every item's name and body digest, by its index in this catalog.
    by_index: BTreeMap<u32, (ItemRef, String)>,
}

impl Session {
    /// The tree as the carry rule reads it, taken once from the pristine build's skeletons.
    pub fn carried_tree(&self) -> &Tree {
        self.closure.carrying.tree.get_or_init(|| {
            let skeletons = self.skeletons();
            let mut items = BTreeMap::new();
            let mut by_index = BTreeMap::new();
            for one in skeletons.items {
                let sealing = if one.sealed {
                    Sealing::Sealed
                } else {
                    Sealing::Unsealed
                };
                items.insert(
                    one.item.clone(),
                    Body {
                        digest: one.body_digest.clone(),
                        sealing,
                    },
                );
                by_index.insert(one.index, (one.item, one.body_digest));
            }
            Tree {
                skeleton: crate::carry::tree_skeleton(&skeletons.units),
                items,
                by_index,
            }
        })
    }

    /// Where `mutant` sits: the innermost item body its edit is inside, or nothing where it is inside none, which leaves it nothing to carry.
    #[must_use]
    pub fn locus(&self, mutant: &Mutant) -> Option<Locus> {
        let span = mutant.candidate.span;
        let item = self
            .verified
            .touched
            .items
            .iter()
            .filter(|item| {
                item.path == mutant.candidate.path
                    && item.body.start <= span.start
                    && span.end <= item.body.end
            })
            .max_by_key(|item| item.body.start)?;
        let (named, digest) = self.carried_tree().by_index.get(&item.index)?;
        let replacement = match String::from_utf8(mutant.candidate.replacement.clone()) {
            Ok(text) => text,
            Err(_not_text) => return None,
        };
        Some(Locus {
            item: named.clone(),
            body_digest: digest.clone(),
            start: span.start.checked_sub(item.body.start)?,
            end: span.end.checked_sub(item.body.start)?,
            replacement,
            rule: format!(
                "{}@{}",
                mutant.candidate.rule.name, mutant.candidate.rule.version
            ),
        })
    }

    /// Every execution this run's route makes of `mutant` with `args`, in the order it makes them, with the tests each names.
    ///
    /// # Errors
    /// The engine's refusals while establishing which tests a target can be narrowed to.
    pub fn plan(
        &self,
        mutant: &Mutant,
        args: &[String],
        cancel: &Cancel,
    ) -> Result<Vec<Planned>, EngineError> {
        let request = Request::new(mutant.id.to_string()).with_args(args.to_vec());
        let chosen = self.chosen(&request, mutant, Asking::ThisRun);
        let mut planned = Vec::new();
        for target in self.selected(None)? {
            if let Chosen::Narrowed { only, .. } = &chosen
                && !only.iter().any(|one| one == &target.id)
            {
                continue;
            }
            planned.push(Planned {
                target: target.id.clone(),
                filter: self.filtering(target, &chosen, cancel)?,
            });
        }
        Ok(planned)
    }

    /// What this run knows of its tree for a record resting on `targets`: each one's skeleton, every item's body, and which of them held their reach under a control.
    ///
    /// # Errors
    /// The engine's refusals while running a control.
    pub fn now(&self, targets: &BTreeSet<String>, cancel: &Cancel) -> Result<Now<'_>, EngineError> {
        let tree = self.carried_tree();
        let mut held = BTreeSet::new();
        for target in targets {
            if self.reach_held(target, cancel)? {
                held.insert(target.clone());
            }
        }
        Ok(Now {
            skeletons: targets
                .iter()
                .map(|target| (target.clone(), tree.skeleton.clone()))
                .collect(),
            items: &tree.items,
            held,
        })
    }

    /// Whether a control of `target` on this tree reached what its baseline did, asked once per run.
    fn reach_held(&self, target: &str, cancel: &Cancel) -> Result<bool, EngineError> {
        let known = self
            .closure
            .carrying
            .held
            .lock()
            .map_err(|_poisoned| SessionError::CarryStatePoisoned)?
            .get(target)
            .copied();
        if let Some(held) = known {
            return Ok(held);
        }
        let controlled = self.control(
            &Request::new(String::new()).with_target(target),
            cancel,
            Observing::Reach,
        )?;
        if cancel.is_cancelled() {
            return Ok(false);
        }
        let held = !controlled.observed.is_empty()
            && controlled
                .observed
                .iter()
                .all(|one| matches!(one.steadiness, Steadiness::Held));
        self.closure
            .carrying
            .held
            .lock()
            .map_err(|_poisoned| SessionError::CarryStatePoisoned)?
            .insert(target.to_owned(), held);
        Ok(held)
    }

    /// What each execution the judgement `asked` rests on, each paired with the route's `plan` entry for its target, or nothing where one of them did not name what it entered or ran a target the plan does not.
    #[must_use]
    pub fn executions(&self, asked: &[MutantResult], plan: &[Planned]) -> Option<Vec<Execution>> {
        if asked.is_empty() || asked.len() > plan.len() {
            return None;
        }
        let tree = self.carried_tree();
        asked
            .iter()
            .map(|result| {
                let planned = plan.iter().find(|one| one.target == result.target)?;
                let entered = result.entered.as_ref()?;
                let items = entered
                    .items
                    .iter()
                    .map(|item| {
                        Some(Entered {
                            item: item.clone(),
                            body_digest: tree.items.get(item)?.digest.clone(),
                        })
                    })
                    .collect::<Option<BTreeSet<_>>>()?;
                Some(Execution {
                    target: planned.target.clone(),
                    filter: planned.filter.clone(),
                    skeleton: tree.skeleton.clone(),
                    entered: items,
                    completeness: entered.completeness,
                    detected: result.outcome().detected(),
                })
            })
            .collect()
    }

    /// Keeps `record` as what this run believed about the mutant `id`, so the run's evidence can say what each carried answer rests on.
    ///
    /// # Errors
    /// [`SessionError::CarryStatePoisoned`].
    pub fn believed(
        &self,
        id: &str,
        record: crate::carry::Carried,
        plan: Vec<Planned>,
    ) -> Result<(), EngineError> {
        self.closure
            .carrying
            .believed
            .lock()
            .map_err(|_poisoned| SessionError::CarryStatePoisoned)?
            .insert(id.to_owned(), (record, plan));
        Ok(())
    }

    /// Every record this run believed, by the full identity of the mutant it answered for.
    ///
    /// # Errors
    /// [`SessionError::CarryStatePoisoned`].
    pub fn carried_evidence(&self) -> Result<crate::carry::Believed, EngineError> {
        let believed = self
            .closure
            .carrying
            .believed
            .lock()
            .map_err(|_poisoned| SessionError::CarryStatePoisoned)?;
        Ok(crate::carry::Believed {
            document_type: crate::carry::DOCUMENT.to_owned(),
            schema_version: crate::carry::DOCUMENT_VERSION,
            records: believed
                .iter()
                .map(|(mutant, (record, plan))| crate::carry::BelievedRecord {
                    mutant: mutant.clone(),
                    record: record.clone(),
                    plan: plan.clone(),
                })
                .collect(),
        })
    }
}
