// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The one run of every target with nothing active: the baseline check, and the measurement that rides on it.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest as _, Sha256};

use super::Failing;
use super::prepare::Building;
use crate::EngineError;
use crate::catalog::Catalog;
use crate::execute::{self, Context, ExecRequest, MutantResult, Reading, TargetKind, TestTarget};
use crate::workspace::{SessionError, Workspace};

fn duration_millis(duration: Duration) -> Result<u64, SessionError> {
    u64::try_from(duration.as_millis())
        .map_err(|_outside_range| SessionError::DurationMillisOverflow { duration })
}

fn trace_count(subject: &'static str, count: usize) -> Result<u32, SessionError> {
    u32::try_from(count)
        .map_err(|_outside_range| SessionError::TraceCountTooLarge { subject, count })
}

fn baseline_count(quantity: BaselineQuantity, count: usize) -> Result<u64, BaselineCacheError> {
    u64::try_from(count)
        .map_err(|_outside_range| BaselineCacheError::CountOutsideRange { quantity, count })
}

/// Runs every target once with nothing active.
/// A tree whose instrumented baseline fails is one whose every later result would be about the instrumentation rather than about a mutant.
pub(super) fn verify(
    workspace: &Workspace,
    targets: &mut [TestTarget],
    scratch: &Path,
    building: &Building<'_>,
) -> Result<Verified, EngineError> {
    let Building { catalog, .. } = *building;
    let phase = workspace.trace.phase("verify");
    let logs = scratch.join("touch");
    std::fs::create_dir_all(&logs).map_err(|source| SessionError::WriteFailed {
        path: logs.display().to_string(),
        source,
    })?;
    let remembering = match Remembering::of(targets, scratch, building) {
        Ok(remembering) => remembering,
        Err(why) => {
            workspace
                .trace
                .note(BASELINE_NOT_REMEMBERED, &why.to_string());
            None
        }
    };
    if !building.cancel.is_cancelled()
        && let Some(remembering) = remembering.as_ref()
        && let Some(recalled) = remembering.read(targets, catalog)?
    {
        workspace.trace.note(
            BASELINE_REMEMBERED,
            "the directly built executables are byte-identical and every other baseline input matches the passing measurement already made",
        );
        let replayed = replay(
            &recalled.verified,
            &recalled.tests_run,
            targets,
            &workspace.trace,
        );
        phase.end();
        replayed?;
        return Ok(recalled.verified);
    }
    let measured = verify_targets(targets, scratch, building);
    phase.end();
    let (verified, tests_run) = measured?;
    refused(&verified, building.options.failing)?;
    if verified.failing().is_empty()
        && !building.cancel.is_cancelled()
        && let Some(remembering) = remembering
    {
        match workspace.snapshot.redigest() {
            Ok(drift) if drift.is_empty() => {
                if let Err(why) = remembering.write(&verified, &tests_run, targets) {
                    workspace
                        .trace
                        .note(BASELINE_NOT_REMEMBERED, &why.to_string());
                }
            }
            Ok(drift) => workspace.trace.note(
                BASELINE_NOT_REMEMBERED,
                &format!(
                    "the baseline changed the copied tree at {}; replaying its result would not replay what it wrote",
                    drift
                        .iter()
                        .take(8)
                        .map(|one| format!("{}:{}", one.kind().name(), one.rel_path()))
                        .collect::<Vec<String>>()
                        .join(", ")
                ),
            ),
            Err(error) => workspace.trace.note(
                BASELINE_NOT_REMEMBERED,
                &format!("the copied tree could not be checked for baseline writes: {error}"),
            ),
        }
    }
    Ok(verified)
}

fn verify_targets(
    targets: &mut [TestTarget],
    scratch: &Path,
    building: &Building<'_>,
) -> Result<(Verified, BTreeMap<String, Option<u32>>), EngineError> {
    let mut verified = Verified::default();
    let mut tests_run = BTreeMap::new();
    for (index, target) in targets.iter_mut().enumerate() {
        let (baseline, observed) =
            verify_target((target, index), scratch, building, &mut verified.touched)?;
        if tests_run.insert(target.id.clone(), observed).is_some()
            || verified
                .targets
                .insert(target.id.clone(), Measured::of(baseline))
                .is_some()
        {
            return Err(SessionError::DuplicateBaselineTarget {
                target: target.id.clone(),
            }
            .into());
        }
    }
    Ok((verified, tests_run))
}

