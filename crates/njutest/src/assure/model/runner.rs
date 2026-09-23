// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Supervised execution of the one pinned Kani result protocol.
//!
//! A process exit is never a proof by itself.
//! This layer pins the tool banner,
//! refuses stale output paths, supervises the whole process tree, retains the raw JSON, and then combines the independently parsed property document with the exact Kani 0.68 exit convention.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::io::Read as _;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rust_mutants::runner::{
    Bound, Cancel, PROBE, PROBE_OUTPUT_LIMIT, ProcessExit, Spec, Termination,
};
use serde::Deserialize;

use super::result::{
    Affirmative, ArtifactFailure, Configuration, Decision, Parsed, ProcessFailure, ToolFailure,
    Undecided,
};
use super::{Harness, KANI_BACKEND_VERSION, KANI_LIST_VERSION, KANI_VERSION};

/// The most exported JSON this version will retain and parse for one harness.
const RAW_LIMIT: u64 = 16 * 1024 * 1024;

/// Diagnostic bytes retained from one Kani invocation.
const OUTPUT_LIMIT: usize = 8 * 1024 * 1024;

/// Cargo configuration is a proof input, not an unbounded data channel.
const CARGO_CONFIG_LIMIT: u64 = 1024 * 1024;

/// Everything whose identity affects one model-checker invocation.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Invocation<'a> {
    /// The absolute `cargo-kani` executable, resolved before subject Cargo aliases can participate in command selection.
    pub kani: &'a Path,
    /// The fresh dependency-free crate containing only the rendered harness.
    pub root: &'a Path,
    /// The exact source file into which the generated harness was written.
    pub source: &'a Path,
    /// The fixed identity of the isolated proof package.
    pub package: &'a str,
    /// The target triple explicitly fixed for measurement and verification.
    pub target: &'a str,
    /// A path that does not yet exist, where Kani must export its raw answer.
    pub artifact: &'a Path,
    /// A dedicated target directory for the private model-checking copy.
    pub target_dir: &'a Path,
    /// The compiler-checked differential harness.
    pub harness: &'a Harness,
    /// The complete environment captured by the composition root.
    pub environment: &'a [(OsString, OsString)],
    /// Where both the version probe and proof invocation are traced.
    pub trace: &'a crate::trace::Recorder,
}

/// The raw artifact retained for independent re-derivation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Artifact {
    /// No complete, regular export was retained at the requested path.
    Absent {
        /// The exact path passed to `--export-json`.
        path: PathBuf,
    },
    /// A complete regular export was read after normal process exit.
    Complete {
        /// The exact path passed to `--export-json`.
        path: PathBuf,
        /// Number of retained raw bytes.
        bytes: u64,
    },
}

impl Artifact {
    /// The exact byte count only for a complete retained result.
    pub(crate) const fn bytes(&self) -> Option<u64> {
        match self {
            Self::Absent { .. } => None,
            Self::Complete { bytes, .. } => Some(*bytes),
        }
    }
}

/// A closed summary of the supervised process boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Process {
    /// Validation or the version probe refused the run before Kani verification.
    NotRun,
    /// Kani exited with this code and its artifact was considered.
    Exited(i32),
    /// The external ceiling expired and the whole process tree was stopped.
    Cutoff,
    /// The caller cancelled the process tree.
    Cancelled,
    /// The process did not supply a supported, trustworthy exit.
    Failed,
}

/// One fail-closed model-checker attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Attempt {
    /// The three-way property decision and its independently hashable evidence.
    parsed: Parsed,
    /// Where the raw answer remains for a separate audit pass.
    artifact: Artifact,
    /// How the supervised verifier process ended.
    process: Process,
}

impl Attempt {
    pub(crate) const fn parsed(&self) -> &Parsed {
        &self.parsed
    }

    pub(crate) const fn artifact(&self) -> &Artifact {
        &self.artifact
    }

    pub(crate) const fn process(&self) -> Process {
        self.process
    }
}

struct DiscoveryInput<'a> {
    environment: &'a [(OsString, OsString)],
    catalog: &'a Path,
    elapsed: Duration,
}

struct Ready {
    environment: Vec<(OsString, OsString)>,
    elapsed: Duration,
}

