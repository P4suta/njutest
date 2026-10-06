// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a runner recording says each run again against a moved target came to (ADR 0036), read from the stream alone.

use serde_json::Value;

/// One disposition run again against a target whose reach moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repair {
    /// The mutant, by the name a person types.
    pub mutant: String,
    /// The moved target it was run against.
    pub target: String,
    /// The disposition it had.
    pub was: String,
    /// The one it has now.
    pub now: String,
    /// Whether that run reached the mutant's site: `reached`, `not-reached` or `unrecorded`; for a sealed put, whether it put a test of the target.
    pub reached: String,
    /// What ran it again.
    pub by: By,
}

/// What ran a disposition resting on a moved target again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum By {
    /// A native execution of a lead against the target, with its reach recorded.
    Native,
    /// A sealed verdict put again on the sealed bench, and what that put established.
    Sealed(Put),
}

/// What putting a sealed verdict again on the sealed bench established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Put {
    /// A verdict, resting on these sealed executions, each its target, its test and what it came to.
    Established(Vec<(String, String, String)>),
    /// Nothing, for every reason named, so the disposition is a lead the native run judged.
    Unproven(Vec<String>),
}

impl Repair {
    /// Whether it replaced the disposition: a native run that reached the site or came to something else, and every sealed put, which re-establishes the verdict or leaves a lead.
    #[must_use]
    pub fn replaced(&self) -> bool {
        match &self.by {
            By::Native => self.reached == "reached" || self.now != self.was,
            By::Sealed(_) => true,
        }
    }

    /// Whether it decided the disposition against its target so that nothing of it rests there any more: a native run that reached the site, or a sealed put that re-established the verdict with the target counted among those reaching it.
    #[must_use]
    pub fn settles(&self) -> bool {
        match &self.by {
            By::Native => self.reached == "reached",
            By::Sealed(Put::Established(_)) => true,
            By::Sealed(Put::Unproven(_)) => false,
        }
    }

    /// Whether a sealed put, rather than a native run, ran it again.
    #[must_use]
    pub const fn sealed(&self) -> bool {
        match &self.by {
            By::Native => false,
            By::Sealed(_) => true,
        }
    }
}

/// How many dispositions `repairs` replaced against `target`, each counted once however many runs of it did.
#[must_use]
pub fn replaced_against(repairs: &[Repair], target: &str) -> usize {
    let mut mutants: Vec<&str> = repairs
        .iter()
        .filter(|repair| repair.target == target && repair.replaced())
        .map(|repair| repair.mutant.as_str())
        .collect();
    mutants.sort_unstable();
    mutants.dedup();
    mutants.len()
}

/// Every repair record of a runner recording, in the order it was written.
///
/// # Errors
/// [`crate::route::ReadError`] for a repair record without a field its schema requires.
pub fn read(
    recorded: &crate::route::Checked<crate::schemas::RunnerLines>,
) -> Result<Vec<Repair>, crate::route::ReadError> {
    let mut repairs = Vec::new();
    for (at, event) in recorded.events().iter().enumerate() {
        if event.get("type").and_then(Value::as_str) != Some("repair") {
            continue;
        }
        let placed = |cause| crate::route::ReadError {
            line: at.saturating_add(1),
            cause,
        };
        let record = crate::route::required(event, "repair", Some).map_err(placed)?;
        repairs.push(repair(record).map_err(placed)?);
    }
    Ok(repairs)
}

/// One repair record, or the first field of it that is not there.
fn repair(record: &Value) -> Result<Repair, crate::route::ReadCauseError> {
    let text =
        |key: &str| crate::route::required(record, key, |value| value.as_str().map(str::to_owned));
    Ok(Repair {
        mutant: text("mutant")?,
        target: text("target")?,
        was: text("was")?,
        now: text("now")?,
        reached: text("reached")?,
        by: by(crate::route::required(record, "by", Some)?)?,
    })
}

/// What ran a repair again, as its record says.
fn by(record: &Value) -> Result<By, crate::route::ReadCauseError> {
    let absent = |field: &str| crate::route::ReadCauseError::Absent {
        field: field.to_owned(),
    };
    match record.get("kind").and_then(Value::as_str) {
        Some("native") => Ok(By::Native),
        Some("sealed") => {
            let evidence = crate::route::required(record, "evidence", Some)?;
            match evidence.get("kind").and_then(Value::as_str) {
                Some("sealed") => crate::route::required(evidence, "executions", Value::as_array)?
                    .iter()
                    .map(|run| {
                        let field = |key: &str| {
                            crate::route::required(run, key, |value| {
                                value.as_str().map(str::to_owned)
                            })
                        };
                        Ok((field("target")?, field("test")?, field("came_to")?))
                    })
                    .collect::<Result<Vec<_>, crate::route::ReadCauseError>>()
                    .map(|runs| By::Sealed(Put::Established(runs))),
                Some("unproven") => crate::route::required(evidence, "reasons", Value::as_array)?
                    .iter()
                    .map(|reason| {
                        reason
                            .as_str()
                            .map(str::to_owned)
                            .ok_or_else(|| absent("by/evidence/reasons"))
                    })
                    .collect::<Result<Vec<_>, crate::route::ReadCauseError>>()
                    .map(|reasons| By::Sealed(Put::Unproven(reasons))),
                Some(_) | None => Err(absent("by/evidence/kind")),
            }
        }
        Some(_) | None => Err(absent("by/kind")),
    }
}

