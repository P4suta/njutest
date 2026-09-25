// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Which targets could notice a mutation, which of their tests are asked, and what removed the rest.

use std::collections::BTreeMap;

use crate::catalog::Mutant;
use crate::count::{Count, Tests};

/// Why a route's projected work cannot be represented exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RouteAccountingError {
    /// One explicit test set is larger than the durable counter.
    #[error("a route names {count} tests, which does not fit its u64 accounting counter")]
    TestCountTooLarge {
        /// The unrepresentable collection length.
        count: usize,
    },
    /// The sum of individually representable test counts is too large.
    #[error("the total number of tests a route starts does not fit its u64 accounting counter")]
    TestCountOverflow,
    /// A narrowed route retained a target but named no test to start.
    #[error("a narrowed route retained a target with an empty test set")]
    EmptyNamedTestSet,
    /// A narrowed route names more tests than the measured target ran.
    #[error(
        "a narrowed route names {named} tests, but the target baseline measured only {measured}"
    )]
    NamedTestsExceedBaseline {
        /// How many tests the route names.
        named: u32,
        /// How many tests the target's baseline measured.
        measured: u32,
    },
    /// Scaling or summing target baselines exceeded [`std::time::Duration`].
    #[error("the duration projected for a route exceeds the duration type")]
    DurationOverflow,
}

/// Which targets could notice a mutation, and what the route rests on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// Every target, because the measurement says nothing about this place.
    All {
        /// Which target could notice it: all of them.
        reaching: Vec<String>,
        /// Why the route is everything.
        fallback: Fallback,
    },
    /// The targets a measurement places at the mutation, and the ones a proof removed.
    Block {
        /// The targets whose measured run covered the position, plus every target the measurement could not read, each with the tests it is asked for.
        reaching: Vec<Reaches>,
        /// The targets a proof removed from what could have noticed the mutation.
        discharged: Vec<Discharge>,
        /// Why targets the measurement did not place are in `reaching` anyway.
        fallback: Option<Fallback>,
    },
    /// A mutation every target was proved unable to notice.
    Discharged {
        /// Each target, with the proof that removed it.
        discharged: Vec<Discharge>,
    },
    /// A mutation no measured target executes, which nothing needs to run to find out again.
    Unreached {
        /// The targets that were measured, asked, and did not reach the mutation, in the order they were offered.
        considered: Vec<String>,
    },
}

/// Why a target that could have been asked about a mutation was not.
///
/// A closed set rather than a name, because the report carries these into a schema and an audit re-derives them: a proof spelled one way in one place and another way elsewhere is a discharge nobody can hold the run to.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum Proof {
    /// The target never ran the body of the branch the mutation sits in.
    BranchNeverTaken,
    /// The target ran the mutation without its value ever differing.
    NeverInfected,
}

impl Proof {
    /// The name a route record carries.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::BranchNeverTaken => "branch-never-taken",
            Self::NeverInfected => "never-infected",
        }
    }
}

impl std::fmt::Display for Proof {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// The proof that a target which never ran the body of the branch a mutation sits in cannot have noticed it.
pub const BRANCH_NEVER_TAKEN: Proof = Proof::BranchNeverTaken;

/// The proof that a target which ran the mutation without its value ever differing cannot have noticed it.
pub const NEVER_INFECTED: Proof = Proof::NeverInfected;

/// How narrowly a run chose which targets to ask about a mutation.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum Granularity {
    /// Every target, because the measurement says nothing about this place.
    All,
    /// The targets a measurement places at the mutation, each asked for all of its tests.
    Block,
    /// The targets a measurement places at the mutation, each asked for the tests that reach it.
    Test,
    /// Every target a proof removed, so nothing runs.
    Discharged,
    /// No measured target executes it, so nothing runs.
    Unreached,
}

impl Granularity {
    /// The name a route record carries.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Block => "block",
            Self::Test => "test",
            Self::Discharged => "discharged",
            Self::Unreached => "unreached",
        }
    }
}

/// Why a route is wider than a measurement alone would make it.
/// Every one of these runs more, never less.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum Fallback {
    /// Nothing was measured at all.
    NotMeasured,
    /// The mutation's position could not be counted in the file a person would open.
    PositionUnknown,
    /// The coverage build instrumented no block holding the position, so the measurement says nothing about it.
    OutsideBlocks,
    /// A target ran and its profile could not be read, so what it reached is unknown.
    CoverageIncomplete,
    /// A target ran and its guards recorded nothing this run can route by, so what it reached is unknown.
    TouchIncomplete,
}

