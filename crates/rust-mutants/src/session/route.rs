// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Which targets could notice a mutation, which of their tests are asked, and what removed the rest.

use std::collections::BTreeMap;

use crate::catalog::Mutant;

/// Which targets could notice a mutation, and what the route rests on.
///
/// A route is the reach layer of [ADR 0004](../../../../docs/adr/0004-proof-layers-not-budgets.md):
/// a rule that removes an execution because evidence the run already holds
/// says the execution could not observe the mutant. Every fallback is toward
/// running more, and every one of them is named, so a reader who sees a run go
/// faster can say which layer did it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
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
        ///
        /// A layer that removes an execution has to name who was in a position
        /// to notice and did not, or a reader has only the word "unreached"
        /// and no way to check it. This is that list, and its length is what a
        /// report counts.
        considered: Vec<String>,
    },
}

/// The proof that a target which never ran the body of the branch a mutation sits in cannot have noticed it.
pub const BRANCH_NEVER_TAKEN: &str = "branch-never-taken";

/// The proof that a target which ran the mutation without its value ever differing cannot have noticed it.
pub const NEVER_INFECTED: &str = "never-infected";

/// Why a route is wider than a measurement alone would make it. Every one of these runs more, never less.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
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
    /// Every reason a route widens, which is what a schema and an audit have to know in full.
    pub const ALL: [Self; 5] = [
        Self::NotMeasured,
        Self::PositionUnknown,
        Self::OutsideBlocks,
        Self::CoverageIncomplete,
        Self::TouchIncomplete,
    ];

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
///
/// The tests belong to the target rather than beside it, so a route cannot
/// name tests of a target it does not keep, and cannot keep a target whose
/// tests it forgot to say anything about. Both were states two parallel
/// collections could reach and neither is a state that means anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reaches {
    /// The target.
    pub target: String,
    /// Which of its tests the mutation is put to.
    pub tests: Asked,
}

/// Which of a target's tests a route puts a mutation to.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
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

    /// How many tests one execution of the target starts, given how many it has.
    #[must_use]
    pub const fn counting(&self, held: usize) -> usize {
        match self {
            Self::Every => held,
            Self::These(tests) => tests.len(),
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
    pub proof: &'static str,
}

/// What a route is decided among.
#[derive(Debug, Clone, Copy)]
pub struct Routing<'a> {
    /// Every target the run built.
    pub targets: &'a [&'a str],
    /// The targets a coverage build can measure, which is every one it compiles.
    ///
    /// A library's documented examples are compiled by rustdoc while cargo
    /// runs them, so no coverage build instruments them and no measurement can
    /// name them. They are not unmeasured targets, they are targets this
    /// measurement is not about, and they reach by the rule in `also_reaching`.
    pub measurable: &'a [&'a str],
    /// The targets routed by a rule other than the measurement.
    pub also_reaching: &'a [&'a str],
}

impl Route {
    /// Every name [`Route::granularity`] can answer, which is what a schema and an audit have to know in full.
    pub const GRANULARITIES: [&'static str; 5] =
        ["all", "block", "test", "discharged", "unreached"];

    /// Which targets a measurement puts at `position` of `path`, out of `targets`.
    ///
    /// A target that ran and whose profile could not be read is kept: what the
    /// measurement says nothing about is run rather than assumed.
    /// A target a coverage build could have measured is measured when the
    /// measurement **names** it, and not otherwise. One that is absent — its
    /// profile unreadable, its run never made, the measurement cut short
    /// before it — is one nothing was established about, and it stays in the
    /// route. Being absent from a measurement is not the same as being
    /// measured and covering nothing, and reading the first as the second
    /// turns a kill into a survivor: the one thing this layer must never do.
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
    ///
    /// The guards record on the run that verifies the baseline, and libtest
    /// names each test's thread after the test, so the answer is per test
    /// rather than per target. A target the record does not name is one
    /// nothing was established about and stays in the route with every test
    /// of it; a site recorded where nothing named a test reaches every test of
    /// its target, for the same reason. Both run more, never less.
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
    #[must_use]
    pub fn started<F: Fn(&str) -> usize>(&self, of: F) -> usize {
        match self {
            Self::Block { reaching, .. } => reaching
                .iter()
                .map(|one| one.tests.counting(of(&one.target)))
                .sum(),
            Self::All { reaching, .. } => reaching.iter().map(|target| of(target)).sum(),
            Self::Discharged { .. } | Self::Unreached { .. } => 0,
        }
    }

    /// What this route would take, as the share of each target's own baseline the tests it names come to.
    ///
    /// A pair is a process, and what a process takes is what its tests take. A
    /// route that puts a mutation to two tests of a target with two hundred
    /// takes a hundredth of that target's baseline, not the whole of it — and
    /// not the whole of the slowest target the build happened to produce,
    /// which is all an estimate that knows one number can reach for. A target
    /// the caller cannot time is priced at whatever it hands back, which is
    /// the guess that errs toward too long.
    #[must_use]
    pub fn costing<F: Fn(&str) -> Timing>(&self, of: F) -> std::time::Duration {
        self.reaching()
            .iter()
            .map(|target| {
                let timing = of(target);
                let all = timing.tests.max(1);
                let named = u32::try_from(self.tests_of(target).len()).unwrap_or(all);
                let asked = if named == 0 { all } else { named.min(all) };
                timing
                    .baseline
                    .checked_div(all)
                    .unwrap_or_default()
                    .saturating_mul(asked)
            })
            .sum()
    }

    /// The granularity a route record carries: `all`, `test`, `block`, `discharged`, or `unreached`.
    #[must_use]
    pub fn granularity(&self) -> &'static str {
        match self {
            Self::All { .. } => "all",
            Self::Block { reaching, .. }
                if reaching.iter().all(|one| one.tests == Asked::Every) =>
            {
                "block"
            }
            Self::Block { .. } => "test",
            Self::Discharged { .. } => "discharged",
            Self::Unreached { .. } => "unreached",
        }
    }