fn verify_target(
    (target, index): (&mut TestTarget, usize),
    scratch: &Path,
    building: &Building<'_>,
    touched: &mut crate::touch::Touched,
) -> Result<(Baseline, Option<u32>), EngineError> {
    let recording = (building.asked && recordable(target)).then(|| {
        scratch
            .join("touch")
            .join(format!("{}.log", slug(&target.id)))
    });
    let own = baseline_scratch(scratch, index);
    let mut result = ran(target, &own, recording.as_deref(), building);
    let mut retried = false;
    let recording = if result.exit_code == crate::instrument::TOUCH_UNAVAILABLE_EXIT {
        building.trace.note(
            crate::touch::UNRECORDED,
            &format!(
                "{}: the process could not write what its guards reached, so it is run \
                 again with nothing to record and every test of it stays in every route",
                target.id
            ),
        );
        result = ran(target, &own, None, building);
        None
    } else {
        recording
    };
    if let Some(again) = again(&result, target, (&own, recording.as_deref()), building) {
        result = again;
        retried = true;
    }
    building.trace.verify(crate::trace::VerifyRecord {
        target: target.id.clone(),
        outcome: result.outcome().name().to_owned(),
        tests_run: result.tests_run(),
        duration_ms: duration_millis(result.duration)?,
        remembered: false,
        retried,
    });
    let baseline = baseline_of(&result)?;
    if target.kind == TargetKind::Doc && result.tests_run() == Some(0) {
        target
            .limitations
            .push(crate::limitation::DOCTESTS_NONE.to_owned());
    }
    if baseline.passed() && retried {
        touched.limited(crate::limitation::BASELINE_PASSED_ON_RETRY, &target.id);
    }
    if baseline.passed() && result.reading() == Reading::Short {
        touched.limited(crate::limitation::BASELINE_PASSED_UNPARSED, &target.id);
    }
    if baseline.passed() {
        gather(
            touched,
            &Recording {
                target: &target.id,
                log: recording.as_deref(),
                catalog: building.catalog,
                ran: &result.passed_tests,
                summarised: crate::trace::SummaryRecord::of(&result),
            },
            building.trace,
        )?;
    } else {
        touched.limited(crate::limitation::BASELINE_NOT_PASSING, &target.id);
    }
    Ok((baseline, result.tests_run()))
}

/// What one baseline process came to, keeping what it printed only where it did not pass.
fn baseline_of(result: &MutantResult) -> Result<Baseline, SessionError> {
    Ok(Baseline {
        outcome: result.outcome(),
        duration: result.duration,
        tests: match result.tests_run() {
            Some(tests) => tests,
            None => trace_count("passed baseline tests", result.passed_tests.len())?,
        },
        ignored: trace_count("ignored baseline tests", result.ignored_tests.len())?,
        output: if matches!(
            result.outcome(),
            crate::outcome::Outcome::Survived | crate::outcome::Outcome::Inconclusive
        ) {
            String::new()
        } else {
            match std::str::from_utf8(&result.output) {
                Ok(output) => output.to_owned(),
                Err(_not_utf8) => crate::telling::LosslessBytes::new(&result.output).to_string(),
            }
        },
    })
}

/// The trace note proving why no baseline process follows it.
const BASELINE_REMEMBERED: &str = "baseline-remembered";

/// The trace note saying a target was run a second time, and why.
const BASELINE_RETRIED: &str = "baseline-retried";

/// The temporary directory of the baseline of the target at `index`, its own as every execution's is, so a baseline and a control are measured under the same conditions (ADR 0025).
fn baseline_scratch(scratch: &Path, index: usize) -> PathBuf {
    scratch.join(format!("baseline-{index}"))
}

/// One more run of a target that did not pass, or nothing when the first answer stands.
fn again(
    result: &MutantResult,
    target: &TestTarget,
    (scratch, recording): (&Path, Option<&Path>),
    building: &Building<'_>,
) -> Option<MutantResult> {
    if passing(result.outcome()) || building.cancel.is_cancelled() {
        return None;
    }
    building.trace.note(
        BASELINE_RETRIED,
        &format!(
            "{}: the target did not pass with nothing active, so it is run once more before \
             the session refuses: a first answer something outside the code decided is not \
             one to end a run on",
            target.id
        ),
    );
    Some(ran(target, scratch, recording, building))
}

/// Why a passing baseline could not safely become an answer for another run.
const BASELINE_NOT_REMEMBERED: &str = "baseline-not-remembered";

/// The recipe of a remembered baseline.
/// The engine version is also in every key; this number makes a semantic invalidation explicit within one build.
const BASELINE_ABI: u32 = 1;

/// The on-disk shape of one passing baseline.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Remembered {
    abi: u32,
    key: String,
    answer: String,
    artifacts: BTreeMap<String, String>,
    targets: BTreeMap<String, RememberedBaseline>,
    touched: crate::touch::Touched,
}

/// The part of a passing baseline needed after its process has gone.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RememberedBaseline {
    outcome: String,
    duration_nanos: u64,
    tests: u32,
    ignored: u32,
    tests_run: Option<u32>,
}

/// Where the passing answer to this exact baseline may be found.
#[derive(Debug)]
struct Remembering {
    directory: PathBuf,
    key: String,
    artifacts: BTreeMap<String, String>,
}

/// A remembered verification and the optional summary count needed to replay its trace without turning harness silence into a reported zero.
struct Recalled {
    verified: Verified,
    tests_run: BTreeMap<String, Option<u32>>,
}

#[derive(Clone, Copy)]
struct RecallPremise<'a> {
    targets: &'a [TestTarget],
    catalog: &'a Catalog,
    path: &'a Path,
}

/// A baseline-cache count whose exact spelling enters the retained identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaselineQuantity {
    /// Accepted catalog indices.
    AcceptedMutants,
    /// Compared catalog indices.
    ComparedMutants,
    /// Body-marker pairs.
    BodyMarkers,
    /// Test targets.
    Targets,
    /// Command-line arguments.
    Arguments,
    /// Environment entries.
    Environment,
    /// Values in a length-prefixed key field.
    KeyValues,
}

impl std::fmt::Display for BaselineQuantity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::AcceptedMutants => "accepted-mutant count",
            Self::ComparedMutants => "compared-mutant count",
            Self::BodyMarkers => "body-marker count",
            Self::Targets => "target count",
            Self::Arguments => "argument count",
            Self::Environment => "environment-entry count",
            Self::KeyValues => "key field length",
        })
    }
}

