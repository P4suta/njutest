// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The `verified-v1` survivor phase: a pristine-tree check followed by one fresh dependency-free proof crate per closed harness.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use rust_mutants::catalog::Mutant;

use super::result::{
    ArtifactFailure, Configuration, Decision, ProcessFailure, PropertyStatus, Protocol,
    ToolFailure, Undecided,
};
use super::runner::{Attempt, Invocation};
use super::{Harness, IneligibilityKind};

const ARTIFACT_PREFIX: &str = "model";

/// An internal inconsistency or I/O failure that prevents the phase itself from asking all of its typed questions.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ModelError {
    /// The executable contract lost the bounds established by configuration validation.
    #[error("the verified-v1 configuration is no longer valid: {0}")]
    Configuration(#[from] crate::config::VerificationError),
    /// A survivor was absent from the immutable catalog that produced it.
    #[error("the surviving mutation {mutant} is absent from its own catalog")]
    Catalog { mutant: String },
    /// The session no longer carries the pristine bytes named by a catalog entry.
    #[error("the catalogued source {path} is absent from the prepared session")]
    Source { path: String },
    /// The report artifact directory could not be created.
    #[error("creating the model artifact directory {path}: {source}")]
    ArtifactDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The model artifact directory resolved to a symlink or non-directory.
    #[error("the model artifact directory {path} is not one real directory")]
    ArtifactBoundary { path: PathBuf },
    /// A proof attempt could not reserve a new, empty build directory.
    #[error("creating a fresh model target directory below {path}: {source}")]
    TargetDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// A generated harness or its pristine replacement could not be written.
    #[error("writing the model source {path}: {source}")]
    SourceWrite {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// A supposedly compiler-derived model identity lost a typed invariant.
    #[error(transparent)]
    EvidenceIdentity(#[from] crate::report::ModelInvariantError),
    /// A post-lattice model phase received no configured-build preparation.
    #[error("the completed build lattice has no model preparation")]
    EmptyPreparation,
    /// The preparation ledger does not name exactly the builds the completed lattice retained, in the same order.
    #[error("model preparations name builds {actual:?}, not lattice builds {expected:?}")]
    PreparationBuilds {
        /// The exact ordered build names retained by the lattice.
        expected: Vec<String>,
        /// The ordered names attached to the preparations.
        actual: Vec<String>,
    },
    /// A build carried a preparation for a different assurance contract.
    #[error("configured build {build:?} has no verified-v1 model preparation")]
    PreparationContract { build: String },
    /// A non-model contract carried a verified-only preparation.
    #[error("configured build {build:?} carried model inputs for a non-model contract")]
    UnexpectedPreparation { build: String },
    /// Configured builds were measured for different verifier target triples.
    #[error(
        "configured build {build:?} prepared target {actual:?}, not the common target {expected:?}"
    )]
    PreparationTarget {
        build: String,
        expected: String,
        actual: String,
    },
    /// One build did not retain the immutable source for a final survivor.
    #[error("configured build {build:?} did not prepare final survivor {mutant}")]
    PreparationMissing {
        build: String,
        mutant: rust_mutants::id::MutantId,
    },
    /// A build retained a duplicate preparation for one survivor.
    #[error("configured build {build:?} prepared survivor {mutant} more than once")]
    PreparationDuplicate {
        build: String,
        mutant: rust_mutants::id::MutantId,
    },
    /// Builds disagreed about the catalogued edit or pristine source bytes.
    #[error("configured build {build:?} prepared different bytes for final survivor {mutant}")]
    PreparationMismatch {
        build: String,
        mutant: rust_mutants::id::MutantId,
    },
}

/// One survivor and the immutable bytes from which it was catalogued.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Asked {
    mutant: Mutant,
    source: Vec<u8>,
}

/// What one configured build contributes to the post-lattice model phase.
///
/// The contract is a variant rather than a boolean or optional seed, so a caller cannot accidentally treat a non-model run as an empty verified run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Preparation {
    /// This contract has no model phase.
    NotRequired,
    /// `verified-v1` retained immutable survivor inputs for later lattice reconciliation.
    Verified(Seed),
}

/// Immutable model inputs captured by one configured build before its prepared session is closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Seed {
    target: String,
    tree_written: bool,
    asked: Vec<Asked>,
}

/// The exact, cross-build survivor set that may be proved once under the final report namespace.
#[derive(Debug)]
pub(crate) struct Plan {
    target: String,
    tree_written: bool,
    asked: Vec<Asked>,
}

impl Preparation {
    /// Mints the only preparation state allowed by the selected contract.
    pub(crate) fn for_contract(contract: crate::config::Contract, target: &str) -> Self {
        if contract.proves_models() {
            Self::Verified(Seed {
                target: target.to_owned(),
                tree_written: false,
                asked: Vec::new(),
            })
        } else {
            Self::NotRequired
        }
    }

    /// Replaces the empty verified seed with the survivors and pristine bytes captured by this build.
    /// Non-model contracts remain structurally inert.
    pub(crate) fn capture(
        &mut self,
        session: &rust_mutants::session::Session,
        judged: &[crate::assure::mutation::Judged],
        tree_written: bool,
    ) -> Result<(), ModelError> {
        let Self::Verified(seed) = self else {
            return Ok(());
        };
        seed.tree_written = tree_written;
        seed.asked = asked(session, judged)?;
        Ok(())
    }
}

