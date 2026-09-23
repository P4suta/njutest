// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What one mutation stands on when more than one build of the project measured it.

use std::collections::BTreeSet;
use std::fmt;

use super::{
    BlindIn, BuildEvidence, BuildLedger, BuildName, BuildNameError, BuildPartEvidence, BuildReport,
    BuildSelection, CatalogPart, Decision, Finding, FindingKind, FindingOrigin, LatticedDocument,
    LatticedReport, PartLedger, ShardBuildEvidence, ShardBuildLedger, ShardReport,
};

/// One configured build measured into a mutable, non-serializable draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildMeasurement {
    name: BuildName,
    selection: BuildSelection,
    report: BuildReport,
}

/// A non-empty configured-build measurement list in request order.
///
/// The primary `default` build is stored separately, so reconciliation cannot be called with no evidence and invent an error decision for an empty set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildMeasurements {
    first: BuildMeasurement,
    rest: Vec<BuildMeasurement>,
}

/// Why raw configured-build drafts cannot form a measurement ledger.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BuildMeasurementsError {
    /// No build was measured.
    #[error("no configured build was measured")]
    Empty,
    /// One raw name was not canonical.
    #[error(transparent)]
    Name(#[from] BuildNameError),
    /// The primary build was not first.
    #[error("the first configured build must be {expected:?}, not {actual:?}")]
    DefaultFirst {
        /// The required primary name.
        expected: &'static str,
        /// The first measured name.
        actual: BuildName,
    },
    /// The same configured name occurred twice.
    #[error("configured build name {name:?} occurs more than once")]
    Duplicate {
        /// The repeated canonical name.
        name: BuildName,
    },
}

impl BuildMeasurements {
    /// Validates raw build names and proves non-emptiness, primary-first order,
    /// and uniqueness before reconciliation can observe the measurements.
    ///
    /// # Errors
    /// Returns the exact name, ordering, emptiness, or uniqueness invariant violated by the supplied build measurements.
    pub fn checked(
        measured: Vec<(String, BuildSelection, BuildReport)>,
    ) -> Result<Self, BuildMeasurementsError> {
        if measured.is_empty() {
            return Err(BuildMeasurementsError::Empty);
        }
        let typed = measured
            .into_iter()
            .map(|(name, selection, report)| {
                Ok(BuildMeasurement {
                    name: BuildName::try_from(name)?,
                    selection,
                    report,
                })
            })
            .collect::<Result<Vec<_>, BuildMeasurementsError>>()?;
        let mut typed = typed.into_iter();
        let Some(first) = typed.next() else {
            return Err(BuildMeasurementsError::Empty);
        };
        let rest: Vec<_> = typed.collect();
        if first.name.as_str() != crate::config::DEFAULT_CONFIGURATION {
            return Err(BuildMeasurementsError::DefaultFirst {
                expected: crate::config::DEFAULT_CONFIGURATION,
                actual: first.name,
            });
        }
        let mut names = BTreeSet::new();
        for measured in std::iter::once(&first).chain(rest.iter()) {
            if !names.insert(measured.name.clone()) {
                return Err(BuildMeasurementsError::Duplicate {
                    name: measured.name.clone(),
                });
            }
        }
        Ok(Self { first, rest })
    }

    /// Every measured build in exact request order.
    pub fn iter(&self) -> impl Iterator<Item = &BuildMeasurement> {
        std::iter::once(&self.first).chain(self.rest.iter())
    }

    /// The primary measured draft.
    #[must_use]
    pub const fn first(&self) -> &BuildMeasurement {
        &self.first
    }
}

impl BuildMeasurement {
    /// The canonical configured name.
    #[must_use]
    pub const fn name(&self) -> &BuildName {
        &self.name
    }

    /// The exact build selection.
    #[must_use]
    pub const fn selection(&self) -> &BuildSelection {
        &self.selection
    }