/// Why a passing baseline could not be remembered without changing the run's answer.
#[derive(Debug, thiserror::Error)]
pub enum BaselineCacheError {
    /// One cache-key collection length does not fit its canonical prefix.
    #[error("{quantity} {count} does not fit the baseline-cache identity")]
    CountOutsideRange {
        /// The collection being represented.
        quantity: BaselineQuantity,
        /// The exact host count.
        count: usize,
    },
    /// A measured duration does not fit the cache document's nanosecond field.
    #[error("baseline duration {duration:?} does not fit the cache document")]
    DurationNanosOutsideRange {
        /// The exact measured duration.
        duration: Duration,
    },
    /// Two rows claimed the same target identity in one closed cache document.
    #[error("passing baseline cache records target {target:?} more than once")]
    DuplicateTarget {
        /// The duplicated target identity.
        target: String,
    },
    /// The exact environment a baseline target would observe could not be derived.
    #[error("cannot derive the baseline environment for {target}: {source}")]
    EnvironmentUnavailable {
        /// The target whose process environment was being bound into the key.
        target: String,
        /// The toolchain inspection failure.
        #[source]
        source: std::io::Error,
    },
    /// The cache document could not be read.
    #[error("cannot read passing baseline cache {}: {source}", path.display())]
    ReadFailed {
        /// The cache document.
        path: PathBuf,
        /// The filesystem failure.
        #[source]
        source: std::io::Error,
    },
    /// The cache bytes are not this ABI's closed document.
    #[error("cannot parse passing baseline cache {}: {source}", path.display())]
    UnreadableDocument {
        /// The cache document.
        path: PathBuf,
        /// The JSON failure.
        #[source]
        source: serde_json::Error,
    },
    /// A parseable cache contradicts the identity or answer it carries.
    #[error("passing baseline cache {} contradicts itself: {detail}", path.display())]
    Contradiction {
        /// The cache document.
        path: PathBuf,
        /// The invariant that failed.
        detail: &'static str,
    },
    /// A cached target outcome is not in the engine's closed vocabulary.
    #[error("passing baseline cache {} records unknown outcome {outcome:?} for {target}", path.display())]
    UnknownOutcome {
        /// The cache document.
        path: PathBuf,
        /// The target whose row is invalid.
        target: String,
        /// The unrecognized spelling.
        outcome: String,
    },
    /// A built artifact could not be read for its exact identity.
    #[error("cannot read built target {}: {source}", path.display())]
    ArtifactUnreadable {
        /// The built artifact.
        path: PathBuf,
        /// The filesystem failure.
        #[source]
        source: std::io::Error,
    },
    /// The operating system reported an impossible read count.
    #[error(
        "reading built target {} reported {read} bytes into a {capacity}-byte buffer",
        path.display()
    )]
    InvalidReadCount {
        /// The built artifact.
        path: PathBuf,
        /// The reported byte count.
        read: usize,
        /// The supplied buffer capacity.
        capacity: usize,
    },
    /// A target has no passing baseline to persist.
    #[error("{target} has no passing baseline to remember")]
    MissingPassingBaseline {
        /// The target identity.
        target: String,
    },
    /// A target has no observed test count to persist.
    #[error("{target} has no baseline test count to remember")]
    MissingTestCount {
        /// The target identity.
        target: String,
    },
    /// The answer identity could not be encoded.
    #[error("the passing baseline answer could not be encoded: {source}")]
    AnswerUnencodable {
        /// The JSON failure.
        #[source]
        source: serde_json::Error,
    },
    /// The cache document could not be encoded.
    #[error("the passing baseline document could not be encoded: {source}")]
    DocumentUnencodable {
        /// The JSON failure.
        #[source]
        source: serde_json::Error,
    },
    /// The cache document could not be committed atomically.
    #[error("the passing baseline could not be written at {}: {source}", path.display())]
    WriteFailed {
        /// The cache document.
        path: PathBuf,
        /// The filesystem failure.
        #[source]
        source: std::io::Error,
    },
}