impl Plan {
    /// Re-derives one canonical proof plan from the final survivor set and every configured build's immutable preparation.
    pub(crate) fn checked(
        candidates: &crate::report::ModelCandidates,
        prepared: Vec<(String, Preparation)>,
    ) -> Result<Self, ModelError> {
        let builds: Vec<_> = candidates
            .builds()
            .map(|build| build.as_str().to_owned())
            .collect();
        let candidates: Vec<_> = candidates.iter().cloned().collect();
        Self::checked_ids(&candidates, &builds, prepared)
    }

    fn checked_ids(
        candidates: &[rust_mutants::id::MutantId],
        builds: &[String],
        prepared: Vec<(String, Preparation)>,
    ) -> Result<Self, ModelError> {
        let actual: Vec<_> = prepared.iter().map(|(name, _)| name.clone()).collect();
        if actual != builds {
            return Err(ModelError::PreparationBuilds {
                expected: builds.to_vec(),
                actual,
            });
        }
        let mut prepared = prepared.into_iter();
        let Some((first_name, first_preparation)) = prepared.next() else {
            return Err(ModelError::EmptyPreparation);
        };
        let Preparation::Verified(first) = first_preparation else {
            return Err(ModelError::PreparationContract { build: first_name });
        };
        let target = first.target.clone();
        let mut tree_written = first.tree_written;
        let mut seeds = vec![(first_name, first)];
        for (build, preparation) in prepared {
            let Preparation::Verified(seed) = preparation else {
                return Err(ModelError::PreparationContract { build });
            };
            if seed.target != target {
                return Err(ModelError::PreparationTarget {
                    build,
                    expected: target,
                    actual: seed.target,
                });
            }
            tree_written |= seed.tree_written;
            seeds.push((build, seed));
        }

        let mut asked = Vec::with_capacity(candidates.len());
        for mutant in candidates {
            let mut agreed: Option<Asked> = None;
            for (build, seed) in &seeds {
                let mut matching = seed
                    .asked
                    .iter()
                    .filter(|question| question.mutant.id == *mutant);
                let Some(question) = matching.next() else {
                    return Err(ModelError::PreparationMissing {
                        build: build.clone(),
                        mutant: mutant.clone(),
                    });
                };
                if matching.next().is_some() {
                    return Err(ModelError::PreparationDuplicate {
                        build: build.clone(),
                        mutant: mutant.clone(),
                    });
                }
                if let Some(expected) = &agreed {
                    if expected != question {
                        return Err(ModelError::PreparationMismatch {
                            build: build.clone(),
                            mutant: mutant.clone(),
                        });
                    }
                } else {
                    agreed = Some(question.clone());
                }
            }
            let Some(question) = agreed else {
                return Err(ModelError::PreparationMissing {
                    build: String::new(),
                    mutant: mutant.clone(),
                });
            };
            asked.push(question);
        }
        Ok(Self {
            target,
            tree_written,
            asked,
        })
    }

    /// Whether the lattice contains no survivor requiring a model record.
    pub(crate) const fn is_empty(&self) -> bool {
        self.asked.is_empty()
    }

    /// Consumes the plan into the only three inputs the isolated proof phase can observe.
    pub(crate) fn into_parts(self) -> (String, bool, Vec<Asked>) {
        (self.target, self.tree_written, self.asked)
    }
}

/// Proves that a non-model contract did not accidentally carry a verified preparation through the build lattice.
pub(crate) fn confirm_not_required(prepared: &[(String, Preparation)]) -> Result<(), ModelError> {
    for (build, preparation) in prepared {
        if matches!(preparation, Preparation::Verified(_)) {
            return Err(ModelError::UnexpectedPreparation {
                build: build.clone(),
            });
        }
    }
    Ok(())
}

/// Everything the independent model phase needs to construct fresh isolated proof crates.
/// Subject package manifests, build scripts, and dependencies are deliberately absent from this type and therefore cannot enter Kani.
#[derive(Debug, Clone)]
pub(crate) struct Proving<'a> {
    pub scratch_dir: &'a Path,
    pub environment: &'a [(std::ffi::OsString, std::ffi::OsString)],
    pub target: &'a str,
    pub verified: crate::config::Verified,
    pub artifact_dir: &'a Path,
    pub tree_written: bool,
}

#[derive(Debug)]
struct Admitted {
    index: usize,
    asked: Asked,
    harness: Harness,
}

#[derive(Debug)]
struct EvidencePaths<'a> {
    result: &'a str,
    source: &'a crate::report::ModelArtifact,
}

struct Admission {
    decided: Vec<(usize, Decided)>,
    admitted: Vec<Admitted>,
}

#[derive(Debug, Clone)]
pub(crate) struct Decided {
    record: crate::report::ModelRecord,
}

impl Decided {
    /// Consumes the internal execution result into its durable final record.
    pub(crate) fn into_record(self) -> crate::report::ModelRecord {
        self.record
    }
}