impl Fallback {
    /// The name a route record carries.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::NotMeasured => "not-measured",
            Self::PositionUnknown => "position-unknown",
            Self::OutsideBlocks => "outside-blocks",
            Self::CoverageIncomplete => "coverage-incomplete",
            Self::TouchIncomplete => "touch-incomplete",
        }
    }
}

/// One target a route keeps, and which of its tests it puts the mutation to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reaches {
    /// The target.
    pub target: String,
    /// Which of its tests the mutation is put to.
    pub tests: Asked,
}

/// Which of a target's tests a route puts a mutation to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    /// Every test the target has, because nothing narrowed it to fewer.
    Every,
    /// Exactly these, because the measurement named them and no others reached the mutation.
    These(Vec<String>),
}

impl Asked {
    /// The tests this names, or nothing when it names every test the target has.
    #[must_use]
    pub fn named(&self) -> &[String] {
        match self {
            Self::Every => &[],
            Self::These(tests) => tests,
        }
    }

    fn exact_count(&self, held: u32) -> Result<u32, RouteAccountingError> {
        match self {
            Self::Every => Ok(held),
            Self::These(tests) => {
                let named = u32::try_from(tests.len()).map_err(|_overflow| {
                    RouteAccountingError::TestCountTooLarge { count: tests.len() }
                })?;
                if named == 0 {
                    return Err(RouteAccountingError::EmptyNamedTestSet);
                }
                if named > held {
                    return Err(RouteAccountingError::NamedTestsExceedBaseline {
                        named,
                        measured: held,
                    });
                }
                Ok(named)
            }
        }
    }
}

/// What one target costs a route: how long its own baseline took, and how many tests that was the cost of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Timing {
    /// How long the target's own baseline took, with nothing active.
    pub baseline: std::time::Duration,
    /// How many tests it ran, which is what that duration is the cost of.
    pub tests: u32,
}

impl Timing {
    /// One target's baseline and the tests it was the cost of.
    #[must_use]
    pub const fn new(baseline: std::time::Duration, tests: u32) -> Self {
        Self { baseline, tests }
    }
}

/// One target a proof removed from what could have noticed a mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discharge {
    /// The target.
    pub target: String,
    /// The proof that removed it.
    pub proof: Proof,
}

/// What a route is decided among.
#[derive(Debug, Clone, Copy)]
pub struct Routing<'a> {
    /// Every target the run built.
    pub targets: &'a [&'a str],
    /// The targets a coverage build can measure, which is every one it compiles.
    pub measurable: &'a [&'a str],
    /// The targets routed by a rule other than the measurement.
    pub also_reaching: &'a [&'a str],
}

impl Route {
    /// Which targets a measurement puts at `position` of `path`, out of `targets`.
    #[must_use]
    pub fn decide(
        reached: &crate::reach::Reached,
        path: &std::path::Path,
        position: crate::coverage::Point,
        among: &Routing<'_>,
    ) -> Self {
        let Routing {
            targets,
            measurable,
            also_reaching,
        } = *among;
        let everything = |fallback: Fallback| Self::All {
            reaching: targets.iter().map(|target| (*target).to_owned()).collect(),
            fallback,
        };
        if !reached.measured() {
            return everything(Fallback::NotMeasured);
        }
        let Some(covering) = reached.covering(path, position) else {
            return everything(Fallback::OutsideBlocks);
        };
        let unmeasured: Vec<&str> = measurable
            .iter()
            .copied()
            .filter(|target| {
                !reached.targets.contains_key(*target)
                    || reached.limitations.iter().any(|limitation| {
                        limitation == &format!("{}:{target}", crate::reach::UNMEASURED)
                    })
            })
            .collect();
        let reaching: Vec<Reaches> = targets
            .iter()
            .copied()
            .filter(|target| {
                covering.contains(target)
                    || unmeasured.contains(target)
                    || also_reaching.contains(target)
            })
            .map(|target| Reaches {
                target: target.to_owned(),
                tests: Asked::Every,
            })
            .collect();
        if reaching.is_empty() {
            return Self::Unreached {
                considered: measurable
                    .iter()
                    .map(|target| (*target).to_owned())
                    .collect(),
            };
        }
        Self::Block {
            reaching,
            discharged: Vec::new(),
            fallback: (!unmeasured.is_empty()).then_some(Fallback::CoverageIncomplete),
        }
    }