impl Remembering {
    /// Names a baseline by every input visible to its processes or to the engine interpreting their answers.
    /// The actual executable bytes are checked on read as well: source equality never stands in for program equality.
    fn of(
        targets: &[TestTarget],
        scratch: &Path,
        building: &Building<'_>,
    ) -> Result<Option<Self>, BaselineCacheError> {
        let Some(directory) = building.options.measurements.clone() else {
            return Ok(None);
        };
        let mut key = Key::default();
        key.text("domain", "rust-mutants-passing-baseline")?;
        key.u64("abi", u64::from(BASELINE_ABI))?;
        key.text("engine", crate::VERSION)?;
        key.text("workspace", building.workspace.workspace_digest())?;
        key.text("closure", building.closure)?;
        key.text("manifests", building.manifests)?;
        key.text("catalog", building.catalog.digest())?;
        key.text(
            "cargo-version",
            &building.workspace.toolchain.cargo_version().summary,
        )?;
        key.text(
            "rustc-version",
            &building.workspace.toolchain.rustc_version().summary,
        )?;
        key.text("host", building.workspace.toolchain.host())?;
        key.os("cargo", building.workspace.toolchain.cargo().as_os_str())?;
        key.os("rustc", building.workspace.toolchain.rustc().as_os_str())?;
        key.boolean("touch", building.asked)?;
        key.boolean("doctests", building.options.doctests)?;
        key.boolean("locked", building.workspace.locked)?;
        key.boolean("offline", building.workspace.offline)?;
        key.boolean("debug", building.options.build.debug)?;
        key.texts("build", &building.options.build.arguments())?;
        key.texts("packages", &building.options.packages)?;
        key.texts("skip-targets", &building.options.skip_targets)?;
        key.u64(
            "accepted-count",
            baseline_count(BaselineQuantity::AcceptedMutants, building.accepted.len())?,
        )?;
        for index in building.accepted {
            key.u64("accepted", u64::from(*index))?;
        }
        key.u64(
            "compared-count",
            baseline_count(
                BaselineQuantity::ComparedMutants,
                building.narrowing.compared.len(),
            )?,
        )?;
        for index in &building.narrowing.compared {
            key.u64("compared", u64::from(*index))?;
        }
        key.u64(
            "bodies-count",
            baseline_count(
                BaselineQuantity::BodyMarkers,
                building.narrowing.bodies.len(),
            )?,
        )?;
        for (index, marker) in &building.narrowing.bodies {
            key.u64("body", u64::from(*index))?;
            key.u64("marker", u64::from(*marker))?;
        }
        key.u64(
            "target-count",
            baseline_count(BaselineQuantity::Targets, targets.len())?,
        )?;
        for (index, target) in targets.iter().enumerate() {
            target_key(
                &mut key,
                target,
                (scratch, &baseline_scratch(scratch, index)),
                building,
            )?;
        }
        Ok(Some(Self {
            directory,
            key: crate::id::digest(&key.0),
            artifacts: artifacts(targets)?,
        }))
    }

    fn path(&self) -> PathBuf {
        self.directory.join(format!("baseline-{}.json", self.key))
    }

    /// Reads only a whole, passing document for the exact binaries that are about to be run.
    /// Any malformed or stale part turns the whole document into a miss.
    fn read(
        &self,
        targets: &[TestTarget],
        catalog: &Catalog,
    ) -> Result<Option<Recalled>, BaselineCacheError> {
        let path = self.path();
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(BaselineCacheError::ReadFailed { path, source });
            }
        };
        let remembered: Remembered = crate::strictjson::decode_slice(&bytes).map_err(|source| {
            BaselineCacheError::UnreadableDocument {
                path: path.clone(),
                source,
            }
        })?;
        if !self.document_matches(
            &remembered,
            RecallPremise {
                targets,
                catalog,
                path: &path,
            },
        )? {
            return Ok(None);
        }
        recalled(remembered, &path).map(Some)
    }

    fn document_matches(
        &self,
        remembered: &Remembered,
        premise: RecallPremise<'_>,
    ) -> Result<bool, BaselineCacheError> {
        let RecallPremise {
            targets,
            catalog,
            path,
        } = premise;
        if remembered.abi != BASELINE_ABI || remembered.key != self.key {
            return Err(BaselineCacheError::Contradiction {
                path: path.to_path_buf(),
                detail: "the ABI or embedded key differs from the key in its file name",
            });
        }
        let answer = answer_digest(
            &remembered.artifacts,
            &remembered.targets,
            &remembered.touched,
        )?;
        if answer != remembered.answer {
            return Err(BaselineCacheError::Contradiction {
                path: path.to_path_buf(),
                detail: "the recorded answer digest does not match its facts",
            });
        }
        let ids: BTreeSet<&str> = targets.iter().map(|target| target.id.as_str()).collect();
        if !remembered
            .targets
            .keys()
            .map(String::as_str)
            .eq(ids.iter().copied())
            || !remembered
                .artifacts
                .keys()
                .map(String::as_str)
                .eq(ids.iter().copied())
            || self.artifacts != remembered.artifacts
            || !valid_touches(&remembered.touched, &ids, catalog)
        {
            return Ok(false);
        }
        Ok(true)
    }

    /// Writes only a passing answer and the byte identity of every executable.
    /// A write failure merely makes the next run measure again.
    fn write(
        &self,
        verified: &Verified,
        tests_run: &BTreeMap<String, Option<u32>>,
        targets: &[TestTarget],
    ) -> Result<(), BaselineCacheError> {
        let mut remembered_targets = BTreeMap::new();
        for target in targets {
            let Some(baseline) = verified
                .targets
                .get(&target.id)
                .and_then(Measured::judgeable)
                .map(Passing::baseline)
            else {
                return Err(BaselineCacheError::MissingPassingBaseline {
                    target: target.id.clone(),
                });
            };
            let Some(observed_tests_run) = tests_run.get(&target.id) else {
                return Err(BaselineCacheError::MissingTestCount {
                    target: target.id.clone(),
                });
            };
            let previous = remembered_targets.insert(
                target.id.clone(),
                RememberedBaseline {
                    outcome: baseline.outcome.name().to_owned(),
                    duration_nanos: u64::try_from(baseline.duration.as_nanos()).map_err(
                        |_outside_range| BaselineCacheError::DurationNanosOutsideRange {
                            duration: baseline.duration,
                        },
                    )?,
                    tests: baseline.tests,
                    ignored: baseline.ignored,
                    tests_run: *observed_tests_run,
                },
            );
            if previous.is_some() {
                return Err(BaselineCacheError::DuplicateTarget {
                    target: target.id.clone(),
                });
            }
        }
        let answer = serde_json::to_vec(&(&self.artifacts, &remembered_targets, &verified.touched))
            .map(|bytes| crate::id::digest(&bytes))
            .map_err(|source| BaselineCacheError::AnswerUnencodable { source })?;
        let document = Remembered {
            abi: BASELINE_ABI,
            key: self.key.clone(),
            answer,
            artifacts: self.artifacts.clone(),
            targets: remembered_targets,
            touched: verified.touched.clone(),
        };
        let bytes = serde_json::to_vec(&document)
            .map_err(|source| BaselineCacheError::DocumentUnencodable { source })?;
        crate::replace::file(&self.path(), &bytes).map_err(|error| {
            BaselineCacheError::WriteFailed {
                path: error.path,
                source: error.source,
            }
        })
    }
}