/// Collects every test survivor without silently dropping a catalog or source inconsistency.
pub(crate) fn asked(
    session: &rust_mutants::session::Session,
    judged: &[crate::assure::mutation::Judged],
) -> Result<Vec<Asked>, ModelError> {
    let catalog: BTreeMap<&str, &Mutant> = session
        .catalog()
        .mutants()
        .iter()
        .map(|mutant| (mutant.id.as_str(), mutant))
        .collect();
    let mut answers = Vec::new();
    for judged in judged {
        if !matches!(
            judged.disposition,
            crate::assure::mutation::Disposition::Survived { .. }
        ) {
            continue;
        }
        let mutant = catalog
            .get(judged.id.as_str())
            .ok_or_else(|| ModelError::Catalog {
                mutant: judged.id.clone(),
            })?;
        let source = session
            .source(&mutant.candidate.path)
            .ok_or_else(|| ModelError::Source {
                path: mutant.candidate.path.clone(),
            })?;
        answers.push(Asked {
            mutant: (*mutant).clone(),
            source: source.to_vec(),
        });
    }
    Ok(answers)
}

/// Generates every closed harness, executes the eligible ones in a clean workspace, and retains one typed record for every survivor offered.
pub(crate) fn prove(
    proving: &Proving<'_>,
    asked: Vec<Asked>,
    watch: crate::watch::Watch<'_>,
) -> Result<Vec<Decided>, crate::error::RunnerError> {
    let Admission {
        mut decided,
        admitted,
    } = admit(proving, asked)?;
    if admitted.is_empty() {
        return Ok(ordered(decided));
    }
    create_private_directory(proving.artifact_dir).map_err(|source| {
        crate::error::RunnerError::from(ModelError::ArtifactDirectory {
            path: proving.artifact_dir.to_path_buf(),
            source,
        })
    })?;
    let artifact_boundary = match std::fs::symlink_metadata(proving.artifact_dir) {
        Ok(metadata) => metadata.file_type().is_dir(),
        Err(_error) => false,
    };
    if !artifact_boundary {
        return Err(crate::error::RunnerError::from(
            ModelError::ArtifactBoundary {
                path: proving.artifact_dir.to_path_buf(),
            },
        ));
    }
    if proving.tree_written {
        for admitted in admitted {
            let index = admitted.index;
            decided.push((index, refuse_written_tree(proving, &admitted, watch)?));
        }
        return Ok(ordered(decided));
    }
    for admitted in admitted {
        if watch.is_cancelled() {
            return Err(crate::error::RunnerError::Interrupted);
        }
        let index = admitted.index;
        decided.push((index, execute(proving, &admitted, watch)?));
    }
    Ok(ordered(decided))
}

fn refuse_written_tree(
    proving: &Proving<'_>,
    admitted: &Admitted,
    watch: crate::watch::Watch<'_>,
) -> Result<Decided, crate::error::RunnerError> {
    let artifact = proving
        .artifact_dir
        .join(format!("{}.json", admitted.asked.mutant.id));
    let source_artifact = proving
        .artifact_dir
        .join(format!("{}.rs", admitted.asked.mutant.id));
    write_new(&source_artifact, admitted.harness.source().as_bytes())?;
    let retained_source = retained_source(admitted)?;
    let artifact_name = format!("{}/{}.json", ARTIFACT_PREFIX, admitted.asked.mutant.id);
    let paths = EvidencePaths {
        result: &artifact_name,
        source: &retained_source,
    };
    let attempt = super::runner::refused_configuration(
        &admitted.harness,
        &artifact,
        Configuration::TreeWritten,
    );
    let record = record_of(
        admitted.asked.mutant.id.to_string(),
        answer(&attempt, &paths)?,
    )?;
    trace(watch.trace, &record, &attempt, &paths);
    Ok(record)
}

fn retained_source(admitted: &Admitted) -> Result<crate::report::ModelArtifact, ModelError> {
    let bytes = u64::try_from(admitted.harness.source().len()).map_err(|_error| {
        ModelError::EvidenceIdentity(crate::report::ModelInvariantError::ArtifactBytes)
    })?;
    crate::report::ModelArtifact::checked(
        &format!("{}/{}.rs", ARTIFACT_PREFIX, admitted.asked.mutant.id),
        bytes,
        admitted.harness.rendered_digest().to_owned(),
    )
    .map_err(ModelError::from)
}

fn admit(proving: &Proving<'_>, asked: Vec<Asked>) -> Result<Admission, ModelError> {
    let mut decided = Vec::new();
    let mut admitted = Vec::new();
    for (index, one) in asked.into_iter().enumerate() {
        match super::generate(&one.source, &one.mutant, proving.verified) {
            Ok(harness) => admitted.push(Admitted {
                index,
                asked: one,
                harness,
            }),
            Err(why) => decided.push((
                index,
                record_of(
                    one.mutant.id.to_string(),
                    crate::report::ModelDecision::Ineligible {
                        reason: ineligibility(why.kind()),
                    },
                )?,
            )),
        }
    }
    Ok(Admission { decided, admitted })
}