    /// Which of each target's tests the guards put at `index`, out of `targets`.
    #[must_use]
    pub fn by_touch(touched: &crate::touch::Touched, index: u32, among: &Routing<'_>) -> Self {
        let Routing {
            targets,
            measurable,
            also_reaching,
        } = *among;
        if !touched.measured() {
            return Self::All {
                reaching: targets.iter().map(|target| (*target).to_owned()).collect(),
                fallback: Fallback::NotMeasured,
            };
        }
        let mut reaching = Vec::new();
        let mut considered = Vec::new();
        let mut incomplete = false;
        for target in targets.iter().copied() {
            let asked = if measurable.contains(&target) {
                match touched.reaching(target, index) {
                    None => {
                        incomplete = true;
                        Some(Asked::Every)
                    }
                    Some(crate::touch::Reaching::Nothing) => {
                        considered.push(target.to_owned());
                        None
                    }
                    Some(crate::touch::Reaching::Whole) => Some(Asked::Every),
                    Some(crate::touch::Reaching::Tests(named)) => Some(Asked::These(named)),
                }
            } else {
                also_reaching.contains(&target).then_some(Asked::Every)
            };
            if let Some(tests) = asked {
                reaching.push(Reaches {
                    target: target.to_owned(),
                    tests,
                });
            }
        }
        if reaching.is_empty() {
            return Self::Unreached { considered };
        }
        Self::Block {
            reaching,
            discharged: Vec::new(),
            fallback: incomplete.then_some(Fallback::TouchIncomplete),
        }
    }

    /// The tests of `target` this route names, or nothing when every test of it runs.
    #[must_use]
    pub fn tests_of(&self, target: &str) -> &[String] {
        self.keeps(target).map_or(&[], |one| one.tests.named())
    }

    /// What this route keeps `target` for, or nothing when it does not keep it.
    #[must_use]
    pub fn keeps(&self, target: &str) -> Option<&Reaches> {
        match self {
            Self::Block { reaching, .. } => reaching.iter().find(|one| one.target == target),
            Self::All { .. } | Self::Discharged { .. } | Self::Unreached { .. } => None,
        }
    }

    /// Every target this route keeps, each with the tests it is asked for.
    #[must_use]
    pub fn asked(&self) -> Vec<Reaches> {
        match self {
            Self::Block { reaching, .. } => reaching.clone(),
            Self::All { reaching, .. } => reaching
                .iter()
                .map(|target| Reaches {
                    target: target.clone(),
                    tests: Asked::Every,
                })
                .collect(),
            Self::Discharged { .. } | Self::Unreached { .. } => Vec::new(),
        }
    }

    /// For each target this route narrowed to some of its tests, exactly those tests.
    #[must_use]
    pub fn tests(&self) -> BTreeMap<String, Vec<String>> {
        match self {
            Self::Block { reaching, .. } => reaching
                .iter()
                .filter_map(|one| match &one.tests {
                    Asked::Every => None,
                    Asked::These(tests) => Some((one.target.clone(), tests.clone())),
                })
                .collect(),
            Self::All { .. } | Self::Discharged { .. } | Self::Unreached { .. } => BTreeMap::new(),
        }
    }

    /// Every test this route would start, counted, which is the work it asks for.
    ///
    /// # Errors
    /// Returns [`RouteAccountingError`] instead of truncating or saturating an unrepresentable test count.
    pub fn started<F: Fn(&str) -> u32>(&self, of: F) -> Result<Count<Tests>, RouteAccountingError> {
        let total = match self {
            Self::Block { reaching, .. } => reaching.iter().try_fold(0u64, |total, one| {
                let asked = one.tests.exact_count(of(&one.target))?;
                total
                    .checked_add(u64::from(asked))
                    .ok_or(RouteAccountingError::TestCountOverflow)
            })?,
            Self::All { reaching, .. } => reaching.iter().try_fold(0u64, |total, target| {
                total
                    .checked_add(u64::from(of(target)))
                    .ok_or(RouteAccountingError::TestCountOverflow)
            })?,
            Self::Discharged { .. } | Self::Unreached { .. } => 0,
        };
        Ok(Count::new(total))
    }