struct AttemptParts {
    parsed: Parsed,
    process: Process,
    artifact: Retained,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Retained {
    Absent,
    Complete { bytes: u64 },
}

/// Runs Kani only after validating the closed invocation and exact version.
#[must_use]
pub(crate) fn run(invocation: &Invocation<'_>, cancel: &Cancel) -> Attempt {
    let ready = match prepare(invocation, cancel) {
        ControlFlow::Continue(ready) => ready,
        ControlFlow::Break(answer) => return answer,
    };
    checked_after(invocation, verify(invocation, ready, cancel))
}

fn prepare(invocation: &Invocation<'_>, cancel: &Cancel) -> ControlFlow<Attempt, Ready> {
    if let Some(why) = invalid(invocation) {
        return ControlFlow::Break(refused(invocation, Undecided::Configuration(why)));
    }
    if !source_matches(invocation) {
        return ControlFlow::Break(refused(
            invocation,
            Undecided::Artifact(ArtifactFailure::SourceChanged),
        ));
    }
    match std::fs::symlink_metadata(invocation.artifact) {
        Ok(_metadata) => {
            return ControlFlow::Break(refused(
                invocation,
                Undecided::Artifact(ArtifactFailure::AlreadyExists),
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_error) => {
            return ControlFlow::Break(refused(
                invocation,
                Undecided::Artifact(ArtifactFailure::Unreadable),
            ));
        }
    }
    let catalog = invocation.root.join("kani-list.json");
    match std::fs::symlink_metadata(&catalog) {
        Ok(_metadata) => {
            return ControlFlow::Break(refused(
                invocation,
                Undecided::Tool(ToolFailure::HarnessListArtifact),
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_error) => {
            return ControlFlow::Break(refused(
                invocation,
                Undecided::Tool(ToolFailure::HarnessListArtifact),
            ));
        }
    }

    let Some(environment) = environment(invocation) else {
        return ControlFlow::Break(refused(
            invocation,
            Undecided::Configuration(Configuration::CompilerEnvironment),
        ));
    };
    let version_duration = match probe(invocation, &environment, cancel) {
        ControlFlow::Continue(duration) => duration,
        ControlFlow::Break(answer) => return ControlFlow::Break(answer),
    };
    if !model_crate_matches(invocation) {
        return ControlFlow::Break(refused(
            invocation,
            Undecided::Configuration(Configuration::WorkspaceDrift),
        ));
    }
    let discovery = DiscoveryInput {
        environment: &environment,
        catalog: &catalog,
        elapsed: version_duration,
    };
    let ready = match discover(invocation, &discovery, cancel) {
        ControlFlow::Continue(duration) => Ready {
            environment,
            elapsed: duration,
        },
        ControlFlow::Break(answer) => return ControlFlow::Break(answer),
    };
    if !model_crate_matches(invocation) {
        return ControlFlow::Break(refused(
            invocation,
            Undecided::Configuration(Configuration::WorkspaceDrift),
        ));
    }
    ControlFlow::Continue(ready)
}

fn checked_after(invocation: &Invocation<'_>, mut answer: Attempt) -> Attempt {
    if !source_matches(invocation) {
        answer.parsed = super::result::undecided(
            &[],
            invocation.harness,
            Undecided::Artifact(ArtifactFailure::SourceChanged),
        );
    } else if !model_crate_matches(invocation) {
        answer.parsed = super::result::undecided(
            &[],
            invocation.harness,
            Undecided::Configuration(Configuration::WorkspaceDrift),
        );
    }
    answer
}

/// Constructs the same retained attempt shape when a pre-execution configuration fact makes invoking Kani unsound.
pub(super) fn refused_configuration(
    harness: &Harness,
    artifact: &Path,
    why: Configuration,
) -> Attempt {
    Attempt {
        parsed: super::result::undecided(&[], harness, Undecided::Configuration(why)),
        artifact: Artifact::Absent {
            path: artifact.to_path_buf(),
        },
        process: Process::NotRun,
    }
}

fn probe(
    invocation: &Invocation<'_>,
    environment: &[(OsString, OsString)],
    cancel: &Cancel,
) -> ControlFlow<Attempt, Duration> {
    let mut version_spec = Spec::new(
        [
            invocation.kani.as_os_str(),
            std::ffi::OsStr::new("kani"),
            std::ffi::OsStr::new("--version"),
        ],
        Bound::After(PROBE.min(invocation.harness.timeout())),
    );
    version_spec.dir = Some(invocation.root.to_path_buf());
    version_spec.env = Some(environment.to_vec());
    version_spec.output_limit = Some(PROBE_OUTPUT_LIMIT);
    version_spec.structured_stdout = Some(PROBE_OUTPUT_LIMIT);
    let version = rust_mutants::runner::run(&version_spec, cancel);
    invocation
        .trace
        .exec_result(crate::trace::ExecRecord::of(&version_spec, &version));
    let version_duration = version.duration;
    let version_failure = version_failure(&version);
    if let Some(why) = version_failure {
        let (decision, process) = match why {
            VersionFailure::Cutoff => (Undecided::Cutoff, Process::Cutoff),
            VersionFailure::Cancelled => (Undecided::Cancelled, Process::Cancelled),
            VersionFailure::Tool(why) => (Undecided::Tool(why), Process::NotRun),
            VersionFailure::Process(why) => (Undecided::Process(why), Process::Failed),
        };
        return ControlFlow::Break(attempt(
            invocation,
            AttemptParts {
                parsed: super::result::undecided(&[], invocation.harness, decision),
                process,
                artifact: Retained::Absent,
            },
        ));
    }
    ControlFlow::Continue(version_duration)
}

fn discover(
    invocation: &Invocation<'_>,
    input: &DiscoveryInput<'_>,
    cancel: &Cancel,
) -> ControlFlow<Attempt, Duration> {
    let Some(ceiling) = remaining(invocation.harness.timeout(), input.elapsed) else {
        return ControlFlow::Break(cutoff(invocation));
    };
    let catalog_arguments = [
        invocation.kani.as_os_str().to_owned(),
        OsString::from("kani"),
        OsString::from("list"),
        OsString::from("--format"),
        OsString::from("json"),
    ];
    let mut spec = Spec::new(catalog_arguments, Bound::After(ceiling));
    spec.dir = Some(invocation.root.to_path_buf());
    spec.env = Some(input.environment.to_vec());
    spec.output_limit = Some(OUTPUT_LIMIT);
    let ran = rust_mutants::runner::run(&spec, cancel);
    invocation
        .trace
        .exec_result(crate::trace::ExecRecord::of(&spec, &ran));
    let Some(duration) = input.elapsed.checked_add(ran.duration) else {
        return ControlFlow::Break(cutoff(invocation));
    };
    match ran.termination {
        Termination::TimedOut => ControlFlow::Break(cutoff(invocation)),
        Termination::Cancelled { .. } => ControlFlow::Break(attempt(
            invocation,
            AttemptParts {
                parsed: super::result::undecided(&[], invocation.harness, Undecided::Cancelled),
                process: Process::Cancelled,
                artifact: Retained::Absent,
            },
        )),
        Termination::Exited(ProcessExit::Code(0)) => {
            if let Err(why) = catalog_harness(input.catalog, invocation.harness) {
                return ControlFlow::Break(attempt(
                    invocation,
                    AttemptParts {
                        parsed: super::result::undecided(
                            &[],
                            invocation.harness,
                            Undecided::Tool(why),
                        ),
                        process: Process::NotRun,
                        artifact: Retained::Absent,
                    },
                ));
            }
            ControlFlow::Continue(duration)
        }
        Termination::NotStarted { .. }
        | Termination::StoppedByMonitor
        | Termination::Stalled
        | Termination::WaitFailed { .. }
        | Termination::Exited(
            ProcessExit::Signal(_) | ProcessExit::Unknown | ProcessExit::Code(_),
        ) => discovery_refusal(invocation, ToolFailure::HarnessListCommand),
        Termination::MonitorFailed { .. } => {
            ControlFlow::Break(failed_process(invocation, ProcessFailure::Monitor))
        }
    }
}

fn discovery_refusal(
    invocation: &Invocation<'_>,
    why: ToolFailure,
) -> ControlFlow<Attempt, Duration> {
    ControlFlow::Break(attempt(
        invocation,
        AttemptParts {
            parsed: super::result::undecided(&[], invocation.harness, Undecided::Tool(why)),
            process: Process::NotRun,
            artifact: Retained::Absent,
        },
    ))
}

fn verify(invocation: &Invocation<'_>, ready: Ready, cancel: &Cancel) -> Attempt {
    let Some(ceiling) = remaining(invocation.harness.timeout(), ready.elapsed) else {
        return cutoff(invocation);
    };
    let mut spec = Spec::new(
        arguments(invocation, invocation.harness.harness_name()),
        Bound::After(ceiling),
    );
    spec.dir = Some(invocation.root.to_path_buf());
    spec.env = Some(ready.environment);
    spec.output_limit = Some(OUTPUT_LIMIT);
    let ran = rust_mutants::runner::run(&spec, cancel);
    invocation
        .trace
        .exec_result(crate::trace::ExecRecord::of(&spec, &ran));
    terminated(invocation, &ran.termination)
}

fn terminated(invocation: &Invocation<'_>, termination: &Termination) -> Attempt {
    match termination {
        Termination::TimedOut => attempt(
            invocation,
            AttemptParts {
                parsed: super::result::undecided(&[], invocation.harness, Undecided::Cutoff),
                process: Process::Cutoff,
                artifact: Retained::Absent,
            },
        ),
        Termination::Cancelled { .. } => attempt(
            invocation,
            AttemptParts {
                parsed: super::result::undecided(&[], invocation.harness, Undecided::Cancelled),
                process: Process::Cancelled,
                artifact: Retained::Absent,
            },
        ),
        Termination::NotStarted { .. } => failed_process(invocation, ProcessFailure::NotStarted),
        Termination::StoppedByMonitor | Termination::Stalled => {
            failed_process(invocation, ProcessFailure::Stopped)
        }
        Termination::MonitorFailed { .. } => failed_process(invocation, ProcessFailure::Monitor),
        Termination::WaitFailed { .. } => failed_process(invocation, ProcessFailure::Wait),
        Termination::Exited(ProcessExit::Signal(_signal)) => {
            failed_process(invocation, ProcessFailure::Signal)
        }
        Termination::Exited(ProcessExit::Unknown) => {
            failed_process(invocation, ProcessFailure::UnknownExit)
        }
        Termination::Exited(ProcessExit::Code(code)) => completed(invocation, *code),
    }
}

fn invalid(invocation: &Invocation<'_>) -> Option<Configuration> {
    if invocation.package != super::MODEL_PACKAGE {
        return Some(Configuration::Package);
    }
    if invocation
        .environment
        .iter()
        .any(|(name, _value)| compiler_flags_key(name))
    {
        return Some(Configuration::CompilerFlags);
    }
    if invocation
        .environment
        .iter()
        .any(|(name, _value)| compiler_environment_key(name))
    {
        return Some(Configuration::CompilerEnvironment);
    }
    if invocation
        .environment
        .iter()
        .any(|(name, _value)| profile_environment_key(name))
    {
        return Some(Configuration::Profile);
    }
    if !cargo_configuration_safe(invocation.root, invocation.environment) {
        return Some(Configuration::CompilerEnvironment);
    }
    if !invocation.kani.is_absolute()
        || !invocation.source.is_absolute()
        || !invocation.artifact.is_absolute()
        || !invocation.target_dir.is_absolute()
    {
        return Some(Configuration::RelativePath);
    }
    if !checker_boundary(invocation)
        || !verifier_bundle_boundary(invocation.environment)
        || !directory(invocation.root)
        || !invocation.artifact.parent().is_some_and(directory)
        || !invocation.target_dir.parent().is_some_and(directory)
        || !fresh_directory(invocation.target_dir)
        || environment(invocation).is_none()
    {
        return Some(Configuration::Directory);
    }
    if invocation.source != invocation.root.join(super::MODEL_SOURCE_PATH)
        || !model_crate_matches(invocation)
    {
        return Some(Configuration::WorkspaceDrift);
    }
    None
}

fn cargo_configuration_safe(root: &Path, environment: &[(OsString, OsString)]) -> bool {
    let mut configurations = Vec::new();
    let canonical_root = match std::fs::canonicalize(root) {
        Ok(root) => root,
        Err(_error) => return false,
    };
    for ancestor in canonical_root.ancestors() {
        configurations.push((ancestor.to_path_buf(), PathBuf::from(".cargo/config")));
        configurations.push((ancestor.to_path_buf(), PathBuf::from(".cargo/config.toml")));
    }
    let cargo_home = match unique_environment_value(environment, "CARGO_HOME") {
        Ok(Some(home)) => PathBuf::from(home),
        Ok(None) | Err(EnvironmentLookupError::Duplicate { .. }) => return false,
    };
    if !cargo_home.is_absolute() || !directory(&cargo_home) {
        return false;
    }
    configurations.push((cargo_home.clone(), PathBuf::from("config")));
    configurations.push((cargo_home, PathBuf::from("config.toml")));
    configurations
        .into_iter()
        .all(|(boundary, relative)| cargo_configuration_file_safe(&boundary, &relative))
}

fn cargo_configuration_file_safe(boundary: &Path, relative: &Path) -> bool {
    let path = boundary.join(relative);
    let bytes = match read_regular_beneath(boundary, relative, CARGO_CONFIG_LIMIT, true) {
        Ok(bytes) => bytes,
        Err(ReadFailure::Missing) => return true,
        Err(
            ReadFailure::NotFile | ReadFailure::TooLarge | ReadFailure::Empty | ReadFailure::Io,
        ) => return false,
    };
    let Some(parent) = path.parent() else {
        return false;
    };
    let parent_is_directory = match std::fs::symlink_metadata(parent) {
        Ok(metadata) => metadata.file_type().is_dir(),
        Err(_error) => false,
    };
    if !parent_is_directory {
        return false;
    }
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return false;
    };
    let Ok(document) = toml::from_str::<toml::Value>(text) else {
        return false;
    };
    document.as_table().is_some_and(cargo_document_safe)
}

fn cargo_document_safe(document: &toml::Table) -> bool {
    document.iter().all(|(name, value)| match name.as_str() {
        "build" => value.as_table().is_some_and(toml::Table::is_empty),
        "alias" => value.as_table().is_some_and(|table| {
            table.iter().all(|(alias, command)| {
                alias == "xtask"
                    && command.as_str() == Some("run --quiet --locked --package xtask --")
            })
        }),
        "net" => value.as_table().is_some_and(|table| {
            table.len() == 1 && table.get("retry").and_then(toml::Value::as_integer) == Some(10)
        }),
        "term" => value.as_table().is_some_and(|table| {
            table.len() == 1 && table.get("verbose").and_then(toml::Value::as_bool) == Some(false)
        }),
        _ => false,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
enum EnvironmentLookupError {
    #[error("the environment contains more than one case-insensitive {name} entry")]
    Duplicate { name: &'static str },
}

fn unique_environment_value<'a>(
    environment: &'a [(OsString, OsString)],
    name: &'static str,
) -> Result<Option<&'a std::ffi::OsStr>, EnvironmentLookupError> {
    let mut values = environment
        .iter()
        .filter(|(candidate, _value)| environment_key(candidate, name))
        .map(|(_candidate, value)| value.as_os_str());
    let first = values.next();
    if values.next().is_some() {
        return Err(EnvironmentLookupError::Duplicate { name });
    }
    Ok(first)
}

fn checker_boundary(invocation: &Invocation<'_>) -> bool {
    let cargo_home = match unique_environment_value(invocation.environment, "CARGO_HOME") {
        Ok(Some(home)) => PathBuf::from(home),
        Ok(None) | Err(EnvironmentLookupError::Duplicate { .. }) => return false,
    };
    let executable = if cfg!(windows) {
        "cargo-kani.exe"
    } else {
        "cargo-kani"
    };
    let relative = Path::new("bin").join(executable);
    if !cargo_home.is_absolute()
        || !directory(&cargo_home)
        || invocation.kani != cargo_home.join(&relative)
    {
        return false;
    }
    let file = match open_beneath(&cargo_home, &relative) {
        Ok(file) => file,
        Err(_error) => return false,
    };
    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(_error) => return false,
    };
    if !metadata.file_type().is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn verifier_bundle_boundary(environment: &[(OsString, OsString)]) -> bool {
    let home_name = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    let home = match unique_environment_value(environment, home_name) {
        Ok(Some(home)) => PathBuf::from(home),
        Ok(None) | Err(EnvironmentLookupError::Duplicate { .. }) => return false,
    };
    if !home.is_absolute() || !directory(&home) {
        return false;
    }
    let version_root = Path::new(".kani").join(format!("kani-{KANI_VERSION}"));
    let version = match open_beneath(&home, &version_root) {
        Ok(version) => version,
        Err(_error) => return false,
    };
    if !matches!(version.metadata(), Ok(metadata) if metadata.file_type().is_dir()) {
        return false;
    }
    [
        "kani-compiler",
        "kani-driver",
        "cbmc",
        "goto-cc",
        "goto-instrument",
    ]
    .into_iter()
    .all(|name| {
        let relative = version_root.join("bin").join(name);
        match open_beneath(&home, &relative).and_then(|file| file.metadata()) {
            Ok(metadata) => metadata.file_type().is_file(),
            Err(_error) => false,
        }
    })
}

fn directory(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata.file_type().is_dir(),
        Err(_error) => false,
    }
}

fn fresh_directory(path: &Path) -> bool {
    if !directory(path) {
        return false;
    }
    let Ok(mut entries) = std::fs::read_dir(path) else {
        return false;
    };
    match entries.next() {
        None => true,
        Some(_entry) => false,
    }
}

fn source_matches(invocation: &Invocation<'_>) -> bool {
    if !invocation.source.starts_with(invocation.root) {
        return false;
    }
    let expected = invocation.harness.source().as_bytes();
    let Ok(expected_len) = u64::try_from(expected.len()) else {
        return false;
    };
    let Some(limit) = expected_len.checked_add(1) else {
        return false;
    };
    let relative = match invocation.source.strip_prefix(invocation.root) {
        Ok(relative) => relative,
        Err(_outside_root) => return false,
    };
    match read_regular_beneath(invocation.root, relative, limit, false) {
        Ok(source) => source == expected,
        Err(_failure) => false,
    }
}

fn model_crate_matches(invocation: &Invocation<'_>) -> bool {
    let manifest_limit = match u64::try_from(super::MODEL_MANIFEST.len()) {
        Ok(limit) => limit,
        Err(_overflow) => return false,
    };
    let lock_limit = match u64::try_from(super::MODEL_LOCK.len()) {
        Ok(limit) => limit,
        Err(_overflow) => return false,
    };
    let manifest = read_regular_beneath(
        invocation.root,
        Path::new("Cargo.toml"),
        manifest_limit,
        false,
    );
    let lock = read_regular_beneath(invocation.root, Path::new("Cargo.lock"), lock_limit, false);
    matches!(manifest, Ok(bytes) if bytes == super::MODEL_MANIFEST.as_bytes())
        && matches!(lock, Ok(bytes) if bytes == super::MODEL_LOCK.as_bytes())
        && exact_entries(invocation.root, &["Cargo.lock", "Cargo.toml", "src"])
        && exact_entries(&invocation.root.join("src"), &["lib.rs"])
        && source_matches(invocation)
}

fn exact_entries(directory: &Path, expected: &[&str]) -> bool {
    let mut entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(_error) => return false,
    };
    let mut actual = BTreeSet::new();
    loop {
        match entries.next() {
            Some(Ok(entry)) => {
                actual.insert(entry.file_name());
            }
            Some(Err(_error)) => return false,
            None => break,
        }
    }
    let expected: BTreeSet<OsString> = expected.iter().map(OsString::from).collect();
    actual == expected
}

fn environment(invocation: &Invocation<'_>) -> Option<Vec<(OsString, OsString)>> {
    let cargo_home = match unique_environment_value(invocation.environment, "CARGO_HOME") {
        Ok(Some(cargo_home)) => PathBuf::from(cargo_home),
        Ok(None) | Err(EnvironmentLookupError::Duplicate { .. }) => return None,
    };
    let home_name = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    let home = match unique_environment_value(invocation.environment, home_name) {
        Ok(Some(home)) => PathBuf::from(home),
        Ok(None) | Err(EnvironmentLookupError::Duplicate { .. }) => return None,
    };
    if !cargo_home.is_absolute()
        || !home.is_absolute()
        || !directory(&cargo_home)
        || !directory(&home)
    {
        return None;
    }
    #[cfg(unix)]
    let mut path_entries = vec![cargo_home.join("bin")];
    #[cfg(unix)]
    path_entries.extend([PathBuf::from("/usr/bin"), PathBuf::from("/bin")]);
    #[cfg(not(unix))]
    let path_entries = vec![cargo_home.join("bin")];
    let path = match std::env::join_paths(path_entries) {
        Ok(path) => path,
        Err(_invalid_path) => return None,
    };
    let kani_home = home.join(".kani");
    let mut environment = vec![
        (OsString::from("CARGO_HOME"), cargo_home.into_os_string()),
        (OsString::from(home_name), home.into_os_string()),
        (OsString::from("KANI_HOME"), kani_home.into_os_string()),
        (OsString::from("PATH"), path),
        (OsString::from("CARGO_ENCODED_RUSTFLAGS"), OsString::new()),
        (OsString::from("RUSTC_WRAPPER"), OsString::new()),
        (OsString::from("RUSTC_WORKSPACE_WRAPPER"), OsString::new()),
        (
            OsString::from("CARGO_TARGET_DIR"),
            invocation.target_dir.as_os_str().to_owned(),
        ),
        (
            OsString::from("CARGO_BUILD_TARGET"),
            OsString::from(invocation.target),
        ),
        (OsString::from("CARGO_NET_OFFLINE"), OsString::from("true")),
    ];
    #[cfg(unix)]
    environment.push((
        OsString::from("TMPDIR"),
        invocation.target_dir.as_os_str().to_owned(),
    ));
    #[cfg(windows)]
    {
        let temporary = invocation.target_dir.as_os_str().to_owned();
        environment.push((OsString::from("TEMP"), temporary.clone()));
        environment.push((OsString::from("TMP"), temporary));
        let system_root = match unique_environment_value(invocation.environment, "SYSTEMROOT") {
            Ok(Some(system_root)) => system_root.to_owned(),
            Ok(None) | Err(EnvironmentLookupError::Duplicate { .. }) => return None,
        };
        environment.push((OsString::from("SYSTEMROOT"), system_root));
    }
    #[cfg(test)]
    environment.extend(
        invocation
            .environment
            .iter()
            .filter(|(name, _value)| {
                name.to_str()
                    .is_some_and(|name| name.starts_with("FAKE_KANI_"))
            })
            .cloned(),
    );
    Some(environment)
}

fn remaining(ceiling: Duration, elapsed: Duration) -> Option<Duration> {
    ceiling.checked_sub(elapsed).filter(|left| !left.is_zero())
}

fn environment_key(actual: &std::ffi::OsStr, expected: &str) -> bool {
    actual
        .to_str()
        .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
}

fn compiler_flags_key(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    let upper = name.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "RUSTFLAGS" | "CARGO_ENCODED_RUSTFLAGS" | "CARGO_BUILD_RUSTFLAGS"
    ) || (upper.starts_with("CARGO_TARGET_") && upper.ends_with("_RUSTFLAGS"))
}

fn compiler_environment_key(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    matches!(
        name.to_ascii_uppercase().as_str(),
        "RUSTC"
            | "RUSTC_WRAPPER"
            | "RUSTC_WORKSPACE_WRAPPER"
            | "RUSTC_BOOTSTRAP"
            | "CARGO_BUILD_RUSTC"
            | "CARGO_BUILD_RUSTC_WRAPPER"
            | "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER"
    )
}

fn profile_environment_key(name: &std::ffi::OsStr) -> bool {
    name.to_str()
        .is_some_and(|name| name.to_ascii_uppercase().starts_with("CARGO_PROFILE_"))
}

fn arguments(invocation: &Invocation<'_>, harness: &str) -> Vec<OsString> {
    let mut arguments = vec![
        invocation.kani.as_os_str().to_owned(),
        OsString::from("kani"),
        OsString::from("--package"),
        OsString::from(invocation.package),
    ];
    arguments.extend([
        OsString::from("--jobs"),
        OsString::from("1"),
        OsString::from("--target-dir"),
        invocation.target_dir.as_os_str().to_owned(),
        OsString::from("--harness"),
        OsString::from(harness),
        OsString::from("--exact"),
        OsString::from("--solver"),
        OsString::from(super::KANI_SOLVER),
        OsString::from("-Z"),
        OsString::from("unstable-options"),
        OsString::from("--export-json"),
        invocation.artifact.as_os_str().to_owned(),
    ]);
    arguments
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    #[serde(rename = "kani-version")]
    kani_version: String,
    #[serde(rename = "file-version")]
    file_version: String,
    #[serde(rename = "standard-harnesses")]
    standard_harnesses: BTreeMap<String, Vec<String>>,
    #[serde(rename = "contract-harnesses")]
    contract_harnesses: BTreeMap<String, Vec<String>>,
    contracts: Vec<serde_json::Value>,
    totals: CatalogTotals,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogTotals {
    #[serde(rename = "standard-harnesses")]
    standard_harnesses: u64,
    #[serde(rename = "contract-harnesses")]
    contract_harnesses: u64,
    #[serde(rename = "functions-under-contract")]
    functions_under_contract: u64,
}

fn catalog_harness(path: &Path, expected: &Harness) -> Result<(), ToolFailure> {
    let raw = read_artifact(path).map_err(|_why| ToolFailure::HarnessListArtifact)?;
    std::fs::remove_file(path).map_err(|_error| ToolFailure::HarnessListArtifact)?;
    let value =
        crate::strictjson::from_slice(&raw).map_err(|_error| ToolFailure::HarnessListSchema)?;
    let catalog: Catalog =
        serde_json::from_value(value).map_err(|_error| ToolFailure::HarnessListSchema)?;
    let expected_name = expected.harness_name();
    if catalog.kani_version != KANI_VERSION
        || catalog.file_version != KANI_LIST_VERSION
        || catalog.totals.standard_harnesses != 1
        || catalog.totals.contract_harnesses != 0
        || catalog.totals.functions_under_contract != 0
        || !catalog.contract_harnesses.is_empty()
        || !catalog.contracts.is_empty()
        || catalog.standard_harnesses.len() != 1
        || !matches!(
            catalog.standard_harnesses.get(super::MODEL_SOURCE_PATH),
            Some(harnesses) if harnesses.as_slice() == [expected_name]
        )
    {
        return Err(ToolFailure::HarnessListMatch);
    }
    Ok(())
}

enum VersionFailure {
    Cutoff,
    Cancelled,
    Tool(ToolFailure),
    Process(ProcessFailure),
}

fn version_failure(ran: &rust_mutants::runner::RunResult) -> Option<VersionFailure> {
    match ran.termination {
        Termination::TimedOut => return Some(VersionFailure::Cutoff),
        Termination::Cancelled { .. } => return Some(VersionFailure::Cancelled),
        Termination::NotStarted { .. } | Termination::WaitFailed { .. } => {
            return Some(VersionFailure::Tool(ToolFailure::Unavailable));
        }
        Termination::StoppedByMonitor
        | Termination::Stalled
        | Termination::Exited(
            ProcessExit::Signal(_) | ProcessExit::Unknown | ProcessExit::Code(1.. | ..0),
        ) => {
            return Some(VersionFailure::Tool(ToolFailure::VersionCommand));
        }
        Termination::MonitorFailed { .. } => {
            return Some(VersionFailure::Process(ProcessFailure::Monitor));
        }
        Termination::Exited(ProcessExit::Code(0)) => {}
    }
    let expected =
        format!("Kani Rust Verifier {KANI_VERSION} (cargo plugin)\nCBMC {KANI_BACKEND_VERSION}\n");
    let Some(normalized) = normalize_newlines(&ran.stdout) else {
        return Some(VersionFailure::Tool(ToolFailure::VersionBanner));
    };
    if ran.stdout_truncated || normalized != expected.as_bytes() || !ran.output.is_empty() {
        return Some(VersionFailure::Tool(ToolFailure::VersionBanner));
    }
    None
}

fn normalize_newlines(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut normalized = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes
            .get(index..)
            .is_some_and(|remaining| remaining.starts_with(b"\r\n"))
        {
            normalized.push(b'\n');
            index = index.checked_add(2)?;
        } else if let Some(byte) = bytes.get(index) {
            normalized.push(*byte);
            index = index.checked_add(1)?;
        }
    }
    Some(normalized)
}

fn completed(invocation: &Invocation<'_>, code: i32) -> Attempt {
    let raw = match read_artifact(invocation.artifact) {
        Ok(raw) => raw,
        Err(why) => {
            return attempt(
                invocation,
                AttemptParts {
                    parsed: super::result::undecided(
                        &[],
                        invocation.harness,
                        Undecided::Artifact(why),
                    ),
                    process: Process::Exited(code),
                    artifact: Retained::Absent,
                },
            );
        }
    };
    let Ok(raw_len) = u64::try_from(raw.len()) else {
        return attempt(
            invocation,
            AttemptParts {
                parsed: super::result::undecided(
                    &raw,
                    invocation.harness,
                    Undecided::Artifact(ArtifactFailure::TooLarge),
                ),
                process: Process::Exited(code),
                artifact: Retained::Absent,
            },
        );
    };
    let mut parsed = super::result::parse(
        &raw,
        super::result::Expectation {
            harness: invocation.harness,
            target: invocation.target,
            root: invocation.root,
            target_dir: invocation.target_dir,
            package: invocation.package,
        },
    );
    parsed.decision = match (&parsed.decision, code) {
        (Decision::Proved, 0) | (Decision::Noticed, 1) | (Decision::Undecided(_), 0 | 1) => {
            parsed.decision
        }
        (Decision::Proved, actual) => Decision::Undecided(Undecided::ExitMismatch {
            decision: Affirmative::Proved,
            actual,
        }),
        (Decision::Noticed, actual) => Decision::Undecided(Undecided::ExitMismatch {
            decision: Affirmative::Noticed,
            actual,
        }),
        (Decision::Undecided(_), _actual) => {
            Decision::Undecided(Undecided::Process(ProcessFailure::UnexpectedExit))
        }
    };
    attempt(
        invocation,
        AttemptParts {
            parsed,
            process: Process::Exited(code),
            artifact: Retained::Complete { bytes: raw_len },
        },
    )
}

fn read_artifact(path: &Path) -> Result<Vec<u8>, ArtifactFailure> {
    read_regular_nofollow(path, RAW_LIMIT, false).map_err(|failure| match failure {
        ReadFailure::Missing => ArtifactFailure::Missing,
        ReadFailure::NotFile => ArtifactFailure::NotFile,
        ReadFailure::TooLarge | ReadFailure::Empty => ArtifactFailure::TooLarge,
        ReadFailure::Io => ArtifactFailure::Unreadable,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadFailure {
    Missing,
    NotFile,
    TooLarge,
    Empty,
    Io,
}

fn read_regular_nofollow(
    path: &Path,
    limit: u64,
    empty_allowed: bool,
) -> Result<Vec<u8>, ReadFailure> {
    let Some(parent) = path.parent() else {
        return Err(ReadFailure::Io);
    };
    let Some(name) = path.file_name() else {
        return Err(ReadFailure::Io);
    };
    read_regular_beneath(parent, Path::new(name), limit, empty_allowed)
}

fn read_regular_beneath(
    boundary: &Path,
    relative: &Path,
    limit: u64,
    empty_allowed: bool,
) -> Result<Vec<u8>, ReadFailure> {
    let mut file = open_beneath(boundary, relative).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ReadFailure::Missing
        } else {
            ReadFailure::Io
        }
    })?;
    let before = file.metadata().map_err(|_error| ReadFailure::Io)?;
    if !before.file_type().is_file() {
        return Err(ReadFailure::NotFile);
    }
    if before.len() == 0 && !empty_allowed {
        return Err(ReadFailure::Empty);
    }
    if before.len() > limit {
        return Err(ReadFailure::TooLarge);
    }
    let Some(read_limit) = limit.checked_add(1) else {
        return Err(ReadFailure::TooLarge);
    };
    let mut raw = Vec::new();
    file.by_ref()
        .take(read_limit)
        .read_to_end(&mut raw)
        .map_err(|_error| ReadFailure::Io)?;
    let after = file.metadata().map_err(|_error| ReadFailure::Io)?;
    let length_matches = match u64::try_from(raw.len()) {
        Ok(length) => length <= limit && length == after.len(),
        Err(_overflow) => false,
    };
    if before.len() != after.len() || !length_matches {
        return Err(ReadFailure::TooLarge);
    }
    Ok(raw)
}

#[cfg(unix)]
fn open_beneath(boundary: &Path, relative: &Path) -> std::io::Result<std::fs::File> {
    use rustix::fs::{Mode, OFlags};

    let mut directory = rustix::fs::open(
        boundary,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::DIRECTORY | OFlags::NOFOLLOW,
        Mode::empty(),
    )?;
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let std::path::Component::Normal(name) = component else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "proof input path is not canonical beneath its boundary",
            ));
        };
        if components.peek().is_some() {
            directory = rustix::fs::openat(
                &directory,
                name,
                OFlags::RDONLY | OFlags::CLOEXEC | OFlags::DIRECTORY | OFlags::NOFOLLOW,
                Mode::empty(),
            )?;
            continue;
        }
        let descriptor = rustix::fs::openat(
            &directory,
            name,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )?;
        return Ok(std::fs::File::from(descriptor));
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "proof input path is empty",
    ))
}