fn execute(
    proving: &Proving<'_>,
    admitted: &Admitted,
    watch: crate::watch::Watch<'_>,
) -> Result<Decided, crate::error::RunnerError> {
    let tool_environment = proving.environment;
    let Some(kani) = installed_checker(tool_environment) else {
        let record =
            refuse_configuration(proving, admitted, watch, Configuration::CompilerEnvironment)?;
        return Ok(record);
    };
    let (model_root, source_path) = create_model_crate(
        proving.scratch_dir,
        admitted.asked.mutant.id.as_str(),
        admitted.harness.source().as_bytes(),
    )?;
    let source_artifact = proving
        .artifact_dir
        .join(format!("{}.rs", admitted.asked.mutant.id));
    write_new(&source_artifact, admitted.harness.source().as_bytes())?;
    let artifact = proving
        .artifact_dir
        .join(format!("{}.json", admitted.asked.mutant.id));
    let target = create_private_target(proving.scratch_dir, admitted.asked.mutant.id.as_str())
        .map_err(|source| {
            crate::error::RunnerError::from(ModelError::TargetDirectory {
                path: proving.scratch_dir.to_path_buf(),
                source,
            })
        })?;
    let invocation = Invocation {
        kani: &kani,
        root: &model_root,
        source: &source_path,
        package: super::MODEL_PACKAGE,
        target: proving.target,
        artifact: &artifact,
        target_dir: &target,
        harness: &admitted.harness,
        environment: tool_environment,
        trace: watch.trace,
    };
    let attempt = super::runner::run(&invocation, watch.cancel);
    let artifact_name = format!("{}/{}.json", ARTIFACT_PREFIX, admitted.asked.mutant.id);
    let retained_source = retained_source(admitted)?;
    let paths = EvidencePaths {
        result: &artifact_name,
        source: &retained_source,
    };
    let record = record_of(
        admitted.asked.mutant.id.to_string(),
        answer(&attempt, &paths)?,
    )?;
    trace(watch.trace, &record, &attempt, &paths);
    Ok(record)
}

fn installed_checker(environment: &[(std::ffi::OsString, std::ffi::OsString)]) -> Option<PathBuf> {
    let cargo_home = environment
        .iter()
        .find(|(name, _value)| {
            name.to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case("CARGO_HOME"))
        })
        .map(|(_name, value)| PathBuf::from(value))?;
    if !cargo_home.is_absolute() {
        return None;
    }
    let executable = if cfg!(windows) {
        "cargo-kani.exe"
    } else {
        "cargo-kani"
    };
    Some(cargo_home.join("bin").join(executable))
}

fn refuse_configuration(
    proving: &Proving<'_>,
    admitted: &Admitted,
    watch: crate::watch::Watch<'_>,
    why: Configuration,
) -> Result<Decided, crate::error::RunnerError> {
    let artifact = proving
        .artifact_dir
        .join(format!("{}.json", admitted.asked.mutant.id));
    let source_artifact = proving
        .artifact_dir
        .join(format!("{}.rs", admitted.asked.mutant.id));
    write_new(&source_artifact, admitted.harness.source().as_bytes())?;
    let retained_source = retained_source(admitted)?;
    let artifact_name = format!("{}/{}.json", ARTIFACT_PREFIX, admitted.asked.mutant.id);
    let paths = EvidencePaths {
        result: &artifact_name,
        source: &retained_source,
    };
    let attempt = super::runner::refused_configuration(&admitted.harness, &artifact, why);
    let record = record_of(
        admitted.asked.mutant.id.to_string(),
        answer(&attempt, &paths)?,
    )?;
    trace(watch.trace, &record, &attempt, &paths);
    Ok(record)
}

fn overwrite_regular_handle(file: &mut std::fs::File, bytes: &[u8]) -> std::io::Result<()> {
    let before = file.metadata()?;
    if !before.file_type().is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "workspace source is not a regular file",
        ));
    }
    file.set_len(0)?;
    file.write_all(bytes)?;
    file.sync_data()?;
    let after = file.metadata()?;
    let expected = u64::try_from(bytes.len()).map_err(|_overflow| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "workspace source is too large",
        )
    })?;
    if !after.file_type().is_file() || after.len() != expected {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "workspace source changed while it was being written",
        ));
    }
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), crate::error::RunnerError> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| {
            crate::error::RunnerError::from(ModelError::SourceWrite {
                path: path.to_path_buf(),
                source,
            })
        })?;
    overwrite_regular_handle(&mut file, bytes).map_err(|source| {
        crate::error::RunnerError::from(ModelError::SourceWrite {
            path: path.to_path_buf(),
            source,
        })
    })
}

#[cfg(unix)]
fn create_private_directory(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt as _;

    let mut builder = std::fs::DirBuilder::new();
    builder.mode(0o700).create(path)
}

fn create_private_target(parent: &Path, mutant: &str) -> std::io::Result<PathBuf> {
    let metadata = std::fs::symlink_metadata(parent)?;
    if !metadata.file_type().is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the model target parent is not one real directory",
        ));
    }
    for sequence in 0_u16..=u16::MAX {
        let candidate = parent.join(format!("njutest-kani-{mutant}-{sequence}"));
        match create_private_directory(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "all model target directory names are occupied",
    ))
}

fn create_model_crate(
    parent: &Path,
    mutant: &str,
    source: &[u8],
) -> Result<(PathBuf, PathBuf), crate::error::RunnerError> {
    let root = create_private_target(parent, &format!("crate-{mutant}")).map_err(|source| {
        crate::error::RunnerError::from(ModelError::TargetDirectory {
            path: parent.to_path_buf(),
            source,
        })
    })?;
    let source_directory = root.join("src");
    create_private_directory(&source_directory).map_err(|source| {
        crate::error::RunnerError::from(ModelError::TargetDirectory {
            path: source_directory.clone(),
            source,
        })
    })?;
    write_new(&root.join("Cargo.toml"), super::MODEL_MANIFEST.as_bytes())?;
    write_new(&root.join("Cargo.lock"), super::MODEL_LOCK.as_bytes())?;
    let source_path = root.join(super::MODEL_SOURCE_PATH);
    write_new(&source_path, source)?;
    Ok((root, source_path))
}