    /// Why the route is wider than the measurement alone would make it, when it is.
    #[must_use]
    pub fn fallback(&self) -> Option<&'static str> {
        match self {
            Self::All { fallback, .. } => Some(fallback.name()),
            Self::Block { fallback, .. } => fallback.map(Fallback::name),
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
    ///
    /// This is what [`super::Session::exec`] runs, and it is deliberately wider than
    /// [`Route::reaching`]: a discharge is a proof a caller may not share, and
    /// `exec` is the question "what do the tests say", asked by a caller with
    /// its own evidence. [`super::Session::judge`] is the one that removes work.
    ///
    /// It is the one place a coverage narrowing is decided: an execution that
    /// narrowed by anything else would run fewer targets than the route says,
    /// and a survivor it reported would be one nobody measured.
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
    ///
    /// A route that keeps nothing has to say who it asked, or "unreached" is a
    /// word with nothing behind it and an audit can only confirm that the
    /// engine said it.
    ///
    /// Only a route that keeps nothing carries the list. A route that kept
    /// some targets and dropped others names the ones it kept and the ones a
    /// proof removed, and says nothing about the ones the measurement placed
    /// elsewhere — an asymmetry, and one a reader asking "why was this target
    /// not run" meets.
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

    /// The targets an execution of this route ran, given the target that answered and whether it detected the mutation.
    ///
    /// [`super::Session::exec`] walks the routed targets in order and stops at the
    /// first one that detects, so what ran is the whole route when nothing
    /// detected and the prefix ending at the answer when one did. An answer
    /// this route does not hold is one target on its own, which is what
    /// `--target` asks for.
    #[must_use]
    pub fn executed(&self, answered: &str, detected: bool) -> Vec<String> {
        if answered.is_empty() {
            return Vec::new();
        }
        let reaching: Vec<String> = self.reaching().into_iter().map(str::to_owned).collect();
        let Some(at) = reaching.iter().position(|target| target == answered) else {
            return vec![answered.to_owned()];
        };
        if detected {
            reaching.into_iter().take(at.saturating_add(1)).collect()
        } else {
            reaching
        }
    }

    /// The record of this decision, with the targets that actually ran.
    #[must_use]
    pub fn record(&self, mutant: &Mutant, executed: Vec<String>) -> crate::trace::RouteRecord {
        crate::trace::RouteRecord {
            mutant: mutant.display_id.clone(),
            index: mutant.index,
            granularity: self.granularity().to_owned(),
            fallback: self.fallback().map(str::to_owned),
            reaching: self.reaching().into_iter().map(str::to_owned).collect(),
            considered: self.considered().to_vec(),
            discharged: self
                .discharged()
                .iter()
                .map(|discharge| crate::trace::DischargeRecord {
                    target: discharge.target.clone(),
                    proof: discharge.proof.to_owned(),
                })
                .collect(),
            executed,
            reused: None,
        }
    }
}