    /// The mutable build draft to close.
    #[must_use]
    pub const fn report(&self) -> &BuildReport {
        &self.report
    }
}

/// What a run records about one mutation, taken across every build that measured it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// What the run records, which is the weakest thing any build established.
    decision: Decision,
    /// The builds it is a hole in, each with what that build established, in the order a report lists them.
    /// A run of one build names none: which build is not a question it has.
    blind_in: Vec<BlindIn>,
}

impl Resolved {
    /// The weakest decision any configured build established.
    #[must_use]
    pub const fn decision(&self) -> Decision {
        self.decision
    }

    /// Every configured build in which the mutation remains a hole.
    #[must_use]
    pub fn blind_in(&self) -> &[BlindIn] {
        &self.blind_in
    }
}

/// What a non-empty ordered build-decision ledger leaves one mutation standing on.
#[must_use]
pub fn across(first: (&BuildName, Decision), rest: &[(&BuildName, Decision)]) -> Resolved {
    let decision = rest.iter().fold(first.1, |weakest, (_, candidate)| {
        if candidate.standing() < weakest.standing() {
            *candidate
        } else {
            weakest
        }
    });
    let blind_in = std::iter::once(first)
        .chain(rest.iter().copied())
        .filter_map(|(build, held)| {
            Some(BlindIn {
                build: build.clone(),
                decision: held.blind()?,
            })
        })
        .collect();
    Resolved { decision, blind_in }
}

/// Why the builds a run measured are not builds of one catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfiguredError {
    /// Raw measurements did not form one ordered non-empty ledger.
    Measurements(BuildMeasurementsError),
    /// A mutation one build catalogued and another did not.
    CataloguesDiffer {
        /// The mutation.
        mutant: String,
        /// The build that did not catalogue it.
        build: BuildName,
    },
    /// A fact this format does not reconcile differs between builds.
    BuildsDiffer {
        /// The field whose meaning would otherwise be lost by cloning one build.
        about: &'static str,
        /// The build that differs from the first.
        build: BuildName,
    },
    /// The reconciled rows and their durable evidence contradict one another.
    Unsound {
        /// What the durable-report audit refused.
        because: String,
    },
    /// Per-build model evidence cannot be rebound to the final run namespace.
    ModelPipelineRequired,
}

impl fmt::Display for ConfiguredError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Measurements(error) => error.fmt(f),
            Self::CataloguesDiffer { mutant, build } => write!(
                f,
                "the build {build:?} did not catalogue the mutation {mutant}, so the \
                 run cannot answer for it across the builds; taking the answers it \
                 does have would report that build's silence as agreement"
            ),
            Self::BuildsDiffer { about, build } => write!(
                f,
                "configured build {build:?} disagrees about {about}; this report version does not \
                 silently choose one build's fact over another"
            ),
            Self::Unsound { because } => write!(
                f,
                "the configured builds do not form one sound report: {because}"
            ),
            Self::ModelPipelineRequired => write!(
                f,
                "verified-v1 model evidence was produced inside more than one build; model \
                 checking must run once after the cross-build mutation lattice so every retained \
                 artifact belongs to the final run namespace"
            ),
        }
    }
}

impl std::error::Error for ConfiguredError {}

impl From<BuildMeasurementsError> for ConfiguredError {
    fn from(error: BuildMeasurementsError) -> Self {
        Self::Measurements(error)
    }
}

impl From<BuildNameError> for ConfiguredError {
    fn from(error: BuildNameError) -> Self {
        Self::Measurements(BuildMeasurementsError::Name(error))
    }
}