/// How a route ranks the holes a run can leave, weakest first, which is how the runs of one disposition again against several moved targets are joined.
const HOLES: [&str; 4] = ["step-limit-reached", "waited", "unconfirmed", "errored"];

/// What the repairs `of_it` of one disposition come to together, whatever order they ran in: the verdict a sealed put re-established; otherwise, over its native runs against the moved targets it rested on, a kill over the worst hole over a pass that reached the site over what any other run decided, what the native run judged of it where a sealed put left it a lead, and what it was where none decided anything; nothing where it was run again against none (ADR 0036 decision 1).
#[must_use]
pub fn joined(of_it: &[&Repair]) -> Option<String> {
    let first = of_it.first()?;
    if let Some(put) = of_it
        .iter()
        .find(|one| matches!(one.by, By::Sealed(Put::Established(_))))
    {
        return Some(put.now.clone());
    }
    let judged = of_it
        .iter()
        .find(|one| matches!(one.by, By::Sealed(Put::Unproven(_))))
        .map(|one| one.now.clone());
    let mut decided: Vec<&Repair> = of_it
        .iter()
        .copied()
        .filter(|one| !one.sealed() && one.replaced())
        .collect();
    decided.sort_by(|left, right| left.target.cmp(&right.target));
    if decided.iter().any(|one| one.now == "killed") {
        return Some("killed".to_owned());
    }
    if let Some(worst) = HOLES
        .iter()
        .rev()
        .find(|hole| decided.iter().any(|one| one.now == **hole))
    {
        return Some((*worst).to_owned());
    }
    if decided.iter().any(|one| one.now == "survived") {
        return Some("survived".to_owned());
    }
    Some(match (decided.first(), judged) {
        (Some(one), _) => one.now.clone(),
        (None, Some(judged)) => judged,
        (None, None) => first.was.clone(),
    })
}

/// What one repair's own evidence decides: whether its run reached the site, and the dispositions it may now carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Derived {
    /// `reached`, `not-reached` or `unrecorded`, from the repair's own touch record.
    pub reached: &'static str,
    /// Every disposition its last execution allows: a kill or a wait is itself or, where its confirmation did not hold, `unconfirmed`, and a run that did not run to an answer is `declined` where every test declined and `errored` otherwise.
    pub now: Vec<String>,
}

/// Where a repair's evidence cannot be read into a decision.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RepairContradictionError {
    /// No execution of the mutant against the target was recorded.
    #[error("no execution of it against {target} was recorded")]
    NotRun {
        /// The moved target.
        target: String,
    },
    /// An execution came to something no run comes to.
    #[error("its last execution came to {outcome:?}, which no run comes to")]
    Outcome {
        /// What it said.
        outcome: String,
    },
}

impl crate::error::Coded for RepairContradictionError {
    fn code(&self) -> crate::error::XtCode {
        match self {
            Self::NotRun { .. } | Self::Outcome { .. } => crate::error::XtCode::RepairContradicted,
        }
    }
}

/// What a repair of a mutation at `index` whose last execution came to `outcome`, with the repair touch record `touch`, decides, `was` being what it had.
///
/// # Errors
/// [`RepairContradictionError::Outcome`] for an outcome no run comes to.
pub fn derived(
    was: &str,
    outcome: &str,
    (index, touch): (u64, Option<&crate::drift::Touch>),
) -> Result<Derived, RepairContradictionError> {
    let reached = match touch {
        None => "unrecorded",
        Some(touch) if touch.reached.contains(&index) => "reached",
        Some(_) => "not-reached",
    };
    let now: Vec<String> = match outcome {
        "survived" if reached == "reached" => vec!["survived".to_owned()],
        "survived" => vec![was.to_owned()],
        "killed" => vec!["killed".to_owned(), "unconfirmed".to_owned()],
        "waited" => vec!["waited".to_owned(), "unconfirmed".to_owned()],
        "step_limit_reached" => vec!["step-limit-reached".to_owned()],
        "not_run" => vec!["declined".to_owned(), "errored".to_owned()],
        "errored" | "inconclusive" => vec!["errored".to_owned()],
        other => {
            return Err(RepairContradictionError::Outcome {
                outcome: other.to_owned(),
            });
        }
    };
    Ok(Derived { reached, now })
}