#[cfg(not(unix))]
#[expect(
    clippy::create_dir,
    reason = "the unix builder beside this one also creates exactly one directory and refuses an \
              existing one, and create_dir_all would accept both"
)]
fn create_private_directory(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir(path)
}

fn ordered(mut answers: Vec<(usize, Decided)>) -> Vec<Decided> {
    answers.sort_by_key(|(index, _answer)| *index);
    answers.into_iter().map(|(_index, answer)| answer).collect()
}

fn trace(
    trace: &crate::trace::Recorder,
    decided: &Decided,
    attempt: &Attempt,
    paths: &EvidencePaths<'_>,
) {
    if let Some(bytes) = attempt.artifact().bytes() {
        trace.artifact(crate::trace::ArtifactRecord {
            kind: "model-checker-result".to_owned(),
            path: paths.result.to_owned(),
            bytes: Some(bytes),
        });
    }
    trace.artifact(crate::trace::ArtifactRecord {
        kind: "model-checker-source".to_owned(),
        path: paths.source.path().to_owned(),
        bytes: Some(paths.source.bytes()),
    });
    trace.model(decided.record.clone());
}

fn record_of(mutant: String, answer: crate::report::ModelDecision) -> Result<Decided, ModelError> {
    let record = crate::report::ModelRecord::checked(mutant, answer).map_err(ModelError::from)?;
    Ok(Decided { record })
}

fn answer(
    attempt: &Attempt,
    paths: &EvidencePaths<'_>,
) -> Result<crate::report::ModelDecision, ModelError> {
    match attempt.parsed().decision {
        Decision::Proved => affirmative(attempt, paths, true),
        Decision::Noticed => affirmative(attempt, paths, false),
        Decision::Undecided(ref why) => undecided(attempt, paths, uncertainty(why)),
    }
}

fn affirmative(
    attempt: &Attempt,
    paths: &EvidencePaths<'_>,
    proved: bool,
) -> Result<crate::report::ModelDecision, ModelError> {
    let Some(parsed_verifier) = attempt.parsed().evidence.verifier.as_ref() else {
        return contradictory(attempt, paths);
    };
    let Some(raw) = artifact_of(attempt, paths.result)? else {
        return contradictory(attempt, paths);
    };
    let verifier = verifier_of(&parsed_verifier.tool, &parsed_verifier.backend)?;
    let process = process_fact(attempt.process());
    if !matches!(
        (proved, process),
        (true, crate::report::ModelProcess::Exited(0))
            | (false, crate::report::ModelProcess::Exited(1))
    ) {
        return contradictory(attempt, paths);
    }
    if proved {
        let evidence = crate::report::ModelEvidence::checked(crate::report::ModelEvidenceInput {
            verifier,
            identity: identity(attempt)?,
            artifact: raw,
            source: paths.source.clone(),
            process: crate::report::ModelProvedExit,
        })
        .map_err(ModelError::from)?;
        Ok(crate::report::ModelDecision::Proved { evidence })
    } else {
        let evidence = crate::report::ModelEvidence::checked(crate::report::ModelEvidenceInput {
            verifier,
            identity: identity(attempt)?,
            artifact: raw,
            source: paths.source.clone(),
            process: crate::report::ModelNoticedExit,
        })
        .map_err(ModelError::from)?;
        Ok(crate::report::ModelDecision::Noticed { evidence })
    }
}

fn contradictory(
    attempt: &Attempt,
    paths: &EvidencePaths<'_>,
) -> Result<crate::report::ModelDecision, ModelError> {
    undecided(
        attempt,
        paths,
        crate::report::ModelUncertainty::Protocol(crate::report::ModelProtocol::Contradiction),
    )
}

fn undecided(
    attempt: &Attempt,
    paths: &EvidencePaths<'_>,
    reason: crate::report::ModelUncertainty,
) -> Result<crate::report::ModelDecision, ModelError> {
    let evidence = attempt_evidence(attempt, paths)?;
    let attempt =
        crate::report::ModelAttempt::checked(reason, evidence).map_err(ModelError::from)?;
    Ok(crate::report::ModelDecision::Undecided { attempt })
}

fn attempt_evidence(
    attempt: &Attempt,
    paths: &EvidencePaths<'_>,
) -> Result<crate::report::ModelAttemptEvidence, ModelError> {
    let verifier = attempt
        .parsed()
        .evidence
        .verifier
        .as_ref()
        .map(|identity| verifier_of(&identity.tool, &identity.backend))
        .transpose()?;
    crate::report::ModelAttemptEvidence::checked(crate::report::ModelAttemptEvidenceInput {
        verifier,
        identity: identity(attempt)?,
        artifact: artifact_of(attempt, paths.result)?,
        source: paths.source.clone(),
        process: process_fact(attempt.process()),
        raw_sha256: attempt.parsed().evidence.raw_digest.clone(),
    })
    .map_err(ModelError::from)
}