#[cfg(windows)]
fn open_beneath(boundary: &Path, relative: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    let boundary_metadata = std::fs::symlink_metadata(boundary)?;
    if !boundary_metadata.file_type().is_dir()
        || boundary_metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "proof input boundary is not one real directory",
        ));
    }
    let mut path = boundary.to_path_buf();
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let std::path::Component::Normal(name) = component else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "proof input path is not canonical beneath its boundary",
            ));
        };
        path.push(name);
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || (components.peek().is_some() && !metadata.file_type().is_dir())
            || (components.peek().is_none() && !metadata.file_type().is_file())
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "proof input path crosses a reparse point or non-file entry",
            ));
        }
    }
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

#[cfg(not(any(unix, windows)))]
fn open_beneath(boundary: &Path, relative: &Path) -> std::io::Result<std::fs::File> {
    let boundary_metadata = std::fs::symlink_metadata(boundary)?;
    if !boundary_metadata.file_type().is_dir() || boundary_metadata.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "proof input boundary is not one real directory",
        ));
    }
    let mut path = boundary.to_path_buf();
    for component in relative.components() {
        let std::path::Component::Normal(name) = component else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "proof input path is not canonical beneath its boundary",
            ));
        };
        path.push(name);
        if std::fs::symlink_metadata(&path)?.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "proof input path crosses a symlink",
            ));
        }
    }
    std::fs::File::open(path)
}