/// The report the run writes from the builds it measured, where a mutation stands on the weakest of them.
///
/// # Errors
/// [`ConfiguredError`]: nothing to reconcile, or builds that did not catalogue the same mutations.
pub fn configured(
    final_run_id: &rust_mutants::id::RunId,
    measured: &BuildMeasurements,
) -> Result<LatticedDocument, ConfiguredError> {
    let first = measured.first().report();
    validate_build_names(measured, first)?;
    same_catalogue(measured)?;
    same_unreconciled(measured)?;
    reject_per_build_models(measured)?;
    let global_findings = global_findings(measured, first)?;
    let sources = source_evidence(measured)?;
    lattice_from_sources(first, final_run_id, sources, global_findings)
}

type SourceEvidence = (BuildName, BuildSelection, BuildPartEvidence);

fn reject_per_build_models(measured: &BuildMeasurements) -> Result<(), ConfiguredError> {
    if measured.iter().any(|measurement| {
        let report = measurement.report();
        report.mutants.iter().any(|row| {
            matches!(
                row.outcome.outcome(),
                super::Outcome::ModelNoticed | super::Outcome::ModelProved
            )
        })
    }) {
        return Err(ConfiguredError::ModelPipelineRequired);
    }
    Ok(())
}

fn global_findings(
    measured: &BuildMeasurements,
    first: &BuildReport,
) -> Result<Vec<Finding>, ConfiguredError> {
    let global_findings: Vec<_> = first
        .findings
        .iter()
        .filter(|finding| finding.kind == FindingKind::UnmatchedAcceptance)
        .cloned()
        .collect();
    for measurement in measured.iter().skip(1) {
        let name = measurement.name();
        let report = measurement.report();
        let actual: Vec<_> = report
            .findings
            .iter()
            .filter(|finding| finding.kind == FindingKind::UnmatchedAcceptance)
            .cloned()
            .collect();
        if actual != global_findings {
            return Err(ConfiguredError::BuildsDiffer {
                about: "run-wide unmatched acceptances",
                build: name.clone(),
            });
        }
    }
    Ok(global_findings)
}

fn source_evidence(measured: &BuildMeasurements) -> Result<Vec<SourceEvidence>, ConfiguredError> {
    measured
        .iter()
        .map(|measurement| {
            let name = measurement.name();
            let configuration = measurement.selection();
            let report = measurement.report();
            let part = CatalogPart::parse(report.scope.shard.as_deref()).map_err(|error| {
                ConfiguredError::Unsound {
                    because: error.to_string(),
                }
            })?;
            let run_id =
                rust_mutants::id::RunId::try_from(report.run_id.clone()).map_err(|error| {
                    ConfiguredError::Unsound {
                        because: error.to_string(),
                    }
                })?;
            let findings = report
                .findings
                .iter()
                .filter(|finding| finding.kind != FindingKind::UnmatchedAcceptance)
                .cloned()
                .map(|mut finding| {
                    finding.origin = FindingOrigin::Source {
                        build: name.clone(),
                        run_id: run_id.clone(),
                        part,
                    };
                    finding
                })
                .collect();
            let source = BuildPartEvidence::from_report(report, run_id, part, findings).map_err(
                |error| ConfiguredError::Unsound {
                    because: error.to_string(),
                },
            )?;
            Ok((name.clone(), configuration.clone(), source))
        })
        .collect()
}