fn verifier_of(
    tool: &str,
    backend: &super::result::KaniBackend,
) -> Result<crate::report::ModelVerifier, ModelError> {
    crate::report::ModelVerifier::checked(crate::report::ModelVerifierInput {
        tool: tool.to_owned(),
        export_version: backend.export_version.clone(),
        build_mode: backend.build_mode.clone(),
        target: backend.target.clone(),
        rustc: backend.rustc.clone(),
        cbmc: backend.cbmc.clone(),
        goto_cc: backend.goto_cc.clone(),
        goto_instrument: backend.goto_instrument.clone(),
        solver: backend.solver.clone(),
    })
    .map_err(ModelError::from)
}

const fn process_fact(process: super::runner::Process) -> crate::report::ModelProcess {
    match process {
        super::runner::Process::NotRun => crate::report::ModelProcess::NotRun,
        super::runner::Process::Exited(code) => crate::report::ModelProcess::Exited(code),
        super::runner::Process::Cutoff => crate::report::ModelProcess::Cutoff,
        super::runner::Process::Cancelled => crate::report::ModelProcess::Cancelled,
        super::runner::Process::Failed => crate::report::ModelProcess::Failed,
    }
}

fn artifact_of(
    attempt: &Attempt,
    path: &str,
) -> Result<Option<crate::report::ModelArtifact>, ModelError> {
    let Some(bytes) = attempt.artifact().bytes() else {
        return Ok(None);
    };
    crate::report::ModelArtifact::checked(path, bytes, attempt.parsed().evidence.raw_digest.clone())
        .map(Some)
        .map_err(ModelError::from)
}

fn identity(attempt: &Attempt) -> Result<crate::report::ModelIdentity, ModelError> {
    let evidence = &attempt.parsed().evidence;
    crate::report::ModelIdentity::checked(crate::report::ModelIdentityInput {
        harness: evidence.harness.clone(),
        assertion: evidence.assertion.clone(),
        unwind: evidence.unwind,
        timeout_ms: evidence.timeout_ms,
        source_sha256: evidence.source_digest.clone(),
        rendered_sha256: evidence.rendered_digest.clone(),
        crate_sha256: evidence.crate_digest.clone(),
        mutant: evidence.mutant_id.clone(),
        path: evidence.path.clone(),
        rule: evidence.rule.clone(),
        rule_version: evidence.rule_version,
        start_byte: evidence.start_byte,
        end_byte: evidence.end_byte,
        original_hex: evidence.original_hex.clone(),
        replacement_hex: evidence.replacement_hex.clone(),
    })
    .map_err(ModelError::from)
}

const fn ineligibility(why: IneligibilityKind) -> crate::report::ModelIneligibility {
    match why {
        IneligibilityKind::SourceEncoding => crate::report::ModelIneligibility::SourceEncoding,
        IneligibilityKind::SourceDigest => crate::report::ModelIneligibility::SourceDigest,
        IneligibilityKind::Candidate => crate::report::ModelIneligibility::Candidate,
        IneligibilityKind::Identity => crate::report::ModelIneligibility::Identity,
        IneligibilityKind::SourceSyntax => crate::report::ModelIneligibility::SourceSyntax,
        IneligibilityKind::EnclosingFunction => {
            crate::report::ModelIneligibility::EnclosingFunction
        }
        IneligibilityKind::FunctionShape => crate::report::ModelIneligibility::FunctionShape,
        IneligibilityKind::NoSymbolicInput => crate::report::ModelIneligibility::NoSymbolicInput,
        IneligibilityKind::ArgumentPattern => crate::report::ModelIneligibility::ArgumentPattern,
        IneligibilityKind::InputType => crate::report::ModelIneligibility::InputType,
        IneligibilityKind::OutputType => crate::report::ModelIneligibility::OutputType,
        IneligibilityKind::Effect => crate::report::ModelIneligibility::Effect,
        IneligibilityKind::MutantSyntax => crate::report::ModelIneligibility::MutantSyntax,
        IneligibilityKind::NameCollision => crate::report::ModelIneligibility::NameCollision,
        IneligibilityKind::SourceSpan => crate::report::ModelIneligibility::SourceSpan,
    }
}

fn uncertainty(why: &Undecided) -> crate::report::ModelUncertainty {
    match why {
        Undecided::BoundExhausted => crate::report::ModelUncertainty::BoundExhausted,
        Undecided::Cutoff => crate::report::ModelUncertainty::Cutoff,
        Undecided::Cancelled => crate::report::ModelUncertainty::Cancelled,
        Undecided::Configuration(why) => {
            crate::report::ModelUncertainty::Configuration(configuration(*why))
        }
        Undecided::Tool(why) => crate::report::ModelUncertainty::Tool(tool(*why)),
        Undecided::Process(why) => crate::report::ModelUncertainty::Process(process(*why)),
        Undecided::Artifact(why) => crate::report::ModelUncertainty::Artifact(artifact(*why)),
        Undecided::ExitMismatch { decision, actual } => {
            crate::report::ModelUncertainty::ExitMismatch {
                expected: match decision {
                    super::result::Affirmative::Proved => crate::report::ModelAffirmative::Proved,
                    super::result::Affirmative::Noticed => crate::report::ModelAffirmative::Noticed,
                },
                actual: *actual,
            }
        }
        Undecided::Protocol(why) => crate::report::ModelUncertainty::Protocol(protocol(*why)),
        Undecided::Property { status } => {
            crate::report::ModelUncertainty::Property(property(*status))
        }
        Undecided::OtherFailure { category } => {
            crate::report::ModelUncertainty::OtherFailure(category.clone())
        }
    }
}