fn recalled(remembered: Remembered, path: &Path) -> Result<Recalled, BaselineCacheError> {
    let mut verified = Verified {
        touched: remembered.touched,
        ..Verified::default()
    };
    let mut tests_run = BTreeMap::new();
    verified.touched.narrowing = crate::touch::Narrowing::default();
    for (target, baseline) in remembered.targets {
        let Some(outcome) = crate::outcome::Outcome::parse(&baseline.outcome) else {
            return Err(BaselineCacheError::UnknownOutcome {
                path: path.to_path_buf(),
                target,
                outcome: baseline.outcome,
            });
        };
        let value = Baseline {
            outcome,
            duration: Duration::from_nanos(baseline.duration_nanos),
            tests: baseline.tests,
            ignored: baseline.ignored,
            output: String::new(),
        };
        if !value.passed() {
            return Err(BaselineCacheError::Contradiction {
                path: path.to_path_buf(),
                detail: "a remembered baseline is not passing",
            });
        }
        if tests_run
            .insert(target.clone(), baseline.tests_run)
            .is_some()
            || verified
                .targets
                .insert(target.clone(), Measured::of(value))
                .is_some()
        {
            return Err(BaselineCacheError::DuplicateTarget { target });
        }
    }
    Ok(Recalled {
        verified,
        tests_run,
    })
}

/// Integrity of the remembered answer itself.
/// The input key prevents a stale answer being selected; this prevents a parseable partial edit from being mistaken for the whole answer that was written.
fn answer_digest(
    artifacts: &BTreeMap<String, String>,
    targets: &BTreeMap<String, RememberedBaseline>,
    touched: &crate::touch::Touched,
) -> Result<String, BaselineCacheError> {
    serde_json::to_vec(&(artifacts, targets, touched))
        .map(|bytes| crate::id::digest(&bytes))
        .map_err(|source| BaselineCacheError::AnswerUnencodable { source })
}

/// Re-emits the same auditable facts a fresh verification emits.
fn replay(
    verified: &Verified,
    tests_run: &BTreeMap<String, Option<u32>>,
    targets: &mut [TestTarget],
    trace: &crate::trace::Recorder,
) -> Result<(), EngineError> {
    for target in targets {
        let Some(baseline) = verified.targets.get(&target.id).map(Measured::baseline) else {
            continue;
        };
        trace.verify(crate::trace::VerifyRecord {
            target: target.id.clone(),
            outcome: baseline.outcome.name().to_owned(),
            tests_run: tests_run
                .get(&target.id)
                .copied()
                .and_then(std::convert::identity),
            duration_ms: duration_millis(baseline.duration)?,
            remembered: true,
            retried: false,
        });
        if target.kind == TargetKind::Doc
            && tests_run
                .get(&target.id)
                .copied()
                .and_then(std::convert::identity)
                == Some(0)
        {
            target
                .limitations
                .push(crate::limitation::DOCTESTS_NONE.to_owned());
        }
        if let Some(touched) = verified.touched.targets.get(&target.id) {
            trace_touch(&target.id, touched, trace)?;
        }
    }
    Ok(())
}

/// The aggregate record [`gather`] emits for one target, reconstructed from a remembered log without pretending that the log was run again.
fn trace_touch(
    target: &str,
    gathered: &crate::touch::TargetTouches,
    trace: &crate::trace::Recorder,
) -> Result<(), SessionError> {
    trace.touch(touch_record(
        target,
        crate::trace::Measurement::Baseline,
        gathered,
        crate::trace::SummaryRecord::Remembered,
    )?);
    Ok(())
}

/// What one whole run of a target recorded, as the recording carries it: the counts a reader skims and the unions an audit compares.
pub(super) fn touch_record(
    target: &str,
    measured: crate::trace::Measurement,
    gathered: &crate::touch::TargetTouches,
    summary: crate::trace::SummaryRecord,
) -> Result<crate::trace::TouchRecord, SessionError> {
    let reached = gathered.reached.union();
    let infected = gathered.infected.union();
    Ok(crate::trace::TouchRecord {
        target: target.to_owned(),
        measured,
        passed: gathered.ran.clone(),
        summary,
        tests: trace_count("recorded baseline tests", gathered.reached.tests.len())?,
        sites: trace_count("recorded baseline sites", reached.len())?,
        loose: trace_count(
            "loosely attributed baseline sites",
            gathered.reached.loose.len(),
        )?,
        infected: trace_count("infected baseline sites", infected.len())?,
        reached_sites: reached.into_iter().collect(),
        entered_bodies: gathered.bodies.union().into_iter().collect(),
        infected_sites: infected.into_iter().collect(),
    })
}

