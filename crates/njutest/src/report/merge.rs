// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Combining every typed shard of one catalog into one complete answer.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    BuildEvidence, BuildLedger, BuildReport, LatticedReport, MergeSource, MergeSources, PartLedger,
    SCHEMA, SCHEMA_VERSION, ShardReport,
};

/// Why some shard documents are not the parts of one catalog.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MergeError {
    /// No parts were offered.
    #[error("{}: no shard reports were offered", crate::error::MERGE_REFUSED.code)]
    Nothing,
    /// The parts do not agree about one common input.
    #[error(
        "{}: shard reports disagree about {about}: {first} and {other}",
        crate::error::MERGE_REFUSED.code
    )]
    Disagree {
        /// The common fact that differed.
        about: &'static str,
        /// What the canonical first shard said.
        first: String,
        /// What another shard said.
        other: String,
    },
    /// A shard index occurred more than once.
    #[error(
        "{}: shard {index}/{of} was offered more than once",
        crate::error::MERGE_REFUSED.code
    )]
    DuplicateShard {
        /// The repeated one-based index.
        index: u32,
        /// The common denominator.
        of: u32,
    },
    /// A shard was absent.
    #[error(
        "{}: shard {index}/{of} is missing",
        crate::error::MERGE_REFUSED.code
    )]
    MissingShard {
        /// The absent one-based index.
        index: u32,
        /// The common denominator.
        of: u32,
    },
    /// One document named a different denominator.
    #[error(
        "{}: shard {index}/{actual} disagrees with denominator {expected}",
        crate::error::MERGE_REFUSED.code
    )]
    Denominator {
        /// The shard index.
        index: u32,
        /// The expected denominator.
        expected: u32,
        /// The other denominator.
        actual: u32,
    },
    /// Two input documents used one namespace.
    #[error(
        "{}: source document namespace {run_id} occurs more than once",
        crate::error::MERGE_REFUSED.code
    )]
    DuplicateRun {
        /// The repeated canonical namespace.
        run_id: rust_mutants::id::RunId,
    },
    /// The reconstructed whole contradicted its own evidence.
    #[error(
        "{}: the merged report is internally unsound: {because}",
        crate::error::MERGE_REFUSED.code
    )]
    Unsound {
        /// What the checked constructor refused.
        because: String,
    },
}

/// Merges one complete typed shard set under a fresh final namespace.
///
/// Input order is irrelevant: shard identity determines canonical order.
///
/// # Errors
/// Refuses an incomplete/duplicate division, disagreement between common inputs, ownerless model evidence, or a reconstructed report that fails its checked constructor.
pub fn merge(
    final_run_id: &rust_mutants::id::RunId,
    parts: &[ShardReport],
) -> Result<LatticedReport, MergeError> {
    let ordered = ordered(parts)?;
    let first = ordered.first().copied().ok_or(MergeError::Nothing)?;
    agree(&ordered, first)?;
    let builds = merge_builds(&ordered)?;
    let first_build = builds.first().ok_or(MergeError::Nothing)?;
    let first_source = first_build.baseline();
    let template = BuildReport {
        schema: SCHEMA.to_owned(),
        schema_version: SCHEMA_VERSION,
        run_id: final_run_id.to_string(),
        run_kind: first.run_kind,
        contract: first.contract,
        verdict: first.verdict(),
        tool: first.tool.clone(),
        toolchain: first_source.toolchain.clone(),
        repository: first.repository.clone(),
        provenance: first.provenance.clone(),
        scope: first.scope.clone(),
        timing: first_source.timing.clone(),
        accounting: first_source.accounting,
        resources: first_source.resources.clone(),
        candidates: first_source.candidates.clone(),
        seams: first_source.seams.clone(),
        targets: first_source.targets.clone(),
        sources: first_source.sources.clone(),
        mutants: first_source.mutants.clone(),
        findings: first_source.findings.clone(),
        limitations: first_source.limitations.clone(),
        drift: first_source.drift.clone(),
        knobs: first_source.knobs.clone(),
    };
    let ledger = BuildLedger::try_from_vec(builds).map_err(|error| MergeError::Unsound {
        because: error.to_string(),
    })?;
    let sources = MergeSources::checked(
        ordered
            .iter()
            .map(|report| MergeSource::from_parts(report.run_id.clone(), report.shard))
            .collect(),
    )
    .map_err(|error| MergeError::Unsound {
        because: error.to_string(),
    })?;
    LatticedReport::from_merged_parts(
        &template,
        final_run_id,
        ledger,
        super::MergedCompletion {
            global_findings: first.global_findings.clone(),
            sources,
        },
    )
    .map_err(|error| MergeError::Unsound {
        because: error.to_string(),
    })
}