const fn configuration(why: Configuration) -> crate::report::ModelConfiguration {
    match why {
        Configuration::Package => crate::report::ModelConfiguration::Package,
        Configuration::Profile => crate::report::ModelConfiguration::Profile,
        Configuration::CompilerFlags => crate::report::ModelConfiguration::CompilerFlags,
        Configuration::CompilerEnvironment => {
            crate::report::ModelConfiguration::CompilerEnvironment
        }
        Configuration::RelativePath => crate::report::ModelConfiguration::RelativePath,
        Configuration::Directory => crate::report::ModelConfiguration::Directory,
        Configuration::TreeWritten => crate::report::ModelConfiguration::TreeWritten,
        Configuration::WorkspaceDrift => crate::report::ModelConfiguration::WorkspaceDrift,
    }
}

const fn tool(why: ToolFailure) -> crate::report::ModelToolFailure {
    match why {
        ToolFailure::Unavailable => crate::report::ModelToolFailure::Unavailable,
        ToolFailure::VersionCommand => crate::report::ModelToolFailure::VersionCommand,
        ToolFailure::VersionBanner => crate::report::ModelToolFailure::VersionBanner,
        ToolFailure::HarnessListCommand => crate::report::ModelToolFailure::HarnessListCommand,
        ToolFailure::HarnessListArtifact => crate::report::ModelToolFailure::HarnessListArtifact,
        ToolFailure::HarnessListSchema => crate::report::ModelToolFailure::HarnessListSchema,
        ToolFailure::HarnessListMatch => crate::report::ModelToolFailure::HarnessListMatch,
    }
}

const fn process(why: ProcessFailure) -> crate::report::ModelProcessFailure {
    match why {
        ProcessFailure::NotStarted => crate::report::ModelProcessFailure::NotStarted,
        ProcessFailure::Stopped => crate::report::ModelProcessFailure::Stopped,
        ProcessFailure::Monitor => crate::report::ModelProcessFailure::Monitor,
        ProcessFailure::Wait => crate::report::ModelProcessFailure::Wait,
        ProcessFailure::Signal => crate::report::ModelProcessFailure::Signal,
        ProcessFailure::UnknownExit => crate::report::ModelProcessFailure::UnknownExit,
        ProcessFailure::UnexpectedExit => crate::report::ModelProcessFailure::UnexpectedExit,
    }
}

const fn artifact(why: ArtifactFailure) -> crate::report::ModelArtifactFailure {
    match why {
        ArtifactFailure::AlreadyExists => crate::report::ModelArtifactFailure::AlreadyExists,
        ArtifactFailure::Missing => crate::report::ModelArtifactFailure::Missing,
        ArtifactFailure::NotFile => crate::report::ModelArtifactFailure::NotFile,
        ArtifactFailure::TooLarge => crate::report::ModelArtifactFailure::TooLarge,
        ArtifactFailure::Unreadable => crate::report::ModelArtifactFailure::Unreadable,
        ArtifactFailure::SourceChanged => crate::report::ModelArtifactFailure::SourceChanged,
    }
}

const fn protocol(why: Protocol) -> crate::report::ModelProtocol {
    match why {
        Protocol::Schema => crate::report::ModelProtocol::Schema,
        Protocol::ToolVersion => crate::report::ModelProtocol::ToolVersion,
        Protocol::ExportVersion => crate::report::ModelProtocol::ExportVersion,
        Protocol::Backend => crate::report::ModelProtocol::Backend,
        Protocol::Summary => crate::report::ModelProtocol::Summary,
        Protocol::Harness => crate::report::ModelProtocol::Harness,
        Protocol::Assertion => crate::report::ModelProtocol::Assertion,
        Protocol::Contradiction => crate::report::ModelProtocol::Contradiction,
    }
}

const fn property(status: PropertyStatus) -> crate::report::ModelPropertyStatus {
    match status {
        PropertyStatus::Failure => crate::report::ModelPropertyStatus::Failure,
        PropertyStatus::Covered => crate::report::ModelPropertyStatus::Covered,
        PropertyStatus::Satisfied => crate::report::ModelPropertyStatus::Satisfied,
        PropertyStatus::Success => crate::report::ModelPropertyStatus::Success,
        PropertyStatus::Undetermined => crate::report::ModelPropertyStatus::Undetermined,
        PropertyStatus::Unknown => crate::report::ModelPropertyStatus::Unknown,
        PropertyStatus::Unreachable => crate::report::ModelPropertyStatus::Unreachable,
        PropertyStatus::Uncovered => crate::report::ModelPropertyStatus::Uncovered,
        PropertyStatus::Unsatisfiable => crate::report::ModelPropertyStatus::Unsatisfiable,
        PropertyStatus::Error => crate::report::ModelPropertyStatus::Error,
    }
}

#[cfg(test)]
mod tests {

    use rust_mutants::catalog::{Candidate, Mutant};
    use rust_mutants::rule::Registry;
    use rust_mutants::span::Span;