fn lattice_from_sources(
    first: &BuildReport,
    final_run_id: &rust_mutants::id::RunId,
    sources: Vec<SourceEvidence>,
    global_findings: Vec<Finding>,
) -> Result<LatticedDocument, ConfiguredError> {
    let first_part = CatalogPart::parse(first.scope.shard.as_deref()).map_err(|error| {
        ConfiguredError::Unsound {
            because: error.to_string(),
        }
    })?;
    match first_part {
        CatalogPart::Whole => {
            let builds = sources
                .into_iter()
                .map(|(name, configuration, source)| {
                    let parts = PartLedger::checked(vec![source]).map_err(|error| {
                        ConfiguredError::Unsound {
                            because: error.to_string(),
                        }
                    })?;
                    Ok(BuildEvidence::from_parts(name, configuration, parts))
                })
                .collect::<Result<Vec<_>, ConfiguredError>>()?;
            let ledger =
                BuildLedger::try_from_vec(builds).map_err(|error| ConfiguredError::Unsound {
                    because: error.to_string(),
                })?;
            LatticedReport::from_parts(first, final_run_id, ledger, global_findings)
                .map(LatticedDocument::Complete)
                .map_err(|error| ConfiguredError::Unsound {
                    because: error.to_string(),
                })
        }
        CatalogPart::Shard(_) => {
            let builds = sources
                .into_iter()
                .map(|(name, configuration, source)| {
                    ShardBuildEvidence::from_parts(name, configuration, source)
                })
                .collect();
            let ledger = ShardBuildLedger::try_from_vec(builds).map_err(|error| {
                ConfiguredError::Unsound {
                    because: error.to_string(),
                }
            })?;
            ShardReport::from_parts(first, final_run_id, ledger, global_findings)
                .map(LatticedDocument::Shard)
                .map_err(|error| ConfiguredError::Unsound {
                    because: error.to_string(),
                })
        }
    }
}

fn validate_build_names(
    measured: &BuildMeasurements,
    first: &BuildReport,
) -> Result<(), ConfiguredError> {
    let names: Vec<&BuildName> = measured.iter().map(BuildMeasurement::name).collect();
    let expected = first
        .scope
        .configured_builds
        .iter()
        .map(|name| BuildName::try_from(name.clone()))
        .collect::<Result<Vec<_>, BuildNameError>>()?;
    if names.iter().copied().ne(expected.iter()) {
        return Err(ConfiguredError::Unsound {
            because: format!(
                "the measured builds {names:?} differ from the request's ordered builds {expected:?}"
            ),
        });
    }
    Ok(())
}

fn same_unreconciled(measured: &BuildMeasurements) -> Result<(), ConfiguredError> {
    let first = measured.first().report();
    for measurement in measured.iter().skip(1) {
        let build = measurement.name();
        let report = measurement.report();
        macro_rules! same {
            ($about:literal, $field:expr) => {
                if ($field)(first) != ($field)(report) {
                    return Err(ConfiguredError::BuildsDiffer {
                        about: $about,
                        build: build.clone(),
                    });
                }
            };
        }
        same!("the contract", |one: &BuildReport| one.contract);
        same!("the run scope", |one: &BuildReport| one.run_kind);
        for (about, agrees) in [
            (
                "schema identity",
                first.schema == report.schema && first.schema_version == report.schema_version,
            ),
            ("the producing tools", first.tool == report.tool),
            ("the repository", first.repository == report.repository),
            (
                "the evidence provenance",
                first.provenance == report.provenance,
            ),
            ("the requested scope", first.scope == report.scope),
        ] {
            if !agrees {
                return Err(ConfiguredError::BuildsDiffer {
                    about,
                    build: build.clone(),
                });
            }
        }
    }
    Ok(())
}

/// Whether every build catalogued the same mutations, which is what makes them builds of one catalog.
fn same_catalogue(measured: &BuildMeasurements) -> Result<(), ConfiguredError> {
    let first = measured.first().report();
    let expected: BTreeSet<&str> = first.mutants.iter().map(|one| one.id.as_str()).collect();
    for measurement in measured.iter() {
        let build = measurement.name();
        let part = measurement.report();
        let held: BTreeSet<&str> = part.mutants.iter().map(|one| one.id.as_str()).collect();
        if let Some(missing) = expected.difference(&held).next() {
            return Err(ConfiguredError::CataloguesDiffer {
                mutant: (*missing).to_owned(),
                build: build.clone(),
            });
        }
        if let Some(extra) = held.difference(&expected).next() {
            return Err(ConfiguredError::CataloguesDiffer {
                mutant: (*extra).to_owned(),
                build: measured.first().name().clone(),
            });
        }
    }
    Ok(())
}
