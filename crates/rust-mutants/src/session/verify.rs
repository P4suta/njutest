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
use crate::execute::{self, Context, ExecRequest, MutantResult, TargetKind, TestTarget};
use crate::workspace::{SessionError, Workspace};

/// Runs every target once with nothing active. A tree whose instrumented baseline fails is one whose every later result would be about the instrumentation rather than about a mutant.
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
            workspace.trace.note(BASELINE_NOT_REMEMBERED, &why);
            None
        }
    };
    if !building.cancel.is_cancelled()
        && let Some(recalled) = remembering
            .as_ref()
            .and_then(|one| one.read(targets, catalog))
    {
        workspace
            .trace
            .note(BASELINE_REMEMBERED, "the directly built executables are byte-identical and every other baseline input matches the passing measurement already made");
        replay(
            &recalled.verified,
            &recalled.tests_run,
            targets,
            &workspace.trace,
        );
        phase.end();
        return Ok(recalled.verified);
    }
    let (verified, tests_run) = verify_targets(targets, scratch, building);
    phase.end();
    refused(&verified, building.options.failing)?;
    if verified.failing().is_empty()
        && !building.cancel.is_cancelled()
        && let Some(remembering) = remembering
    {
        match workspace.snapshot.redigest() {
            Ok(drift) if drift.is_empty() => {
                if let Err(why) = remembering.write(&verified, &tests_run, targets) {
                    workspace.trace.note(BASELINE_NOT_REMEMBERED, &why);
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
) -> (Verified, BTreeMap<String, Option<u32>>) {
    let mut verified = Verified::default();
    let mut tests_run = BTreeMap::new();
    for target in targets {
        let (baseline, observed) = verify_target(target, scratch, building, &mut verified.touched);
        let _old = tests_run.insert(target.id.clone(), observed);
        let _kept = verified
            .targets
            .insert(target.id.clone(), Measured::of(baseline));
    }
    (verified, tests_run)
}

fn verify_target(
    target: &mut TestTarget,
    scratch: &Path,
    building: &Building<'_>,
    touched: &mut crate::touch::Touched,
) -> (Baseline, Option<u32>) {
    let recording = (building.asked && recordable(target)).then(|| {
        scratch
            .join("touch")
            .join(format!("{}.log", slug(&target.id)))
    });
    let mut result = ran(target, scratch, recording.as_deref(), building);
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
        result = ran(target, scratch, None, building);
        None
    } else {
        recording
    };
    if let Some(again) = again(&result, target, (scratch, recording.as_deref()), building) {
        result = again;
        retried = true;
    }
    building.trace.verify(crate::trace::VerifyRecord {
        target: target.id.clone(),
        outcome: result.outcome.name().to_owned(),
        tests_run: result.tests_run,
        duration_ms: u64::try_from(result.duration.as_millis()).unwrap_or(u64::MAX),
        remembered: false,
        retried,
    });
    let baseline = Baseline {
        outcome: result.outcome,
        duration: result.duration,
        tests: result
            .tests_run
            .unwrap_or_else(|| u32::try_from(result.passed_tests.len()).unwrap_or(u32::MAX)),
        ignored: u32::try_from(result.ignored_tests.len()).unwrap_or(u32::MAX),
        output: if matches!(
            result.outcome,
            crate::outcome::Outcome::Survived | crate::outcome::Outcome::Inconclusive
        ) {
            String::new()
        } else {
            String::from_utf8_lossy(&result.output).into_owned()
        },
    };
    if target.kind == TargetKind::Doc && result.tests_run == Some(0) {
        target
            .limitations
            .push(crate::limitation::DOCTESTS_NONE.to_owned());
    }
    if baseline.passed() && retried {
        touched.limited(crate::limitation::BASELINE_PASSED_ON_RETRY, &target.id);
    }
    if baseline.passed() {
        gather(
            touched,
            &Recording {
                target: &target.id,
                log: recording.as_deref(),
                catalog: building.catalog,
                ran: &result.passed_tests,
            },
            building.trace,
        );
    } else {
        touched.limited(crate::limitation::BASELINE_NOT_PASSING, &target.id);
    }
    (baseline, result.tests_run)
}

/// The trace note proving why no baseline process follows it.
const BASELINE_REMEMBERED: &str = "baseline-remembered";

/// The trace note saying a target was run a second time, and why.
const BASELINE_RETRIED: &str = "baseline-retried";

/// One more run of a target that did not pass, or nothing when the first answer stands.
fn again(
    result: &MutantResult,
    target: &TestTarget,
    (scratch, recording): (&Path, Option<&Path>),
    building: &Building<'_>,
) -> Option<MutantResult> {
    if passing(result.outcome) || building.cancel.is_cancelled() {
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

/// The recipe of a remembered baseline. The engine version is also in every key; this number makes a semantic invalidation explicit within one build.
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

impl Remembering {
    /// Names a baseline by every input visible to its processes or to the engine interpreting their answers. The actual executable bytes are checked on read as well: source equality never stands in for program equality.
    fn of(
        targets: &[TestTarget],
        scratch: &Path,
        building: &Building<'_>,
    ) -> Result<Option<Self>, String> {
        let Some(directory) = building.options.measurements.clone() else {
            return Ok(None);
        };
        let mut key = Key::default();
        key.text("domain", "rust-mutants-passing-baseline");
        key.u64("abi", u64::from(BASELINE_ABI));
        key.text("engine", crate::VERSION);
        key.text("workspace", building.workspace.workspace_digest());
        key.text("closure", building.closure);
        key.text("manifests", building.manifests);
        key.text("catalog", building.catalog.digest());
        key.text(
            "cargo-version",
            &building.workspace.toolchain.cargo_version().summary,
        );
        key.text(
            "rustc-version",
            &building.workspace.toolchain.rustc_version().summary,
        );
        key.text("host", building.workspace.toolchain.host());
        key.os("cargo", building.workspace.toolchain.cargo().as_os_str());
        key.os("rustc", building.workspace.toolchain.rustc().as_os_str());
        key.boolean("touch", building.asked);
        key.boolean("doctests", building.options.doctests);
        key.boolean("locked", building.workspace.locked);
        key.boolean("offline", building.workspace.offline);
        key.boolean("debug", building.options.build.debug);
        key.texts("build", &building.options.build.arguments());
        key.texts("packages", &building.options.packages);
        key.texts("skip-targets", &building.options.skip_targets);
        key.u64(
            "accepted-count",
            u64::try_from(building.accepted.len()).unwrap_or(u64::MAX),
        );
        for index in building.accepted {
            key.u64("accepted", u64::from(*index));
        }
        key.u64(
            "compared-count",
            u64::try_from(building.narrowing.compared.len()).unwrap_or(u64::MAX),
        );
        for index in &building.narrowing.compared {
            key.u64("compared", u64::from(*index));
        }
        key.u64(
            "bodies-count",
            u64::try_from(building.narrowing.bodies.len()).unwrap_or(u64::MAX),
        );
        for (index, marker) in &building.narrowing.bodies {
            key.u64("body", u64::from(*index));
            key.u64("marker", u64::from(*marker));
        }
        key.u64(
            "target-count",
            u64::try_from(targets.len()).unwrap_or(u64::MAX),
        );
        for target in targets {
            target_key(&mut key, target, scratch, building);
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

    /// Reads only a whole, passing document for the exact binaries that are about to be run. Any malformed or stale part turns the whole document into a miss.
    fn read(&self, targets: &[TestTarget], catalog: &Catalog) -> Option<Recalled> {
        let bytes = std::fs::read(self.path()).ok()?;
        let remembered: Remembered = serde_json::from_slice(&bytes).ok()?;
        if remembered.abi != BASELINE_ABI || remembered.key != self.key {
            return None;
        }
        let answer = answer_digest(
            &remembered.artifacts,
            &remembered.targets,
            &remembered.touched,
        )?;
        if answer != remembered.answer {
            return None;
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
            return None;
        }
        let mut verified = Verified {
            touched: remembered.touched,
            ..Verified::default()
        };
        let mut tests_run = BTreeMap::new();
        verified.touched.narrowing = crate::touch::Narrowing::default();
        for (target, baseline) in remembered.targets {
            let outcome = crate::outcome::Outcome::parse(&baseline.outcome)?;
            let duration = Duration::from_nanos(baseline.duration_nanos);
            let value = Baseline {
                outcome,
                duration,
                tests: baseline.tests,
                ignored: baseline.ignored,
                output: String::new(),
            };
            if !value.passed() {
                return None;
            }
            let _old = tests_run.insert(target.clone(), baseline.tests_run);
            let _old = verified.targets.insert(target, Measured::of(value));
        }
        Some(Recalled {
            verified,
            tests_run,
        })
    }

    /// Writes only a passing answer and the byte identity of every executable. A write failure merely makes the next run measure again.
    fn write(
        &self,
        verified: &Verified,
        tests_run: &BTreeMap<String, Option<u32>>,
        targets: &[TestTarget],
    ) -> Result<(), String> {
        let mut remembered_targets = BTreeMap::new();
        for target in targets {
            let Some(baseline) = verified
                .targets
                .get(&target.id)
                .and_then(Measured::judgeable)
                .map(Passing::baseline)
            else {
                return Err(format!("{} has no passing baseline to remember", target.id));
            };
            let Some(observed_tests_run) = tests_run.get(&target.id) else {
                return Err(format!(
                    "{} has no baseline test count to remember",
                    target.id
                ));
            };
            let _old = remembered_targets.insert(
                target.id.clone(),
                RememberedBaseline {
                    outcome: baseline.outcome.name().to_owned(),
                    duration_nanos: u64::try_from(baseline.duration.as_nanos()).unwrap_or(u64::MAX),
                    tests: baseline.tests,
                    ignored: baseline.ignored,
                    tests_run: *observed_tests_run,
                },
            );
        }
        let answer = answer_digest(&self.artifacts, &remembered_targets, &verified.touched)
            .ok_or_else(|| "the passing baseline could not be encoded".to_owned())?;
        let document = Remembered {
            abi: BASELINE_ABI,
            key: self.key.clone(),
            answer,
            artifacts: self.artifacts.clone(),
            targets: remembered_targets,
            touched: verified.touched.clone(),
        };
        let bytes = serde_json::to_vec(&document)
            .map_err(|error| format!("the passing baseline could not be encoded: {error}"))?;
        crate::replace::file(&self.path(), &bytes).map_err(|error| {
            format!(
                "the passing baseline could not be written at {}: {}",
                error.path.display(),
                error.source
            )
        })
    }
}

/// Integrity of the remembered answer itself. The input key prevents a stale answer being selected; this prevents a parseable partial edit from being mistaken for the whole answer that was written.
fn answer_digest(
    artifacts: &BTreeMap<String, String>,
    targets: &BTreeMap<String, RememberedBaseline>,
    touched: &crate::touch::Touched,
) -> Option<String> {
    serde_json::to_vec(&(artifacts, targets, touched))
        .ok()
        .map(|bytes| crate::id::digest(&bytes))
}

/// Re-emits the same auditable facts a fresh verification emits.
fn replay(
    verified: &Verified,
    tests_run: &BTreeMap<String, Option<u32>>,
    targets: &mut [TestTarget],
    trace: &crate::trace::Recorder,
) {
    for target in targets {
        let Some(baseline) = verified.targets.get(&target.id).map(Measured::baseline) else {
            continue;
        };
        trace.verify(crate::trace::VerifyRecord {
            target: target.id.clone(),
            outcome: baseline.outcome.name().to_owned(),
            tests_run: tests_run.get(&target.id).copied().flatten(),
            duration_ms: u64::try_from(baseline.duration.as_millis()).unwrap_or(u64::MAX),
            remembered: true,
            retried: false,
        });
        if target.kind == TargetKind::Doc && tests_run.get(&target.id).copied().flatten() == Some(0)
        {
            target
                .limitations
                .push(crate::limitation::DOCTESTS_NONE.to_owned());
        }
        if let Some(touched) = verified.touched.targets.get(&target.id) {
            trace_touch(&target.id, touched, trace);
        }
    }
}

/// The aggregate record [`gather`] emits for one target, reconstructed from a remembered log without pretending that the log was run again.
fn trace_touch(
    target: &str,
    gathered: &crate::touch::TargetTouches,
    trace: &crate::trace::Recorder,
) {
    trace.touch(crate::trace::TouchRecord {
        target: target.to_owned(),
        tests: counted(gathered.reached.tests.len()),
        sites: counted(
            gathered
                .reached
                .tests
                .values()
                .flatten()
                .chain(gathered.reached.loose.iter())
                .collect::<BTreeSet<&u32>>()
                .len(),
        ),
        loose: counted(gathered.reached.loose.len()),
        infected: counted(
            gathered
                .infected
                .tests
                .values()
                .flatten()
                .chain(gathered.infected.loose.iter())
                .collect::<BTreeSet<&u32>>()
                .len(),
        ),
    });
}

/// The actual programs built now, keyed by target. Repeated paths (notably Cargo for doctest targets) are hashed once.
fn artifacts(targets: &[TestTarget]) -> Result<BTreeMap<String, String>, String> {
    let mut files: BTreeMap<PathBuf, String> = BTreeMap::new();
    let mut found = BTreeMap::new();
    for target in targets {
        let digest = if let Some(digest) = files.get(&target.executable) {
            digest.clone()
        } else {
            let digest = file_digest(&target.executable)?;
            let _old = files.insert(target.executable.clone(), digest.clone());
            digest
        };
        let _old = found.insert(target.id.clone(), digest);
    }
    Ok(found)
}

fn file_digest(path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(path)
        .map_err(|error| format!("cannot read built target {}: {error}", path.display()))?;
    let mut reader = std::io::BufReader::with_capacity(256 * 1024, file);
    let mut buffer = vec![0_u8; 256 * 1024];
    let mut hasher = Sha256::new();
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("cannot read built target {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        let Some(bytes) = buffer.get(..read) else {
            return Err(format!("cannot read built target {} whole", path.display()));
        };
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
        seen.loose.iter().all(valid) && seen.tests.values().flatten().all(valid)
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
    fn bytes(&mut self, name: &str, value: &[u8]) {
        self.raw(name.as_bytes());
        self.raw(value);
    }

    fn raw(&mut self, value: &[u8]) {
        self.0
            .extend_from_slice(&u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
        self.0.extend_from_slice(value);
    }

    fn text(&mut self, name: &str, value: &str) {
        self.bytes(name, value.as_bytes());
    }

    fn os(&mut self, name: &str, value: &OsStr) {
        self.bytes(name, &os_bytes(value));
    }

    fn u64(&mut self, name: &str, value: u64) {
        self.bytes(name, &value.to_be_bytes());
    }

    fn boolean(&mut self, name: &str, value: bool) {
        self.bytes(name, &[u8::from(value)]);
    }

    fn texts(&mut self, name: &str, values: &[String]) {
        self.u64(
            &format!("{name}-count"),
            u64::try_from(values.len()).unwrap_or(u64::MAX),
        );
        for value in values {
            self.text(name, value);
        }
    }
}

/// Everything the baseline process actually observes for one target.
fn target_key(key: &mut Key, target: &TestTarget, scratch: &Path, building: &Building<'_>) {
    key.text("target-id", &target.id);
    key.text("target-package", &target.package);
    key.text("target-kind", target.kind.name());
    key.text("target-name", &target.name);
    key.boolean("target-harness", target.harness);
    key.texts("target-limitations", &target.limitations);
    key.os("target-cwd", target.cwd.as_os_str());
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
        touch: recording.as_deref().map(|log| execute::Touching {
            log,
            catalog: building.catalog.digest(),
        }),
        profile: None,
    };
    let request = ExecRequest::new(target)
        .with_args(building.options.harness_args.clone())
        .with_scratch(scratch)
        .in_scratch(building.options.scratch_working_directory);
    let argv = request.argv();
    key.u64("argv-count", u64::try_from(argv.len()).unwrap_or(u64::MAX));
    for argument in argv {
        key.os("argv", &argument);
    }
    let environment = execute::environment(&context, target, Some(scratch));
    key.u64(
        "environment-count",
        u64::try_from(environment.len()).unwrap_or(u64::MAX),
    );
    for (name, value) in environment {
        key.os("environment-name", &name);
        key.os("environment-value", &value);
    }
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
    value.to_string_lossy().as_bytes().to_vec()
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

/// One target run with nothing active, recording into `log` when it was asked to.
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
    if let Some(path) = log {
        drop(std::fs::remove_file(path));
    }
    let context = Context {
        base_env: &workspace.base_env,
        cargo: Some(workspace.toolchain.cargo()),
        sysroot: workspace.toolchain.sysroot(),
        active: None,
        touch: log.map(|log| execute::Touching {
            log,
            catalog: catalog.digest(),
        }),
        profile: None,
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
/// The distinction used to be a method somebody had to remember to call. A
/// target whose own tests do not pass answers every mutation with the same
/// failure, so a run that judged against one would report a kill for every
/// mutation it put to it and not one of those kills would be about a mutation.
/// Taking a baseline out of here now makes the caller say which case they are
/// in, and only one of the two hands back something a mutation can be judged
/// against.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
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
/// There is no way to make one from a baseline that did not, so a function
/// that takes this has been given the check rather than asked to remember it.
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
    /// The only way to a baseline a result may rest on. Everything else hands
    /// back what the target came to for an account of it, which is a different
    /// question and reads differently at the call site.
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
}

/// Whether a target's guards can be asked what they reached.
fn recordable(target: &TestTarget) -> bool {
    target.kind != TargetKind::Doc && target.through.is_empty()
}

/// Reads one target's record into `touched`, or says why there is nothing of it to read.
fn gather(
    touched: &mut crate::touch::Touched,
    recording: &Recording<'_>,
    trace: &crate::trace::Recorder,
) {
    let Some(log) = recording.log else {
        touched.limited(crate::touch::UNRECORDED, recording.target);
        return;
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
            return;
        }
    };
    let count = u32::try_from(recording.catalog.mutants().len()).unwrap_or(u32::MAX);
    let recorded = match crate::touch::read(&text, recording.catalog.digest(), count) {
        Ok(recorded) => recorded,
        Err(error) => {
            unreadable(touched, &error);
            return;
        }
    };
    let gathered = crate::touch::TargetTouches {
        reached: attributed(recorded.reached, recording.ran),
        bodies: attributed(recorded.bodies, recording.ran),
        infected: attributed(recorded.infected, recording.ran),
        ran: recording.ran.to_vec(),
    };
    trace.touch(crate::trace::TouchRecord {
        target: recording.target.to_owned(),
        tests: counted(gathered.reached.tests.len()),
        sites: counted(
            gathered
                .reached
                .tests
                .values()
                .flatten()
                .chain(gathered.reached.loose.iter())
                .collect::<BTreeSet<&u32>>()
                .len(),
        ),
        loose: counted(gathered.reached.loose.len()),
        infected: counted(
            gathered
                .infected
                .tests
                .values()
                .flatten()
                .chain(gathered.infected.loose.iter())
                .collect::<BTreeSet<&u32>>()
                .len(),
        ),
    });
    drop(
        touched
            .targets
            .insert(recording.target.to_owned(), gathered),
    );
}

/// A count as the wire carries it.
fn counted(many: usize) -> u32 {
    u32::try_from(many).unwrap_or(u32::MAX)
}

/// What each test of the target reached, with everything else folded into `loose`.
fn attributed(recorded: crate::touch::Seen, ran: &[String]) -> crate::touch::Seen {
    let mut held = crate::touch::Seen {
        loose: recorded.loose,
        ..crate::touch::Seen::default()
    };
    for (thread, reported) in recorded.tests {
        if ran.iter().any(|test| test == &thread) {
            drop(held.tests.insert(thread, reported));
        } else {
            held.loose.extend(reported);
        }
    }
    held
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
    format!("{readable}-{}", digest.get(..16).unwrap_or(&digest))
}