    use super::{Asked, ModelError, Plan, Preparation, Seed};

    fn question() -> Asked {
        let source = b"pub fn selected(x: i32) -> bool { x > 0 }\n";
        let start = source
            .iter()
            .position(|byte| *byte == b'>')
            .expect("the fixture contains its mutation token");
        let end = start.checked_add(1).expect("the fixture span is tiny");
        let rule = Registry::canonical()
            .lookup("gt-to-ge")
            .expect("the fixture uses a canonical rule");
        let candidate = Candidate {
            path: "src/lib.rs".to_owned(),
            rule,
            span: Span::new(
                u32::try_from(start).expect("the fixture offset fits the wire"),
                u32::try_from(end).expect("the fixture offset fits the wire"),
            )
            .expect("the fixture span is ordered"),
            original: b">".to_vec(),
            replacement: b">=".to_vec(),
            source_digest: rust_mutants::id::digest(source),
        };
        let id = candidate.id().expect("the fixture candidate is coherent");
        Asked {
            mutant: Mutant {
                index: 0,
                display_id: id.display(),
                id,
                candidate,
            },
            source: source.to_vec(),
        }
    }

    fn seed(target: &str, tree_written: bool, asked: Vec<Asked>) -> Preparation {
        Preparation::Verified(Seed {
            target: target.to_owned(),
            tree_written,
            asked,
        })
    }

    fn builds(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn post_lattice_plan_requires_every_build_to_rederive_the_same_survivor() {
        let survivor = question();
        let candidates = vec![survivor.mutant.id.clone()];
        let plan = Plan::checked_ids(
            &candidates,
            &builds(&["default", "release"]),
            vec![
                (
                    "default".to_owned(),
                    seed("host", false, vec![survivor.clone()]),
                ),
                (
                    "release".to_owned(),
                    seed("host", true, vec![survivor.clone()]),
                ),
            ],
        )
        .expect("the builds agree on the final survivor");
        let (target, tree_written, asked) = plan.into_parts();
        assert_eq!(target, "host");
        assert!(
            tree_written,
            "one tree write must downgrade the final proof"
        );
        assert_eq!(asked, vec![survivor]);
    }

    #[test]
    fn post_lattice_plan_rejects_missing_mismatched_and_duplicate_inputs() {
        let survivor = question();
        let candidates = vec![survivor.mutant.id.clone()];
        assert!(matches!(
            Plan::checked_ids(
                &candidates,
                &builds(&["default"]),
                vec![("default".to_owned(), seed("host", false, Vec::new()))],
            ),
            Err(ModelError::PreparationMissing { .. })
        ));

        let mut changed = survivor.clone();
        changed.source.push(b' ');
        assert!(matches!(
            Plan::checked_ids(
                &candidates,
                &builds(&["default", "release"]),
                vec![
                    (
                        "default".to_owned(),
                        seed("host", false, vec![survivor.clone()])
                    ),
                    ("release".to_owned(), seed("host", false, vec![changed])),
                ],
            ),
            Err(ModelError::PreparationMismatch { .. })
        ));

        assert!(matches!(
            Plan::checked_ids(
                &candidates,
                &builds(&["default"]),
                vec![(
                    "default".to_owned(),
                    seed("host", false, vec![survivor.clone(), survivor]),
                )],
            ),
            Err(ModelError::PreparationDuplicate { .. })
        ));
    }

    #[test]
    fn post_lattice_plan_rejects_cross_target_or_contract_confusion() {
        let survivor = question();
        let candidates = vec![survivor.mutant.id.clone()];
        assert!(matches!(
            Plan::checked_ids(
                &candidates,
                &builds(&["default", "other"]),
                vec![
                    (
                        "default".to_owned(),
                        seed("host-a", false, vec![survivor.clone()])
                    ),
                    (
                        "other".to_owned(),
                        seed("host-b", false, vec![survivor.clone()])
                    ),
                ],
            ),
            Err(ModelError::PreparationTarget { .. })
        ));
        assert!(matches!(
            Plan::checked_ids(
                &candidates,
                &builds(&["default", "other"]),
                vec![
                    ("default".to_owned(), seed("host-a", false, vec![survivor])),
                    ("other".to_owned(), Preparation::NotRequired),
                ],
            ),
            Err(ModelError::PreparationContract { .. })
        ));
    }

    #[test]
    fn post_lattice_plan_rejects_missing_substituted_and_reordered_builds() {
        let survivor = question();
        let candidates = vec![survivor.mutant.id.clone()];
        for prepared in [
            vec![(
                "default".to_owned(),
                seed("host", false, vec![survivor.clone()]),
            )],
            vec![
                (
                    "default".to_owned(),
                    seed("host", false, vec![survivor.clone()]),
                ),
                (
                    "debug".to_owned(),
                    seed("host", false, vec![survivor.clone()]),
                ),
            ],
            vec![
                (
                    "release".to_owned(),
                    seed("host", false, vec![survivor.clone()]),
                ),
                ("default".to_owned(), seed("host", false, vec![survivor])),
            ],
        ] {
            assert!(matches!(
                Plan::checked_ids(&candidates, &builds(&["default", "release"]), prepared,),
                Err(ModelError::PreparationBuilds { .. })
            ));
        }
    }
}