/// The actual programs built now, keyed by target.
/// Repeated paths (notably Cargo for doctest targets) are hashed once.
fn artifacts(targets: &[TestTarget]) -> Result<BTreeMap<String, String>, BaselineCacheError> {
    let mut files: BTreeMap<PathBuf, String> = BTreeMap::new();
    let mut found = BTreeMap::new();
    for target in targets {
        let digest = if let Some(digest) = files.get(&target.executable) {
            digest.clone()
        } else {
            let digest = file_digest(&target.executable)?;
            if files
                .insert(target.executable.clone(), digest.clone())
                .is_some()
            {
                return Err(BaselineCacheError::DuplicateTarget {
                    target: target.id.clone(),
                });
            }
            digest
        };
        if found.insert(target.id.clone(), digest).is_some() {
            return Err(BaselineCacheError::DuplicateTarget {
                target: target.id.clone(),
            });
        }
    }
    Ok(found)
}

fn file_digest(path: &Path) -> Result<String, BaselineCacheError> {
    let file =
        std::fs::File::open(path).map_err(|source| BaselineCacheError::ArtifactUnreadable {
            path: path.to_path_buf(),
            source,
        })?;
    let mut reader = std::io::BufReader::with_capacity(256 * 1024, file);
    let mut buffer = vec![0_u8; 256 * 1024];
    let mut hasher = Sha256::new();
    loop {
        let read =
            reader
                .read(&mut buffer)
                .map_err(|source| BaselineCacheError::ArtifactUnreadable {
                    path: path.to_path_buf(),
                    source,
                })?;
        if read == 0 {
            break;
        }
        let bytes = buffer
            .get(..read)
            .ok_or_else(|| BaselineCacheError::InvalidReadCount {
                path: path.to_path_buf(),
                read,
                capacity: buffer.len(),
            })?;
        hasher.update(bytes);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Rejects a syntactically valid cache that names a target or catalog site the current build does not have.
fn valid_touches(
    touched: &crate::touch::Touched,
    targets: &BTreeSet<&str>,
    catalog: &Catalog,
) -> bool {
    if touched.narrowing != crate::touch::Narrowing::default() {
        return false;
    }
    let limited: BTreeSet<&str> = touched
        .limitations
        .iter()
        .filter_map(|limitation| limitation.split_once(':').map(|(_, target)| target))
        .collect();
    if touched
        .targets
        .keys()
        .any(|target| !targets.contains(target.as_str()))
        || touched.limitations.iter().any(|limitation| {
            limitation
                .split_once(':')
                .is_none_or(|(_, target)| !targets.contains(target))
        })
    {
        return false;
    }
    if targets
        .iter()
        .any(|target| touched.targets.contains_key(*target) == limited.contains(*target))
    {
        return false;
    }
    let valid = |index: &u32| catalog.by_index(*index).is_some();
    let seen = |seen: &crate::touch::Seen| {
        seen.loose.iter().all(valid)
            && seen
                .tests
                .values()
                .flat_map(|indices| indices.iter())
                .all(valid)
    };
    touched.targets.values().all(|target| {
        let ran: BTreeSet<&str> = target.ran.iter().map(String::as_str).collect();
        target
            .reached
            .tests
            .keys()
            .chain(target.bodies.tests.keys())
            .chain(target.infected.tests.keys())
            .all(|test| ran.contains(test.as_str()))
            && seen(&target.reached)
            && seen(&target.bodies)
            && seen(&target.infected)
    }) && touched.narrowing.compared.iter().all(valid)
        && touched
            .narrowing
            .bodies
            .iter()
            .all(|(index, marker)| valid(index) && valid(marker))
}

/// Adds unambiguous, length-prefixed fields to a content key.
#[derive(Default)]
struct Key(Vec<u8>);

impl Key {
    fn bytes(&mut self, name: &str, value: &[u8]) -> Result<(), BaselineCacheError> {
        self.raw(name.as_bytes())?;
        self.raw(value)
    }

    fn raw(&mut self, value: &[u8]) -> Result<(), BaselineCacheError> {
        let length = baseline_count(BaselineQuantity::KeyValues, value.len())?;
        self.0.extend_from_slice(&length.to_be_bytes());
        self.0.extend_from_slice(value);
        Ok(())
    }

    fn text(&mut self, name: &str, value: &str) -> Result<(), BaselineCacheError> {
        self.bytes(name, value.as_bytes())
    }

    fn os(&mut self, name: &str, value: &OsStr) -> Result<(), BaselineCacheError> {
        self.bytes(name, &os_bytes(value))
    }

    fn u64(&mut self, name: &str, value: u64) -> Result<(), BaselineCacheError> {
        self.bytes(name, &value.to_be_bytes())
    }

    fn boolean(&mut self, name: &str, value: bool) -> Result<(), BaselineCacheError> {
        self.bytes(name, &[u8::from(value)])
    }

    fn texts(&mut self, name: &str, values: &[String]) -> Result<(), BaselineCacheError> {
        self.u64(
            &format!("{name}-count"),
            baseline_count(BaselineQuantity::KeyValues, values.len())?,
        )?;
        for value in values {
            self.text(name, value)?;
        }
        Ok(())
    }
}

/// Everything the baseline process actually observes for one target.
fn target_key(
    key: &mut Key,
    target: &TestTarget,
    (scratch, own): (&Path, &Path),
    building: &Building<'_>,
) -> Result<(), BaselineCacheError> {
    key.text("target-id", &target.id)?;
    key.text("target-package", &target.package)?;
    key.text("target-kind", target.kind.name())?;
    key.text("target-name", &target.name)?;
    key.boolean("target-harness", target.harness)?;
    key.texts("target-limitations", &target.limitations)?;
    key.os("target-cwd", target.cwd.as_os_str())?;
    let recording = (building.asked && recordable(target)).then(|| {
        scratch
            .join("touch")
            .join(format!("{}.log", slug(&target.id)))
    });
    let context = Context {
        base_env: &building.workspace.base_env,
        cargo: Some(building.workspace.toolchain.cargo()),
        sysroot: building.workspace.toolchain.sysroot(),
        active: None,
        beside: None,
        touch: recording.as_deref().map(|log| execute::Touching {
            log,
            catalog: building.catalog.digest(),
        }),
        steps: None,
        profile: None,
        crash: None,
    };
    let request = ExecRequest::new(target)
        .with_args(building.options.harness_args.clone())
        .with_scratch(own)
        .in_scratch(building.options.scratch_working_directory);
    let argv = request.argv();
    key.u64(
        "argv-count",
        baseline_count(BaselineQuantity::Arguments, argv.len())?,
    )?;
    for argument in argv {
        key.os("argv", &argument)?;
    }
    let environment =
        execute::environment(&context, target, (Some(own), Some(own))).map_err(|source| {
            BaselineCacheError::EnvironmentUnavailable {
                target: target.id.clone(),
                source,
            }
        })?;
    key.u64(
        "environment-count",
        baseline_count(BaselineQuantity::Environment, environment.len())?,
    )?;
    for (name, value) in environment {
        key.os("environment-name", &name)?;
        key.os("environment-value", &value)?;
    }
    Ok(())
}

#[cfg(unix)]
fn os_bytes(value: &OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt as _;
    value.as_bytes().to_vec()
}

#[cfg(windows)]
fn os_bytes(value: &OsStr) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt as _;
    value.encode_wide().flat_map(u16::to_le_bytes).collect()
}

#[cfg(not(any(unix, windows)))]
fn os_bytes(value: &OsStr) -> Vec<u8> {
    value.as_encoded_bytes().to_vec()
}

/// Refuses a tree whose instrumented baseline does not pass, once every target has been asked.
fn refused(verified: &Verified, failing: Failing) -> Result<(), EngineError> {
    if verified.failing().is_empty() || failing == Failing::Exclude {
        return Ok(());
    }
    Err(refusal(verified))
}

/// The refusal a tree earns whose instrumented baseline does not pass, naming every target of it that failed.
pub(super) fn refusal(verified: &Verified) -> EngineError {
    let failed = verified.failing();
    EngineError::from(SessionError::VerifyFailed {
        targets: failed.iter().map(|target| (*target).to_owned()).collect(),
        output: said(verified, &failed),
    })
}

/// What every failing target printed, each under its own name.
fn said(verified: &Verified, failed: &[&str]) -> String {
    let mut text = String::new();
    for target in failed {
        let Some(baseline) = verified.targets.get(*target).map(Measured::baseline) else {
            continue;
        };
        if baseline.output.trim().is_empty() {
            continue;
        }
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(target);
        text.push('\n');
        text.push_str(&baseline.output);
    }
    text
}

/// One target run with nothing active in the temporary directory of its baseline, recording into `log` when it was asked to.
fn ran(
    target: &TestTarget,
    scratch: &Path,
    log: Option<&Path>,
    building: &Building<'_>,
) -> MutantResult {
    let Building {
        cancel,
        workspace,
        catalog,
        ..
    } = *building;
    if let Err(error) = std::fs::DirBuilder::new().recursive(true).create(scratch) {
        return MutantResult::apparatus_error(
            &target.id,
            format!(
                "could not make the baseline's own temporary directory {}: {error}",
                scratch.display()
            ),
        );
    }
    if let Some(path) = log {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                let message = format!("could not clear touch log {}: {error}", path.display());
                workspace.trace.note(crate::touch::UNRECORDED, &message);
                return MutantResult::apparatus_error(&target.id, message);
            }
        }
    }
    let context = Context {
        base_env: &workspace.base_env,
        cargo: Some(workspace.toolchain.cargo()),
        sysroot: workspace.toolchain.sysroot(),
        active: None,
        beside: None,
        touch: log.map(|log| execute::Touching {
            log,
            catalog: catalog.digest(),
        }),
        steps: None,
        profile: None,
        crash: None,
    };
    let request = ExecRequest::new(target)
        .with_args(building.options.harness_args.clone())
        .with_scratch(scratch)
        .in_scratch(building.options.scratch_working_directory);
    execute::exec(&request, &context, cancel, &workspace.trace)
}

