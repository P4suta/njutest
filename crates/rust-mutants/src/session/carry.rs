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

/// What a run established about a mutation, which a carried record keeps beside the executions it rests on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answered {
    /// Everything the record is filed under but the locus.
    pub keyed: crate::outcomes::Keyed,
    /// What the executions established.
    pub outcome: crate::outcomes::CacheOutcome,
    /// The target that answered.
    pub target: String,
    /// How many tests the answering execution ran, when the harness said.
    pub tests_run: Option<u32>,
    /// Every test that failed with the mutant active.
    pub failed_tests: Vec<String>,
    /// The run that established it.
    pub run_id: String,
}

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

    /// Whether `record`, an earlier tree's answer about `mutant`, holds here under every premise of ADR 0041, and the premise it fails where it does not.
    ///
    /// The route `mutant` would take with `args` is planned without running, the tree is read for every target the record or the plan names, and a survival that reaches the mutant through a target this run cannot see into is refused as uncontrolled.
    /// A believed record is kept as this run's evidence with the plan it was believed against.
    ///
    /// # Errors
    /// Planning the route, reading the tree, or keeping the evidence.
    pub fn believing(
        &self,
        record: &crate::carry::Carried,
        mutant: &Mutant,
        (args, cancel): (&[String], &Cancel),
    ) -> Result<Result<(), crate::carry::Refusal>, EngineError> {
        let plan = self.plan(mutant, args, cancel)?;
        let targets: BTreeSet<String> = record
            .executions
            .iter()
            .map(|one| one.target.clone())
            .chain(plan.iter().map(|one| one.target.clone()))
            .collect();
        let now = self.now(&targets, cancel)?;
        let verdict = crate::carry::believe(record, &now, &plan).and_then(|()| {
            if self.believable(mutant, record.outcome) {
                Ok(())
            } else {
                Err(crate::carry::Refusal::Uncontrolled)
            }
        });
        if verdict.is_ok() {
            self.believed(mutant.id.as_str(), record.clone(), plan)?;
        }
        Ok(verdict)
    }

    /// Whether this run may believe a remembered `outcome`: a survival is a claim about every target that reaches the mutant, and one that reaches it through a process this run cannot see into is a claim this run could not make.
    #[must_use]
    pub fn believable(&self, mutant: &Mutant, outcome: crate::outcomes::CacheOutcome) -> bool {
        match outcome {
            crate::outcomes::CacheOutcome::Killed => true,
            crate::outcomes::CacheOutcome::Survived => !self
                .route(mutant)
                .reaching()
                .into_iter()
                .any(|target| self.uncontrolled(target)),
        }
    }

    /// The record this run leaves under `mutant`'s locus: `answered`, and every execution in `asked` it rests on, each paired with the route's plan by target.
    /// Nothing where the mutant has no locus, where an execution does not name what it entered or ran a target the plan does not, or where the record would not validate.
    ///
    /// # Errors
    /// Planning the route.
    pub fn carrying(
        &self,
        mutant: &Mutant,
        (args, cancel): (&[String], &Cancel),
        (answered, asked): (Answered, &[MutantResult]),
    ) -> Result<Option<crate::carry::Carried>, EngineError> {
        let Some(locus) = self.locus(mutant) else {
            return Ok(None);
        };
        let plan = self.plan(mutant, args, cancel)?;
        let Some(executions) = self.executions(asked, &plan) else {
            return Ok(None);
        };
        let record = crate::carry::Carried {
            schema: crate::carry::SCHEMA.to_owned(),
            locus,
            keyed: answered.keyed,
            outcome: answered.outcome,
            target: answered.target,
            tests_run: answered.tests_run,
            failed_tests: answered.failed_tests,
            run_id: answered.run_id,
            executions,
        };
        Ok(record.validate().is_ok().then_some(record))
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