fn failed_process(invocation: &Invocation<'_>, why: ProcessFailure) -> Attempt {
    attempt(
        invocation,
        AttemptParts {
            parsed: super::result::undecided(&[], invocation.harness, Undecided::Process(why)),
            process: Process::Failed,
            artifact: Retained::Absent,
        },
    )
}

fn cutoff(invocation: &Invocation<'_>) -> Attempt {
    attempt(
        invocation,
        AttemptParts {
            parsed: super::result::undecided(&[], invocation.harness, Undecided::Cutoff),
            process: Process::Cutoff,
            artifact: Retained::Absent,
        },
    )
}

fn refused(invocation: &Invocation<'_>, why: Undecided) -> Attempt {
    attempt(
        invocation,
        AttemptParts {
            parsed: super::result::undecided(&[], invocation.harness, why),
            process: Process::NotRun,
            artifact: Retained::Absent,
        },
    )
}

fn attempt(invocation: &Invocation<'_>, parts: AttemptParts) -> Attempt {
    Attempt {
        parsed: parts.parsed,
        artifact: match parts.artifact {
            Retained::Absent => Artifact::Absent {
                path: invocation.artifact.to_path_buf(),
            },
            Retained::Complete { bytes } => Artifact::Complete {
                path: invocation.artifact.to_path_buf(),
                bytes,
            },
        },
        process: parts.process,
    }
}