/// What one target's baseline came to on the run that verified it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Baseline {
    /// What running it with nothing active came to.
    pub outcome: crate::outcome::Outcome,
    /// How long it took, which is what a derived timeout is a multiple of.
    pub duration: Duration,
    /// How many tests it ran, which is what asking the whole of it about one mutation costs.
    pub tests: u32,
    /// How many tests the harness was told to skip, which is what tells a target that ran nothing from one that said nothing.
    pub ignored: u32,
    /// What it printed, kept only where it did not pass, because that is the only time anybody reads it.
    pub output: String,
}

/// Whether an outcome with nothing active is one a mutation can be put to.
const fn passing(outcome: crate::outcome::Outcome) -> bool {
    matches!(
        outcome,
        crate::outcome::Outcome::Survived | crate::outcome::Outcome::Inconclusive
    )
}

impl Baseline {
    /// Whether this target can be judged against.
    #[must_use]
    pub const fn passed(&self) -> bool {
        passing(self.outcome)
    }
}

/// What the one run of every target with nothing activated established.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct Verified {
    /// What each target's own baseline came to, by target identity.
    pub targets: BTreeMap<String, Measured>,
    /// What the guards recorded on that same run.
    pub touched: crate::touch::Touched,
}

/// What one target's baseline came to, in the two cases that mean different things.
///
/// The distinction used to be a method somebody had to remember to call.
/// A target whose own tests do not pass answers every mutation with the same failure, so a run that judged against one would report a kill for every mutation it put to it and not one of those kills would be about a mutation.
/// Taking a baseline out of here now makes the caller say which case they are in, and only one of the two hands back something a mutation can be judged against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Measured {
    /// A baseline a mutation may be put to.
    Passing(Passing),
    /// One it may not, kept because a reader has to be told which target it was.
    Failing(Baseline),
}