    /// What this route would take, as the share of each target's own baseline the tests it names come to.
    ///
    /// # Errors
    /// Returns [`RouteAccountingError`] instead of truncating a named-test count or saturating a projected duration.
    pub fn costing<F: Fn(&str) -> Timing>(
        &self,
        of: F,
    ) -> Result<std::time::Duration, RouteAccountingError> {
        self.reaching()
            .iter()
            .try_fold(std::time::Duration::ZERO, |total, target| {
                let timing = of(target);
                let all = timing.tests.max(1);
                let asked = match self.keeps(target).map(|one| &one.tests) {
                    None | Some(Asked::Every) => all,
                    Some(tests @ Asked::These(_)) => tests.exact_count(timing.tests)?,
                };
                let projected = timing
                    .baseline
                    .checked_div(all)
                    .and_then(|each| each.checked_mul(asked))
                    .ok_or(RouteAccountingError::DurationOverflow)?;
                total
                    .checked_add(projected)
                    .ok_or(RouteAccountingError::DurationOverflow)
            })
    }

    /// The granularity a route record carries: `all`, `test`, `block`, `discharged`, or `unreached`.
    #[must_use]
    pub fn granularity(&self) -> Granularity {
        match self {
            Self::All { .. } => Granularity::All,
            Self::Block { reaching, .. }
                if reaching.iter().all(|one| one.tests == Asked::Every) =>
            {
                Granularity::Block
            }
            Self::Block { .. } => Granularity::Test,
            Self::Discharged { .. } => Granularity::Discharged,
            Self::Unreached { .. } => Granularity::Unreached,
        }
    }

    /// Why the route is wider than the measurement alone would make it, when it is.
    #[must_use]
    pub const fn fallback(&self) -> Option<Fallback> {
        match self {
            Self::All { fallback, .. } => Some(*fallback),
            Self::Block { fallback, .. } => *fallback,
            Self::Discharged { .. } | Self::Unreached { .. } => None,
        }
    }

    /// The targets that could notice the mutation.
    #[must_use]
    pub fn reaching(&self) -> Vec<&str> {
        match self {
            Self::All { reaching, .. } => reaching.iter().map(String::as_str).collect(),
            Self::Block { reaching, .. } => {
                reaching.iter().map(|one| one.target.as_str()).collect()
            }
            Self::Discharged { .. } | Self::Unreached { .. } => Vec::new(),
        }
    }

    /// The targets the coverage measurement alone places at the mutation, or nothing when it places none.
    #[must_use]
    pub fn narrowing(&self) -> Option<Vec<String>> {
        let with_discharged = |reaching: &[String], discharged: &[Discharge]| {
            let mut every: Vec<String> = reaching.to_vec();
            every.extend(discharged.iter().map(|one| one.target.clone()));
            every.sort();
            every.dedup();
            every
        };
        match self {
            Self::All { .. } => None,
            Self::Block {
                reaching,
                discharged,
                ..
            } => Some(with_discharged(
                &reaching
                    .iter()
                    .map(|one| one.target.clone())
                    .collect::<Vec<String>>(),
                discharged,
            )),
            Self::Discharged { discharged } => Some(with_discharged(&[], discharged)),
            Self::Unreached { .. } => Some(Vec::new()),
        }
    }

    /// The targets that were measured, asked, and did not reach the mutation.
    #[must_use]
    pub fn considered(&self) -> &[String] {
        match self {
            Self::Unreached { considered } => considered,
            Self::All { .. } | Self::Block { .. } | Self::Discharged { .. } => &[],
        }
    }

    /// The targets a proof removed, each with the proof's name.
    #[must_use]
    pub fn discharged(&self) -> &[Discharge] {
        match self {
            Self::Block { discharged, .. } | Self::Discharged { discharged } => discharged,
            Self::All { .. } | Self::Unreached { .. } => &[],
        }
    }

    /// The record of this decision, with the targets that actually ran.
    #[must_use]
    pub fn record(&self, mutant: &Mutant, executed: Vec<String>) -> crate::trace::RouteRecord {
        crate::trace::RouteRecord {
            mutant: mutant.display_id.to_string(),
            index: mutant.index,
            granularity: self.granularity(),
            fallback: self.fallback(),
            reaching: self.reaching().into_iter().map(str::to_owned).collect(),
            considered: self.considered().to_vec(),
            discharged: self
                .discharged()
                .iter()
                .map(|discharge| crate::trace::DischargeRecord {
                    target: discharge.target.clone(),
                    proof: discharge.proof.name().to_owned(),
                })
                .collect(),
            executed,
            reused: None,
        }
    }
}