/// The runner's tests, which drive a cargo double that is a shell script and a mode bit.
#[cfg(test)]
#[cfg(unix)]
mod tests {
    #![expect(
        clippy::disallowed_methods,
        clippy::indexing_slicing,
        reason = "runner tests fail loudly while constructing private executable fixtures and exact JSON records"
    )]

    use serde_json::json;
    use std::path::Path;

    #[cfg(unix)]
    use super::Artifact;
    #[cfg(unix)]
    use super::ArtifactFailure;
    use super::{Attempt, Invocation, Process, ToolFailure, Undecided, run};
    use crate::assure::model::result::Decision;
    #[cfg(unix)]
    use crate::assure::model::result::Protocol;
    use crate::assure::model::tests::{generated, simple_source};
    use crate::assure::model::{
        Harness, KANI_CBMC_VERSION, KANI_GOTO_CC_VERSION, KANI_GOTO_INSTRUMENT_VERSION,
        KANI_RUSTC_VERSION, KANI_SOLVER, KANI_VERSION, MODEL_LOCK, MODEL_MANIFEST, MODEL_PACKAGE,
        MODEL_SOURCE_PATH,
    };

    struct Fixture {
        temporary: tempfile::TempDir,
        root: std::path::PathBuf,
        source: std::path::PathBuf,
        cargo: std::path::PathBuf,
        artifact: std::path::PathBuf,
        #[cfg(unix)]
        argv: std::path::PathBuf,
        target: std::path::PathBuf,
        harness: Harness,
        environment: Vec<(std::ffi::OsString, std::ffi::OsString)>,
        trace: crate::trace::Recorder,
    }

    impl Fixture {
        /// Whether an invocation carrying this variable is refused, whichever of the three refusals it earns.
        ///
        /// The fixture takes the ambient environment for `PATH` and the rest,
        /// and stripped only the compiler-*environment* keys.
        /// A machine or a CI job that exports `RUSTFLAGS` therefore earned the earlier `CompilerFlags` refusal, and thirty-one tests that set one variable each asserted a refusal about a different one.
        /// Derived from the three predicates, so a fourth cleans the fixture on the day it is written.
        fn refused_key(name: &std::ffi::OsStr) -> bool {
            super::compiler_flags_key(name)
                || super::compiler_environment_key(name)
                || super::profile_environment_key(name)
        }

        fn new(status: &str, exit: i32) -> Self {
            let temporary = tempfile::tempdir().expect("temporary fake Kani tree");
            let root = temporary.path().join("root");
            std::fs::create_dir_all(root.join("src")).expect("working directory");
            let cargo_home = temporary.path().join("cargo-home");
            std::fs::create_dir_all(cargo_home.join("bin")).expect("isolated Cargo bin");
            let cargo = cargo_home.join("bin/cargo-kani");
            fake_cargo(&cargo);
            let home = temporary.path().join("home");
            let bundle = home.join(format!(".kani/kani-{KANI_VERSION}/bin"));
            std::fs::create_dir_all(&bundle).expect("fake pinned Kani bundle");
            for binary in [
                "kani-compiler",
                "kani-driver",
                "cbmc",
                "goto-cc",
                "goto-instrument",
            ] {
                fake_executable(&bundle.join(binary));
            }
            let artifact = temporary.path().join("raw.json");
            #[cfg(unix)]
            let argv = temporary.path().join("argv.txt");
            let target = temporary.path().join("kani-target");
            std::fs::create_dir_all(&target).expect("fresh Kani target directory");
            let harness = generated(simple_source(), ">", ">=");
            std::fs::write(root.join("Cargo.toml"), MODEL_MANIFEST).expect("fixed manifest");
            std::fs::write(root.join("Cargo.lock"), MODEL_LOCK).expect("fixed lockfile");
            let source = root.join(MODEL_SOURCE_PATH);
            std::fs::write(&source, harness.source()).expect("rendered source");
            let document = document(&harness, status, &root, &target).to_string();
            let mut environment: Vec<_> = std::env::vars_os().collect();
            environment.retain(|(name, _value)| !Self::refused_key(name));
            set_env(
                &mut environment,
                "CARGO_HOME",
                cargo_home.as_os_str().to_owned(),
            );
            set_env(&mut environment, "HOME", home.as_os_str().to_owned());
            set_env(
                &mut environment,
                "FAKE_KANI_HOME",
                home.join(".kani").into_os_string(),
            );
            set_env(&mut environment, "FAKE_KANI_JSON", document.into());
            set_env(&mut environment, "FAKE_KANI_EXIT", exit.to_string().into());
            set_env(
                &mut environment,
                "FAKE_KANI_HARNESS",
                harness.harness_name().into(),
            );
            #[cfg(unix)]
            set_env(
                &mut environment,
                "FAKE_KANI_ARGV",
                argv.as_os_str().to_owned(),
            );
            set_env(
                &mut environment,
                "FAKE_KANI_TARGET",
                target.as_os_str().to_owned(),
            );
            Self {
                temporary,
                root,
                source,
                cargo,
                artifact,
                #[cfg(unix)]
                argv,
                target,
                harness,
                environment,
                trace: crate::trace::Recorder::disabled(),
            }
        }

        fn invocation(&self) -> Invocation<'_> {
            Invocation {
                kani: &self.cargo,
                root: &self.root,
                source: &self.source,
                package: MODEL_PACKAGE,
                target: "test-target",
                artifact: &self.artifact,
                target_dir: &self.target,
                harness: &self.harness,
                environment: &self.environment,
                trace: &self.trace,
            }
        }

        fn run(&self) -> Attempt {
            run(&self.invocation(), &rust_mutants::runner::Cancel::new())
        }
    }

    fn document(harness: &Harness, status: &str, root: &Path, target: &Path) -> serde_json::Value {
        let succeeded = u64::from(status == "Success");
        let failed = u64::from(status == "Failure");
        let crate_name = MODEL_PACKAGE.replace('-', "_");
        let output = target.join(format!("test-target/release/build/{crate_name}/out"));
        let goto = output.join("harness.goto");
        let mut value = json!({
            "metadata": {
                "version": "1.0", "timestamp": "2026-09-20T00:00:00Z",
                "kani_version": KANI_VERSION, "target": "test-target", "build_mode": "release"
            },
            "project": {
                "crate_name": [crate_name], "workspace_root": root.display().to_string(),
                "output_dir": output.display().to_string()
            },
            "tools": {
                "kani": KANI_VERSION, "rustc": KANI_RUSTC_VERSION,
                "cbmc": KANI_CBMC_VERSION, "goto_cc": KANI_GOTO_CC_VERSION,
                "goto_instrument": KANI_GOTO_INSTRUMENT_VERSION,
                "solvers": [{"name": KANI_SOLVER, "version": null}]
            },
            "harness_metadata": [{
                "pretty_name": harness.harness_name(),
                "mangled_name": "mangled", "crate_name": crate_name,
                "source": {"file": "src/lib.rs", "start_line": 1, "end_line": 1},
                "goto_file": goto.display().to_string(),
                "attributes": {"kind": "Proof", "should_panic": false},
                "contract": {"contracted_function_name": null, "recursion_tracker": null},
                "has_loop_contracts": false, "is_automatically_generated": false,
                "is_bounded": false, "is_ctor_based": false
            }],
            "error_details": [{
                "harness_id": harness.harness_name(), "has_errors": false
            }],
            "property_details": [{
                "harness_id": harness.harness_name(),
                "property_details": {
                    "total_properties": 1, "passed": succeeded, "failed": failed,
                    "unreachable": 0, "undetermined": 0, "solver_error": 0,
                    "satisfied": 0, "unsatisfiable": 0, "covered": 0, "uncovered": 0
                }
            }],
            "cbmc": [{
                "harness_id": harness.harness_name(),
                "cbmc_metadata": {"version": "6.11.0", "os_info": "fixture unix"},
                "configuration": {"object_bits": 16, "solver": KANI_SOLVER},
                "cbmc_stats": {
                    "runtime_symex_s": 0.0, "size_program_expression": 1,
                    "slicing_removed_assignments": 0, "vccs_generated": 1,
                    "vccs_remaining": 1, "runtime_postprocess_equation_s": 0.0,
                    "runtime_convert_ssa_s": null, "runtime_post_process_s": null,
                    "runtime_solver_s": null, "runtime_decision_procedure_s": null
                }
            }],
            "verification_results": {
                "summary": {
                    "total_harnesses": 1, "executed": 1, "status": "completed",
                    "successful": succeeded, "failed": failed, "duration_ms": 1
                },
                "results": [{
                    "harness_id": harness.harness_name(),
                    "status": status, "duration_ms": 1,
                    "checks": [{
                        "id": 1, "function": "f", "status": status,
                        "description": harness.assertion_tag(),
                        "location": { "file": "src/lib.rs", "line": "1", "column": "1" },
                        "category": "assertion"
                    }]
                }]
            },
            "coverage": { "enabled": false }
        });
        if status == "Failure" {
            value["error_details"] = failed_details(harness);
        }
        value
    }

    fn failed_details(harness: &Harness) -> serde_json::Value {
        json!([{
            "harness_id": harness.harness_name(),
            "has_errors": true,
            "error_type": "assertion_failure",
            "failed_properties_type": "PanicsOnly",
            "exit_status": "properties_failed"
        }])
    }

    fn set_env(
        environment: &mut Vec<(std::ffi::OsString, std::ffi::OsString)>,
        name: &str,
        value: std::ffi::OsString,
    ) {
        environment.retain(|(key, _value)| key != name);
        environment.push((name.into(), value));
    }

    #[cfg(unix)]
    fn fake_cargo(path: &Path) {
        use std::os::unix::fs::PermissionsExt as _;

        std::fs::write(
            path,
            r#"#!/bin/sh
if [ "$1" = "kani" ] && [ "$2" = "--version" ]; then
  printf '%s\n' "${FAKE_KANI_VERSION:-Kani Rust Verifier 0.68.0 (cargo plugin)}"
  printf '%s\n' "${FAKE_KANI_BACKEND:-CBMC 6.11.0}"
  if [ -n "${FAKE_KANI_VERSION_STDERR:-}" ]; then
    printf '%s\n' "$FAKE_KANI_VERSION_STDERR" >&2
  fi
  exit 0
fi
if [ "$CARGO_BUILD_TARGET" != "test-target" ]; then
  printf 'unexpected CARGO_BUILD_TARGET=%s\n' "$CARGO_BUILD_TARGET" >&2
  exit 88
fi
if [ "$CARGO_TARGET_DIR" != "$FAKE_KANI_TARGET" ]; then
  printf 'unexpected CARGO_TARGET_DIR=%s\n' "$CARGO_TARGET_DIR" >&2
  exit 84
fi
if [ "${CARGO_ENCODED_RUSTFLAGS+x}" != x ] || [ -n "$CARGO_ENCODED_RUSTFLAGS" ]; then
  printf 'CARGO_ENCODED_RUSTFLAGS was not fixed to the empty flag set\n' >&2
  exit 87
fi
if [ "${RUSTC_WRAPPER+x}" != x ] || [ -n "$RUSTC_WRAPPER" ] || [ "${RUSTC_WORKSPACE_WRAPPER+x}" != x ] || [ -n "$RUSTC_WORKSPACE_WRAPPER" ]; then
  printf 'Cargo compiler wrappers were not fixed to the empty interposer set\n' >&2
  exit 86
fi
if [ "${RUSTC+x}${RUSTC_BOOTSTRAP+x}${CARGO_BUILD_RUSTC+x}${CARGO_BUILD_RUSTC_WRAPPER+x}${CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER+x}" != "" ]; then
  printf 'caller compiler selection leaked into Kani\n' >&2
  exit 85
fi
if [ "$KANI_HOME" != "$FAKE_KANI_HOME" ]; then
  printf 'KANI_HOME was not fixed to the validated versioned bundle parent\n' >&2
  exit 82
fi
if [ "${PYTHONPATH+x}${NIX_CC+x}${RUSTUP_TOOLCHAIN+x}${DYLD_INSERT_LIBRARIES+x}${LD_PRELOAD+x}" != "" ]; then
  printf 'caller verifier or dynamic-loader environment leaked into Kani\n' >&2
  exit 83
fi
if [ -n "${FAKE_KANI_SLEEP:-}" ]; then
  sleep "$FAKE_KANI_SLEEP"
fi
if [ "$1" = "kani" ] && [ "$2" = "list" ]; then
  printf '{"kani-version":"0.68.0","file-version":"0.1","standard-harnesses":{"%s":["%s"]},"contract-harnesses":{},"contracts":[],"totals":{"standard-harnesses":1,"contract-harnesses":0,"functions-under-contract":0}}' "${FAKE_KANI_FILE:-src/lib.rs}" "$FAKE_KANI_HARNESS" > kani-list.json
  exit 0
fi
printf '%s\n' "$@" > "$FAKE_KANI_ARGV"
output=
previous=
for argument in "$@"; do
  if [ "$previous" = "--export-json" ]; then output=$argument; fi
  previous=$argument
done
printf '%s' "$FAKE_KANI_JSON" > "$output"
exit "$FAKE_KANI_EXIT"
"#,
        )
        .expect("fake cargo script");
        let mut permissions = std::fs::metadata(path)
            .expect("fake metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("fake is executable");
    }

    #[cfg(unix)]
    fn fake_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt as _;

        std::fs::write(path, b"fixture").expect("fake bundle executable");
        let mut permissions = std::fs::metadata(path)
            .expect("fake bundle metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("fake bundle is executable");
    }

    #[cfg(not(unix))]
    fn fake_executable(_path: &Path) {
        panic!("the fake Kani bundle currently requires POSIX permissions");
    }

    #[cfg(unix)]
    #[test]
    fn accepts_only_exit_coherent_affirmative_documents() {
        let proved_fixture = Fixture::new("Success", 0);
        let proved = proved_fixture.run();
        assert_eq!(proved.parsed.decision, Decision::Proved);
        assert_eq!(proved.process, Process::Exited(0));
        assert!(proved.artifact.bytes().is_some());
        let argv = std::fs::read_to_string(&proved_fixture.argv).expect("captured argv");
        let argv: Vec<_> = argv.lines().collect();
        let harness_at = argv
            .iter()
            .position(|argument| *argument == "--harness")
            .expect("harness argument");
        assert_eq!(argv[harness_at + 1], proved_fixture.harness.harness_name());
        assert!(argv.contains(&"--exact"));
        assert!(!argv.contains(&"--locked"));
        let Artifact::Complete { path, .. } = &proved.artifact else {
            panic!("a proved answer retains one complete raw artifact");
        };
        assert_eq!(
            proved.parsed.evidence.raw_digest,
            rust_mutants::id::digest(&std::fs::read(path).expect("raw artifact"),)
        );

        let noticed = Fixture::new("Failure", 1).run();
        assert_eq!(noticed.parsed.decision, Decision::Noticed);

        let contradiction = Fixture::new("Success", 1).run();
        assert!(matches!(
            contradiction.parsed.decision,
            Decision::Undecided(Undecided::ExitMismatch { .. })
        ));

        let mut ambient = Fixture::new("Success", 0);
        set_env(
            &mut ambient.environment,
            "KANI_HOME",
            "/hostile/kani".into(),
        );
        set_env(
            &mut ambient.environment,
            "PYTHONPATH",
            "/hostile/python".into(),
        );
        assert_eq!(
            ambient.run().parsed.decision,
            Decision::Proved,
            "the closed subprocess environment must omit caller verifier inputs"
        );
    }

    #[cfg(unix)]
    #[test]
    fn malformed_and_stale_artifacts_fail_closed() {
        let mut malformed = Fixture::new("Success", 0);
        set_env(
            &mut malformed.environment,
            "FAKE_KANI_JSON",
            "not json".into(),
        );
        assert_eq!(
            malformed.run().parsed.decision,
            Decision::Undecided(Undecided::Protocol(Protocol::Schema))
        );

        let stale = Fixture::new("Success", 0);
        std::fs::write(&stale.artifact, b"stale").expect("stale artifact");
        assert_eq!(
            stale.run().parsed.decision,
            Decision::Undecided(Undecided::Artifact(ArtifactFailure::AlreadyExists))
        );

        let mut wrong_source = Fixture::new("Success", 0);
        set_env(
            &mut wrong_source.environment,
            "FAKE_KANI_FILE",
            "src/other.rs".into(),
        );
        assert_eq!(
            wrong_source.run().parsed.decision,
            Decision::Undecided(Undecided::Tool(ToolFailure::HarnessListMatch))
        );
        assert!(!wrong_source.artifact.exists());
    }

    #[test]
    fn duplicate_harness_catalog_keys_are_rejected() {
        let fixture = Fixture::new("Success", 0);
        let catalog = fixture.root.join("kani-list.json");
        std::fs::write(
            &catalog,
            format!(
                "{{\"kani-version\":\"0.68.0\",\"kani-version\":\"0.68.0\",\"file-version\":\"0.1\",\"standard-harnesses\":{{\"src/lib.rs\":[\"{}::{}\"]}},\"contract-harnesses\":{{}},\"contracts\":[],\"totals\":{{\"standard-harnesses\":1,\"contract-harnesses\":0,\"functions-under-contract\":0}}}}",
                MODEL_PACKAGE.replace('-', "_"),
                fixture.harness.harness_name()
            ),
        )
        .expect("duplicate-key catalog");
        assert_eq!(
            super::catalog_harness(&catalog, &fixture.harness),
            Err(ToolFailure::HarnessListSchema)
        );
    }

    #[test]
    fn catalog_is_exactly_one_standard_harness_and_no_contract_surface() {
        let fixture = Fixture::new("Success", 0);
        let catalog = fixture.root.join("kani-list.json");
        let harness = fixture.harness.harness_name().to_owned();
        for document in [
            json!({
                "kani-version": KANI_VERSION,
                "file-version": super::KANI_LIST_VERSION,
                "standard-harnesses": {
                    MODEL_SOURCE_PATH: [harness, "njutest_verified_model::extra"]
                },
                "contract-harnesses": {},
                "contracts": [],
                "totals": {
                    "standard-harnesses": 2,
                    "contract-harnesses": 0,
                    "functions-under-contract": 0
                }
            }),
            json!({
                "kani-version": KANI_VERSION,
                "file-version": super::KANI_LIST_VERSION,
                "standard-harnesses": { MODEL_SOURCE_PATH: [harness] },
                "contract-harnesses": { MODEL_SOURCE_PATH: ["contract"] },
                "contracts": [{
                    "function": "f", "file": MODEL_SOURCE_PATH, "harnesses": ["contract"]
                }],
                "totals": {
                    "standard-harnesses": 1,
                    "contract-harnesses": 1,
                    "functions-under-contract": 1
                }
            }),
        ] {
            std::fs::write(
                &catalog,
                serde_json::to_vec(&document).expect("catalog fixture serializes"),
            )
            .expect("catalog fixture");
            assert_eq!(
                super::catalog_harness(&catalog, &fixture.harness),
                Err(ToolFailure::HarnessListMatch)
            );
        }
    }

    #[test]
    fn compile_time_package_inputs_cannot_enter_the_isolated_crate() {
        let fixture = Fixture::new("Success", 0);
        std::fs::write(fixture.root.join("build.rs"), "fn main() {}")
            .expect("adversarial build script");
        assert_eq!(
            fixture.run().parsed.decision,
            Decision::Undecided(Undecided::Configuration(
                super::Configuration::WorkspaceDrift
            ))
        );
        assert!(!fixture.artifact.exists());
    }

    #[cfg(unix)]
    #[test]
    fn retained_artifact_reader_rejects_empty_files_and_final_symlinks() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().expect("artifact boundary");
        let empty = temporary.path().join("empty.json");
        std::fs::write(&empty, []).expect("empty artifact");
        assert_eq!(super::read_artifact(&empty), Err(ArtifactFailure::TooLarge));

        let target = temporary.path().join("target.json");
        let link = temporary.path().join("linked.json");
        std::fs::write(&target, b"{}").expect("target artifact");
        symlink(&target, &link).expect("artifact symlink");
        assert_eq!(
            super::read_artifact(&link),
            Err(ArtifactFailure::Unreadable)
        );
    }

    #[cfg(unix)]
    #[test]
    fn generated_source_parent_symlinks_cannot_escape_the_workspace_boundary() {
        use std::os::unix::fs::symlink;

        let mut fixture = Fixture::new("Success", 0);
        let outside = fixture.temporary.path().join("outside-source");
        std::fs::create_dir_all(&outside).expect("outside source directory");
        std::fs::write(outside.join("lib.rs"), fixture.harness.source())
            .expect("outside rendered source");
        symlink(&outside, fixture.root.join("redirect")).expect("source parent symlink");
        fixture.source = fixture.root.join("redirect/lib.rs");
        assert_eq!(
            fixture.run().parsed.decision,
            Decision::Undecided(Undecided::Configuration(
                super::Configuration::WorkspaceDrift
            ))
        );
        assert!(!fixture.artifact.exists());
    }

    #[cfg(unix)]
    #[test]
    fn checker_and_versioned_bundle_parent_symlinks_are_refused() {
        use std::os::unix::fs::symlink;

        let checker = Fixture::new("Success", 0);
        let cargo_home = checker
            .environment
            .iter()
            .find_map(|(name, value)| {
                (name == "CARGO_HOME").then_some(std::path::PathBuf::from(value))
            })
            .expect("fixture Cargo home");
        let real_bin = cargo_home.join("real-bin");
        std::fs::rename(cargo_home.join("bin"), &real_bin).expect("move checker bin");
        symlink(&real_bin, cargo_home.join("bin")).expect("checker parent symlink");
        assert_eq!(
            checker.run().parsed.decision,
            Decision::Undecided(Undecided::Configuration(super::Configuration::Directory))
        );

        let bundle = Fixture::new("Success", 0);
        let home = bundle
            .environment
            .iter()
            .find_map(|(name, value)| (name == "HOME").then_some(std::path::PathBuf::from(value)))
            .expect("fixture home");
        let real_kani = home.join("real-kani");
        std::fs::rename(home.join(".kani"), &real_kani).expect("move Kani bundle");
        symlink(&real_kani, home.join(".kani")).expect("bundle parent symlink");
        assert_eq!(
            bundle.run().parsed.decision,
            Decision::Undecided(Undecided::Configuration(super::Configuration::Directory))
        );
    }

    #[cfg(unix)]
    #[test]
    fn version_and_outer_timeout_are_fail_closed() {
        let mut wrong = Fixture::new("Success", 0);
        set_env(
            &mut wrong.environment,
            "FAKE_KANI_VERSION",
            "Kani Rust Verifier 0.69.0 (cargo plugin)".into(),
        );
        assert_eq!(
            wrong.run().parsed.decision,
            Decision::Undecided(Undecided::Tool(ToolFailure::VersionBanner))
        );
        assert!(!wrong.artifact.exists());

        let mut noisy = Fixture::new("Success", 0);
        set_env(
            &mut noisy.environment,
            "FAKE_KANI_VERSION_STDERR",
            "unexpected diagnostic".into(),
        );
        assert_eq!(
            noisy.run().parsed.decision,
            Decision::Undecided(Undecided::Tool(ToolFailure::VersionBanner))
        );
        assert!(!noisy.artifact.exists());

        let mut slow = Fixture::new("Success", 0);
        slow.harness
            .set_timeout_for_test(std::num::NonZeroU64::new(100).expect("a nonzero test timeout"));
        set_env(&mut slow.environment, "FAKE_KANI_SLEEP", "400".into());
        let answer = slow.run();
        assert_eq!(
            answer.parsed.decision,
            Decision::Undecided(Undecided::Cutoff)
        );
        assert_eq!(answer.process, Process::Cutoff);
    }

    #[test]
    fn caller_compiler_flags_are_refused_instead_of_silently_reinterpreted() {
        for name in [
            "RUSTFLAGS",
            "CARGO_ENCODED_RUSTFLAGS",
            "CARGO_BUILD_RUSTFLAGS",
            "CARGO_TARGET_TEST_TARGET_RUSTFLAGS",
            "cargo_encoded_RustFlags",
            "Cargo_Target_Test_Target_RustFlags",
        ] {
            let mut fixture = Fixture::new("Success", 0);
            set_env(
                &mut fixture.environment,
                name,
                "--cfg=outside_evidence".into(),
            );
            assert_eq!(
                fixture.run().parsed.decision,
                Decision::Undecided(Undecided::Configuration(
                    super::Configuration::CompilerFlags
                )),
                "{name}"
            );
            assert!(!fixture.artifact.exists(), "{name}");
        }
    }

    #[test]
    fn caller_compiler_interposition_is_refused_before_kani() {
        for name in [
            "RUSTC",
            "RUSTC_WRAPPER",
            "RUSTC_WORKSPACE_WRAPPER",
            "RUSTC_BOOTSTRAP",
            "CARGO_BUILD_RUSTC",
            "CARGO_BUILD_RUSTC_WRAPPER",
            "Cargo_Build_Rustc_Workspace_Wrapper",
        ] {
            let mut fixture = Fixture::new("Success", 0);
            set_env(&mut fixture.environment, name, "interposed".into());
            assert_eq!(
                fixture.run().parsed.decision,
                Decision::Undecided(Undecided::Configuration(
                    super::Configuration::CompilerEnvironment
                )),
                "{name}"
            );
            assert!(!fixture.artifact.exists(), "{name}");
        }
    }

    #[test]
    fn caller_release_profile_overrides_are_refused_before_kani() {
        for name in [
            "CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS",
            "cargo_profile_release_debug_assertions",
            "Cargo_Profile_Release_Panic",
        ] {
            let mut fixture = Fixture::new("Success", 0);
            set_env(&mut fixture.environment, name, "true".into());
            assert_eq!(
                fixture.run().parsed.decision,
                Decision::Undecided(Undecided::Configuration(super::Configuration::Profile)),
                "{name}"
            );
            assert!(!fixture.artifact.exists(), "{name}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn subject_cargo_alias_is_refused_even_though_the_checker_is_resolved_directly() {
        let fixture = Fixture::new("Success", 0);
        let cargo_dir = fixture
            .root
            .parent()
            .expect("fixture root has an ancestor")
            .join(".cargo");
        std::fs::create_dir_all(&cargo_dir).expect("subject Cargo config directory");
        std::fs::write(
            cargo_dir.join("config.toml"),
            "[alias]\nkani = [\"run\", \"--bin\", \"attacker\"]\n",
        )
        .expect("adversarial Cargo alias");
        assert_eq!(
            fixture.run().parsed.decision,
            Decision::Undecided(Undecided::Configuration(
                super::Configuration::CompilerEnvironment
            ))
        );
        assert!(!fixture.artifact.exists());
    }

    #[test]
    fn the_repository_cargo_configuration_is_the_only_admitted_shape() {
        let fixture = Fixture::new("Success", 0);
        let cargo_dir = fixture
            .root
            .parent()
            .expect("fixture root has an ancestor")
            .join(".cargo");
        std::fs::create_dir_all(&cargo_dir).expect("subject Cargo config directory");
        std::fs::write(
            cargo_dir.join("config.toml"),
            "[build]\n\n[alias]\nxtask = \"run --quiet --locked --package xtask --\"\n\n[net]\nretry = 10\n\n[term]\nverbose = false\n",
        )
        .expect("closed safe Cargo configuration");
        assert_eq!(fixture.run().parsed.decision, Decision::Proved);
    }

    #[test]
    fn cargo_configuration_unknown_or_semantic_keys_are_refused() {
        for configuration in [
            "[build]\nrustc-wrapper = \"attacker\"\n",
            "[target.test-target]\nrustflags = [\"--cfg\", \"attacker\"]\n",
            "[profile.release]\noverflow-checks = false\n",
            "[unstable]\nbuild-std = [\"core\"]\n",
            "[registries.attacker]\nindex = \"file:///tmp/attacker\"\n",
            "[net]\noffline = true\n",
            "[term]\nverbose = true\n",
            "[alias]\nxtask = \"run attacker\"\n",
        ] {
            let fixture = Fixture::new("Success", 0);
            let cargo_dir = fixture
                .root
                .parent()
                .expect("fixture root has an ancestor")
                .join(".cargo");
            std::fs::create_dir_all(&cargo_dir).expect("subject Cargo config directory");
            std::fs::write(cargo_dir.join("config.toml"), configuration)
                .expect("adversarial Cargo configuration");
            assert_eq!(
                fixture.run().parsed.decision,
                Decision::Undecided(Undecided::Configuration(
                    super::Configuration::CompilerEnvironment
                )),
                "{configuration}"
            );
            assert!(!fixture.artifact.exists(), "{configuration}");
        }
    }

    #[test]
    fn cargo_home_must_be_one_absolute_non_symlink_directory() {
        for home in ["", "relative/cargo-home"] {
            let mut fixture = Fixture::new("Success", 0);
            set_env(&mut fixture.environment, "CARGO_HOME", home.into());
            assert!(matches!(
                fixture.run().parsed.decision,
                Decision::Undecided(Undecided::Configuration(
                    super::Configuration::CompilerEnvironment
                ))
            ));
            assert!(!fixture.artifact.exists());
        }

        let mut duplicate = Fixture::new("Success", 0);
        duplicate.environment.push((
            std::ffi::OsString::from("cargo_home"),
            duplicate.temporary.path().as_os_str().to_owned(),
        ));
        assert!(matches!(
            duplicate.run().parsed.decision,
            Decision::Undecided(Undecided::Configuration(
                super::Configuration::CompilerEnvironment
            ))
        ));

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let mut linked = Fixture::new("Success", 0);
            let real = linked.temporary.path().join("real-cargo-home");
            let link = linked.temporary.path().join("linked-cargo-home");
            std::fs::create_dir_all(&real).expect("real Cargo home");
            symlink(&real, &link).expect("Cargo home symlink");
            set_env(
                &mut linked.environment,
                "CARGO_HOME",
                link.as_os_str().to_owned(),
            );
            assert!(matches!(
                linked.run().parsed.decision,
                Decision::Undecided(Undecided::Configuration(
                    super::Configuration::CompilerEnvironment
                ))
            ));
        }
    }

    #[test]
    fn a_proof_target_is_one_fresh_empty_directory() {
        let contaminated = Fixture::new("Success", 0);
        std::fs::write(contaminated.target.join("stale"), b"prior build")
            .expect("stale target entry");
        assert_eq!(
            contaminated.run().parsed.decision,
            Decision::Undecided(Undecided::Configuration(super::Configuration::Directory))
        );
        assert!(!contaminated.artifact.exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let linked = Fixture::new("Success", 0);
            std::fs::remove_dir(&linked.target).expect("replace target directory");
            let outside = linked.temporary.path().join("outside-target");
            std::fs::create_dir_all(&outside).expect("outside target directory");
            symlink(&outside, &linked.target).expect("adversarial target symlink");
            assert!(matches!(
                linked.run().parsed.decision,
                Decision::Undecided(Undecided::Configuration(super::Configuration::Directory))
            ));
            assert!(!linked.artifact.exists());

            let mut parent_linked = Fixture::new("Success", 0);
            std::fs::remove_dir(&parent_linked.target).expect("replace target directory");
            let outside_parent = parent_linked.temporary.path().join("outside-parent");
            std::fs::create_dir_all(&outside_parent).expect("outside target parent");
            std::fs::create_dir_all(outside_parent.join("target")).expect("outside target leaf");
            let redirect = parent_linked.temporary.path().join("target-redirect");
            symlink(&outside_parent, &redirect).expect("target parent symlink");
            parent_linked.target = redirect.join("target");
            assert!(matches!(
                parent_linked.run().parsed.decision,
                Decision::Undecided(Undecided::Configuration(super::Configuration::Directory))
            ));

            let mut artifact_parent = Fixture::new("Success", 0);
            let outside_artifact = artifact_parent.temporary.path().join("outside-artifact");
            std::fs::create_dir_all(&outside_artifact).expect("outside artifact directory");
            let artifact_redirect = artifact_parent.temporary.path().join("artifact-redirect");
            symlink(&outside_artifact, &artifact_redirect).expect("artifact parent symlink");
            artifact_parent.artifact = artifact_redirect.join("raw.json");
            assert!(matches!(
                artifact_parent.run().parsed.decision,
                Decision::Undecided(Undecided::Configuration(super::Configuration::Directory))
            ));
        }
    }

    #[cfg(unix)]
    #[test]
    fn forced_cargo_configuration_environment_is_refused_before_checker_start() {
        let fixture = Fixture::new("Success", 0);
        let cargo_dir = fixture
            .root
            .parent()
            .expect("fixture root has an ancestor")
            .join(".cargo");
        std::fs::create_dir_all(&cargo_dir).expect("subject Cargo config directory");
        std::fs::write(
            cargo_dir.join("config.toml"),
            "[env.RUSTC_WRAPPER]\nvalue = \"attacker\"\nforce = true\n",
        )
        .expect("adversarial forced environment");
        assert_eq!(
            fixture.run().parsed.decision,
            Decision::Undecided(Undecided::Configuration(
                super::Configuration::CompilerEnvironment
            ))
        );
        assert!(!fixture.artifact.exists());
    }

    #[cfg(unix)]
    #[test]
    fn cargo_configuration_symlinks_and_nonfiles_are_refused() {
        use std::os::unix::fs::symlink;

        let final_link = Fixture::new("Success", 0);
        let cargo_dir = final_link
            .root
            .parent()
            .expect("fixture root has an ancestor")
            .join(".cargo");
        std::fs::create_dir_all(&cargo_dir).expect("subject Cargo config directory");
        let target = final_link.root.join("attacker.toml");
        std::fs::write(&target, "").expect("symlink target");
        symlink(&target, cargo_dir.join("config.toml")).expect("configuration symlink");
        assert!(matches!(
            final_link.run().parsed.decision,
            Decision::Undecided(Undecided::Configuration(
                super::Configuration::CompilerEnvironment
            ))
        ));

        let parent_link = Fixture::new("Success", 0);
        let target_dir = parent_link.root.join("attacker-cargo");
        std::fs::create_dir_all(&target_dir).expect("symlink target directory");
        std::fs::write(target_dir.join("config.toml"), "").expect("configuration");
        symlink(
            &target_dir,
            parent_link
                .root
                .parent()
                .expect("fixture root has an ancestor")
                .join(".cargo"),
        )
        .expect("configuration parent symlink");
        assert!(matches!(
            parent_link.run().parsed.decision,
            Decision::Undecided(Undecided::Configuration(
                super::Configuration::CompilerEnvironment
            ))
        ));

        let directory = Fixture::new("Success", 0);
        let cargo_dir = directory
            .root
            .parent()
            .expect("fixture root has an ancestor")
            .join(".cargo");
        std::fs::create_dir_all(&cargo_dir).expect("subject Cargo config directory");
        std::fs::create_dir_all(cargo_dir.join("config.toml")).expect("non-file configuration");
        assert!(matches!(
            directory.run().parsed.decision,
            Decision::Undecided(Undecided::Configuration(
                super::Configuration::CompilerEnvironment
            ))
        ));
    }

    #[test]
    fn real_kani_068_runs_through_the_supervised_protocol_when_requested() {
        if std::env::var_os("NJUTEST_REAL_KANI").is_none() {
            return;
        }
        let (answer, retained) = real_kani_attempt(simple_source());
        assert_eq!(
            answer.parsed.decision,
            Decision::Proved,
            "{answer:?}\nretained export:\n{retained}"
        );
        assert_eq!(
            answer
                .parsed
                .evidence
                .verifier
                .as_ref()
                .map(|verifier| verifier.tool.as_str()),
            Some(KANI_VERSION)
        );
        assert_eq!(answer.process, Process::Exited(0));
        assert!(answer.artifact.bytes().is_some());
    }

    #[test]
    fn real_kani_068_counterexample_uses_the_typed_noticed_path_when_requested() {
        if std::env::var_os("NJUTEST_REAL_KANI").is_none() {
            return;
        }
        let (answer, retained) = real_kani_attempt("pub fn positive(x: i32) -> bool { x > 0 }\n");
        assert_eq!(
            answer.parsed.decision,
            Decision::Noticed,
            "{answer:?}\nretained export:\n{retained}"
        );
        assert_eq!(
            answer
                .parsed
                .evidence
                .verifier
                .as_ref()
                .map(|verifier| verifier.tool.as_str()),
            Some(KANI_VERSION)
        );
        assert_eq!(answer.process, Process::Exited(1));
        assert!(answer.artifact.bytes().is_some());
    }

    fn real_kani_attempt(source: &str) -> (Attempt, String) {
        let temporary = tempfile::tempdir().expect("temporary real Kani tree");
        let root = temporary.path().join("root");
        std::fs::create_dir_all(root.join("src")).expect("source directory");
        let harness = generated(source, ">", ">=");
        std::fs::write(root.join("Cargo.toml"), MODEL_MANIFEST).expect("manifest");
        std::fs::write(root.join("Cargo.lock"), MODEL_LOCK).expect("lockfile");
        std::fs::write(root.join(MODEL_SOURCE_PATH), harness.source()).expect("rendered source");
        let mut environment: Vec<_> = std::env::vars_os().collect();
        environment.retain(|(name, _value)| !super::compiler_environment_key(name));
        let trace = crate::trace::Recorder::disabled();
        let artifact = temporary.path().join("kani.json");
        let target = temporary.path().join("target-kani");
        std::fs::create_dir_all(&target).expect("fresh real-Kani target directory");
        let host = host_target();
        let cargo_home = std::env::var_os("CARGO_HOME")
            .map(std::path::PathBuf::from)
            .expect("real-Kani tests require an explicit CARGO_HOME");
        let executable = if cfg!(windows) {
            "cargo-kani.exe"
        } else {
            "cargo-kani"
        };
        let kani = cargo_home.join("bin").join(executable);
        let invocation = Invocation {
            kani: &kani,
            root: &root,
            source: &root.join(MODEL_SOURCE_PATH),
            package: MODEL_PACKAGE,
            target: &host,
            artifact: &artifact,
            target_dir: &target,
            harness: &harness,
            environment: &environment,
            trace: &trace,
        };
        let answer = run(&invocation, &rust_mutants::runner::Cancel::new());
        let retained = std::fs::read_to_string(&artifact).unwrap_or_default();
        (answer, retained)
    }

    fn host_target() -> String {
        let output = std::process::Command::new("rustc")
            .arg("-vV")
            .output()
            .expect("rustc host target probe");
        assert!(output.status.success(), "rustc host target probe failed");
        let banner = String::from_utf8(output.stdout).expect("rustc version is UTF-8");
        banner
            .lines()
            .find_map(|line| line.strip_prefix("host: "))
            .expect("rustc host line")
            .to_owned()
    }
}