impl Measured {
    /// What the target came to, whichever case it is in, for an account that covers all of them.
    #[must_use]
    pub const fn baseline(&self) -> &Baseline {
        match self {
            Self::Passing(passing) => passing.baseline(),
            Self::Failing(baseline) => baseline,
        }
    }

    /// The baseline where a mutation may be judged against it, and nothing where it may not.
    #[must_use]
    pub const fn judgeable(&self) -> Option<&Passing> {
        match self {
            Self::Passing(passing) => Some(passing),
            Self::Failing(_) => None,
        }
    }

    /// Which case `baseline` is in, decided once here rather than at every use.
    #[must_use]
    pub const fn of(baseline: Baseline) -> Self {
        if baseline.passed() {
            Self::Passing(Passing(baseline))
        } else {
            Self::Failing(baseline)
        }
    }
}

/// A baseline that passed, which is the only kind a mutation may be judged against.
///
/// There is no way to make one from a baseline that did not, so a function that takes this has been given the check rather than asked to remember it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passing(Baseline);

impl Passing {
    /// What the target came to.
    #[must_use]
    pub const fn baseline(&self) -> &Baseline {
        &self.0
    }
}

impl Verified {
    /// Every target whose baseline did not pass, in identity order.
    #[must_use]
    pub fn failing(&self) -> Vec<&str> {
        self.targets
            .iter()
            .filter(|(_, measured)| measured.judgeable().is_none())
            .map(|(target, _)| target.as_str())
            .collect()
    }

    /// The baseline a mutation may be judged against for `target`, and nothing where there is none.
    ///
    /// The only way to a baseline a result may rest on.
    /// Everything else hands back what the target came to for an account of it, which is a different question and reads differently at the call site.
    #[must_use]
    pub fn judgeable(&self, target: &str) -> Option<&Passing> {
        self.targets.get(target).and_then(Measured::judgeable)
    }
}

/// One target's record, and what makes sense of it.
struct Recording<'a> {
    /// The target the record is about.
    target: &'a str,
    /// Where its guards were told to append, or nothing when they were not asked.
    log: Option<&'a Path>,
    /// The catalog the record must be about.
    catalog: &'a Catalog,
    /// Every test the run of it passed, which is what names a thread a touch can be attributed to.
    ran: &'a [String],
    /// What the run's own summary said, in the protocol it answered in.
    summarised: crate::trace::SummaryRecord,
}

/// Whether a target's guards can be asked what they reached.
pub(super) fn recordable(target: &TestTarget) -> bool {
    target.kind != TargetKind::Doc && target.through.is_empty()
}

/// Reads one target's record into `touched`, or says why there is nothing of it to read.
fn gather(
    touched: &mut crate::touch::Touched,
    recording: &Recording<'_>,
    trace: &crate::trace::Recorder,
) -> Result<(), SessionError> {
    let Some(log) = recording.log else {
        touched.limited(crate::touch::UNRECORDED, recording.target);
        return Ok(());
    };
    let unreadable = |touched: &mut crate::touch::Touched, why: &dyn std::fmt::Display| {
        trace.note(
            crate::touch::UNREADABLE,
            &format!("{}: {why}", recording.target),
        );
        touched.limited(crate::touch::UNREADABLE, recording.target);
    };
    let text = match crate::limitation::appended(std::fs::read_to_string(log)) {
        Ok(text) => text,
        Err(error) => {
            unreadable(touched, &error);
            return Ok(());
        }
    };
    let count = trace_count(
        "catalog mutants in a touch record",
        recording.catalog.mutants().len(),
    )?;
    let recorded = match crate::touch::read(&text, recording.catalog.digest(), count) {
        Ok(recorded) => recorded,
        Err(error) => {
            unreadable(touched, &error);
            return Ok(());
        }
    };
    let gathered = crate::touch::TargetTouches::of(recorded, recording.ran);
    trace.touch(touch_record(
        recording.target,
        crate::trace::Measurement::Baseline,
        &gathered,
        recording.summarised,
    )?);
    if touched
        .targets
        .insert(recording.target.to_owned(), gathered)
        .is_some()
    {
        return Err(SessionError::DuplicateBaselineTarget {
            target: recording.target.to_owned(),
        });
    }
    Ok(())
}

/// A target identity as one path segment, so two targets cannot name one file.
fn slug(target: &str) -> String {
    let readable: String = target
        .chars()
        .map(|letter| {
            if letter.is_ascii_alphanumeric() {
                letter
            } else {
                '-'
            }
        })
        .collect();
    let digest = crate::id::digest(target.as_bytes());
    format!("{readable}-{digest}")
}