fn ordered(parts: &[ShardReport]) -> Result<Vec<&ShardReport>, MergeError> {
    let first = parts.first().ok_or(MergeError::Nothing)?;
    let of = first.shard.of();
    let mut by_index = BTreeMap::new();
    let mut runs = BTreeSet::new();
    for part in parts {
        let shard = part.shard;
        if shard.of() != of {
            return Err(MergeError::Denominator {
                index: shard.index(),
                expected: of,
                actual: shard.of(),
            });
        }
        let run_id = rust_mutants::id::RunId::try_from(part.run_id.as_str()).map_err(|error| {
            MergeError::Unsound {
                because: error.to_string(),
            }
        })?;
        if !runs.insert(run_id.clone()) {
            return Err(MergeError::DuplicateRun { run_id });
        }
        if by_index.insert(shard.index(), part).is_some() {
            return Err(MergeError::DuplicateShard {
                index: shard.index(),
                of,
            });
        }
    }
    for index in 1..=of {
        if !by_index.contains_key(&index) {
            return Err(MergeError::MissingShard { index, of });
        }
    }
    Ok(by_index.into_values().collect())
}

fn agree(parts: &[&ShardReport], first: &ShardReport) -> Result<(), MergeError> {
    for part in parts {
        macro_rules! same {
            ($about:literal, $field:ident) => {
                if first.$field != part.$field {
                    return Err(MergeError::Disagree {
                        about: $about,
                        first: format!("{:?}", first.$field),
                        other: format!("{:?}", part.$field),
                    });
                }
            };
        }
        same!("the run kind", run_kind);
        same!("the contract", contract);
        same!("the producer versions", tool);
        same!("the repository evidence", repository);
        if first.provenance.facts != part.provenance.facts {
            return Err(MergeError::Disagree {
                about: "the evidence provenance",
                first: format!("{:?}", first.provenance.facts),
                other: format!("{:?}", part.provenance.facts),
            });
        }
        same!("the requested scope", scope);
        same!("run-wide findings", global_findings);
        let expected: Vec<_> = first
            .builds
            .iter()
            .map(|build| (&build.name, &build.configuration))
            .collect();
        let actual: Vec<_> = part
            .builds
            .iter()
            .map(|build| (&build.name, &build.configuration))
            .collect();
        if expected != actual {
            return Err(MergeError::Disagree {
                about: "the exact ordered configured-build selections",
                first: format!("{expected:?}"),
                other: format!("{actual:?}"),
            });
        }
    }
    Ok(())
}

fn merge_builds(parts: &[&ShardReport]) -> Result<Vec<BuildEvidence>, MergeError> {
    let first = parts.first().copied().ok_or(MergeError::Nothing)?;
    let first_builds: Vec<_> = first.builds.iter().collect();
    let mut merged = Vec::with_capacity(first_builds.len());
    for (position, seed) in first_builds.into_iter().enumerate() {
        let mut sources = Vec::with_capacity(parts.len());
        for part in parts {
            let builds: Vec<_> = part.builds.iter().collect();
            let Some(build) = builds.get(position) else {
                return Err(MergeError::Disagree {
                    about: "the exact ordered configured-build selections",
                    first: seed.name.as_str().to_owned(),
                    other: "(missing)".to_owned(),
                });
            };
            sources.push(build.source.clone());
        }
        let ledger = PartLedger::checked(sources).map_err(|error| MergeError::Unsound {
            because: format!("configured build {:?}: {error}", seed.name),
        })?;
        merged.push(BuildEvidence::from_parts(
            seed.name.clone(),
            seed.configuration.clone(),
            ledger,
        ));
    }
    Ok(merged)
}
