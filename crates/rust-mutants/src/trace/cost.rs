// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Cost counters fed by the same events as a recording, kept independently of verdicts.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Write as _};
use std::path::Path;
use std::sync::Mutex;

use serde::Serialize;

use super::Payload;

/// One session's optional diagnostic, shared by its recorder clones.
#[derive(Debug)]
pub(super) struct Costs {
    output: File,
    origin: Origin,
    machine: Machine,
    root: String,
    sealed: rust_mutants_sealed::Counted,
    work: Mutex<Work>,
}

/// The actual producer of one cost record, without assigning product work a nextest identity.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Origin {
    /// A test running under the matching nextest suite identity.
    Suite {
        /// The actual nextest binary identity.
        binary: String,
        /// The actual nextest test identity.
        test: String,
    },
    /// A product composition root running outside nextest.
    Product {
        /// The product that owns the command.
        program: Product,
        /// The actual product arguments recorded by its composition root.
        command: Vec<String>,
    },
}

/// The two product composition roots that can publish standalone work.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Product {
    /// The engine command line.
    RustMutants,
    /// The assurance command line.
    Njutest,
}

/// The executing host observed when a cost recorder starts.
#[derive(Debug, Serialize)]
struct Machine {
    os: &'static str,
    arch: &'static str,
    cpus: usize,
}

/// The actual toolchain operation whose result the producer observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProbeRole {
    /// Cargo's actual toolchain banner.
    CargoBanner,
    /// Rustc's actual target configuration.
    RustcCfg,
    /// A direct Rustc compilation.
    RustcBuild,
}

/// One actual keyed probe result, with no fabricated nextest identity.
#[derive(Debug, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Probe {
    identity: String,
    role: ProbeRole,
    started: bool,
    duration_ns: u64,
}

/// Actual requests and launches under one toolchain invocation identity.
#[derive(Debug, Serialize)]
struct ProbeWork {
    role: ProbeRole,
    requests: u64,
    processes: u64,
    failed_launches: u64,
    duration_ns: u64,
}

/// The operation described by one actual observed invocation.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ExecutionRole {
    CargoBuild,
    CargoTest,
    CargoDocTest,
    CargoMetadata,
    CargoProbe,
    CargoUnclassified,
    RustcProbe,
    RustcBuild,
    Program,
}

/// Actual attempts, launches and durations under one observed invocation identity.
#[derive(Debug, Serialize)]
struct ExecutionWork {
    role: ExecutionRole,
    requests: u64,
    processes: u64,
    failed_launches: u64,
    duration_ms: u64,
}

impl ExecutionRole {
    fn of(exec: &super::ExecRecord) -> Self {
        let program = exec
            .argv
            .first()
            .and_then(|program| Path::new(program).file_stem());
        let args: Vec<_> = exec.argv.iter().skip(1).map(String::as_str).collect();
        match program.and_then(std::ffi::OsStr::to_str) {
            Some("cargo") => Self::cargo(&args),
            Some("rustc")
                if args
                    .iter()
                    .any(|arg| matches!(*arg, "-vV" | "-V" | "--version" | "--print")) =>
            {
                Self::RustcProbe
            }
            Some("rustc") => Self::RustcBuild,
            _ => Self::Program,
        }
    }

    fn cargo(args: &[&str]) -> Self {
        let command = args.iter().find(|arg| !arg.starts_with('+')).copied();
        match command {
            Some("build" | "check" | "rustc") => Self::CargoBuild,
            Some("test") if args.contains(&"--no-run") => Self::CargoBuild,
            Some("test") if args.contains(&"--doc") => Self::CargoDocTest,
            Some("test") => Self::CargoTest,
            Some("metadata") => Self::CargoMetadata,
            Some("-vV" | "-V" | "--version") => Self::CargoProbe,
            Some(_) | None => Self::CargoUnclassified,
        }
    }

    const fn is_cargo(self) -> bool {
        match self {
            Self::CargoBuild
            | Self::CargoTest
            | Self::CargoDocTest
            | Self::CargoMetadata
            | Self::CargoProbe
            | Self::CargoUnclassified => true,
            Self::RustcProbe | Self::RustcBuild | Self::Program => false,
        }
    }
}

/// The actual caller's requested diagnostic and workspace root.
#[derive(Debug, Clone, Copy)]
pub struct ProbeSite<'a> {
    /// The caller's actual variables, including its optional cost request.
    pub vars: Option<&'a crate::vars::Variables>,
    /// The workspace the command actually observes.
    pub root: &'a Path,
}

/// The executing host named in one measured wait.
#[derive(Debug, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WaitMachine {
    os: String,
    cpus: u64,
}

/// The actual owner, cause and monotonic time published by a host-wait producer.
#[derive(Debug, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct HostWait {
    owner: String,
    cause: String,
    elapsed_ns: u64,
    machine: WaitMachine,
}

/// Records one actual toolchain probe outside a session's existing recorder.
///
/// # Errors
/// Its measured command or diagnostic cannot be represented or published.
pub fn record_probe(
    site: ProbeSite<'_>,
    role: ProbeRole,
    spec: &crate::runner::Spec,
    result: &crate::runner::RunResult,
) -> Result<(), crate::cargo::CargoError> {
    let ProbeSite { vars, root } = site;
    let Some(vars) = vars else {
        return Ok(());
    };
    if !vars.holds("NJUTEST_TEST_COST_DIR") {
        return Ok(());
    }
    let fail = |source: String| {
        crate::cargo::CargoError::new(crate::cargo::CargoErrorKind::CommandFailed, source)
    };
    let trace = super::Recorder::disabled()
        .costed_as(vars, root, "cost-probe-")
        .map_err(|source| fail(source.to_string()))?;
    let exec = super::ExecRecord::of(spec, result).map_err(|source| fail(source.to_string()))?;
    let input = serde_json::to_vec(&(role, &exec.argv, &exec.dir))
        .map_err(|source| fail(source.to_string()))?;
    let nanos =
        u64::try_from(result.duration.as_nanos()).map_err(|source| fail(source.to_string()))?;
    let millis =
        u64::try_from(result.duration.as_millis()).map_err(|source| fail(source.to_string()))?;
    let probe = Probe {
        identity: hex::encode(<sha2::Sha256 as sha2::Digest>::digest(input)),
        role,
        started: result.leader.is_some(),
        duration_ns: nanos,
    };
    trace.exec(exec);
    trace.note(
        "tool-probe",
        &serde_json::to_string(&probe).map_err(|source| fail(source.to_string()))?,
    );
    let kind = match role {
        ProbeRole::CargoBanner => "cargo-probe",
        ProbeRole::RustcCfg => "rustc-probe",
        ProbeRole::RustcBuild => "rustc-build",
    };
    if probe.started {
        trace.note(kind, &millis.to_string());
    }
    Ok(())
}

impl Origin {
    fn of(vars: &crate::vars::Variables) -> io::Result<Self> {
        let label = |name: &str| {
            vars.var(name)
                .and_then(std::ffi::OsStr::to_str)
                .filter(|label| !label.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| io::Error::other(format!("cost recording requires UTF-8 {name}")))
        };
        if vars.holds("NEXTEST_BINARY_ID") || vars.holds("NEXTEST_TEST_NAME") {
            return Ok(Self::Suite {
                binary: label("NEXTEST_BINARY_ID")?,
                test: label("NEXTEST_TEST_NAME")?,
            });
        }
        let program = match vars
            .var("NJUTEST_COST_PRODUCT")
            .and_then(std::ffi::OsStr::to_str)
        {
            Some("rust-mutants") => Product::RustMutants,
            Some("njutest") => Product::Njutest,
            Some(other) => return Err(io::Error::other(format!("unknown cost product {other:?}"))),
            None => {
                return Err(io::Error::other(
                    "cost recording needs an actual suite or product origin",
                ));
            }
        };
        let command = label("NJUTEST_COST_COMMAND")?;
        let command = crate::strictjson::decode_str(&command)
            .map_err(|source| io::Error::other(format!("cost command is invalid: {source}")))?;
        Ok(Self::Product { program, command })
    }

    fn labels(&self) -> (Option<&str>, Option<&str>) {
        match self {
            Self::Suite { binary, test } => (Some(binary), Some(test)),
            Self::Product {
                program: Product::RustMutants | Product::Njutest,
                command: _,
            } => (None, None),
        }
    }
}

/// What one bound build key accumulated: every request, every process it started, every launch that failed, and why.
#[derive(Debug, Default, Serialize)]
struct KeyWork {
    requests: u64,
    hits: u64,
    misses: u64,
    processes: u64,
    failed_launches: u64,
    reasons: BTreeMap<String, u64>,
    refused_writes: BTreeMap<String, u64>,
    launch_causes: BTreeMap<String, u64>,
}

/// What one unbound identity accumulated, with the reason it never had a key.
#[derive(Debug, Default, Serialize)]
struct UnboundWork {
    requests: u64,
    misses: u64,
    processes: u64,
    failed_launches: u64,
    launch_causes: BTreeMap<String, u64>,
}

#[derive(Debug, Default, Serialize)]
struct Work {
    builds: u64,
    build_requests: u64,
    build_hits: u64,
    build_misses: u64,
    build_keys: BTreeMap<String, KeyWork>,
    unbound: BTreeMap<String, UnboundWork>,
    launch_failures: u64,
    observed_cargo_starts: u64,
    cargo_probes: u64,
    cargo_probe_ms: u64,
    cargo_metadata: u64,
    cargo_metadata_ms: u64,
    rustc_probes: u64,
    rustc_probe_ms: u64,
    rustc_builds: u64,
    rustc_build_ms: u64,
    probes: BTreeMap<String, ProbeWork>,
    executions: BTreeMap<String, ExecutionWork>,
    host_waits: Vec<HostWait>,
    unobserved_cargo: Vec<&'static str>,
    build_ms: u64,
    units: u64,
    platform: Vec<rust_mutants_sealed::Spent>,
    platform_requests: u64,
    error: Option<String>,
}

/// Cargo command classes no costed run observes, named so a zero elsewhere cannot claim complete coverage.
const UNOBSERVED_CARGO: [&str; 1] =
    ["toolchain banners located outside a costed run (standalone commands and test support)"];

#[derive(Serialize)]
struct Record<'a> {
    schema: &'a str,
    origin: &'a Origin,
    machine: &'a Machine,
    #[serde(skip_serializing_if = "Option::is_none")]
    binary: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    test: Option<&'a str>,
    root: &'a str,
    work: &'a Work,
    sealed: Option<rust_mutants_sealed::Spent>,
}

impl Costs {
    pub(super) fn new(
        vars: &crate::vars::Variables,
        root: &Path,
        sealed: rust_mutants_sealed::Counted,
        prefix: &str,
    ) -> io::Result<Option<Self>> {
        let Some(directory) = vars.var("NJUTEST_TEST_COST_DIR") else {
            return Ok(None);
        };
        let origin = Origin::of(vars)?;
        let machine = Machine {
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            cpus: std::thread::available_parallelism()?.get(),
        };
        let root =
            crate::telling::LosslessBytes::new(root.as_os_str().as_encoded_bytes()).to_string();
        std::fs::create_dir_all(directory)?;
        let (output, path) = tempfile::Builder::new()
            .prefix(prefix)
            .suffix(".json")
            .tempfile_in(directory)?
            .keep()
            .map_err(|source| source.error)?;
        drop(path);
        Ok(Some(Self {
            output,
            origin,
            machine,
            root,
            sealed,
            work: Mutex::new(Work {
                unobserved_cargo: UNOBSERVED_CARGO.to_vec(),
                ..Work::default()
            }),
        }))
    }

    pub(super) fn sealed(&self) -> rust_mutants_sealed::Counted {
        self.sealed.clone()
    }

    pub(super) fn invalid(&self, detail: String) {
        let Ok(mut work) = self.work.lock() else {
            return;
        };
        work.error = Some(detail);
    }

    pub(super) fn observe(&self, payload: &Payload) {
        let Ok(mut work) = self.work.lock() else {
            return;
        };
        if let Err(detail) = apply(&mut work, payload) {
            work.error = Some(format!("{} {detail}", detail.code().code));
        }
    }
}

/// Why the cost fold could not keep an exact count.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
enum AccountingError {
    /// A counter reached the width of its field.
    #[error("{what} overflowed")]
    Overflowed {
        /// Which accounting overflowed.
        what: &'static str,
    },
    /// A note did not carry what its kind promises.
    #[error("{problem}")]
    Invalid {
        /// What the note lacked.
        problem: String,
    },
}

impl AccountingError {
    /// The stable code of this accounting failure.
    #[must_use]
    pub(crate) const fn code(&self) -> crate::error::ErrorCode {
        match self {
            Self::Overflowed { .. } => crate::error::RmCode::CostAccountingOverflowed.error_code(),
            Self::Invalid { .. } => crate::error::RmCode::CostAccountingInvalid.error_code(),
        }
    }
}

/// One failed launch, losslessly: the identity the request carried and why nothing started.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FailedLaunch {
    /// The bound key or unbound identity the attempted request carried.
    identity: String,
    /// Why no child started.
    cause: String,
}

/// A complete input key is 64 hexadecimal characters; anything else is an unbound reason.
fn bound(detail: &str) -> bool {
    detail.len() == 64 && detail.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn apply(work: &mut Work, payload: &Payload) -> Result<(), AccountingError> {
    if let Payload::Exec { exec } = payload {
        observed_cargo(work, exec)?;
    }
    if let Payload::Note { note } = payload {
        noted(work, note)?;
    }
    Ok(())
}

/// Observes one Cargo command the engine watched: a filename says it needs a role note, not what its work was.
fn observed_cargo(work: &mut Work, exec: &super::ExecRecord) -> Result<(), AccountingError> {
    let role = ExecutionRole::of(exec);
    let ran = started(&exec.stopped);
    if role.is_cargo() && ran {
        add(
            &mut work.observed_cargo_starts,
            "observed cargo start accounting",
        )?;
    }
    let input =
        serde_json::to_vec(&(&exec.argv, &exec.dir, &exec.env_names)).map_err(|source| {
            AccountingError::Invalid {
                problem: source.to_string(),
            }
        })?;
    let identity = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(input));
    let held = work.executions.entry(identity).or_insert(ExecutionWork {
        role,
        requests: 0,
        processes: 0,
        failed_launches: 0,
        duration_ms: 0,
    });
    add(&mut held.requests, "execution request accounting")?;
    if ran {
        add(&mut held.processes, "execution process accounting")?;
    } else {
        add(
            &mut held.failed_launches,
            "execution failed-launch accounting",
        )?;
    }
    sum(
        &mut held.duration_ms,
        exec.duration_ms,
        "execution duration accounting",
    )?;
    Ok(())
}

/// Whether the supervised run actually started a child.
const fn started(stopped: &crate::execute::Stopped) -> bool {
    !matches!(
        stopped,
        crate::execute::Stopped::NotStarted { .. }
            | crate::execute::Stopped::Cancelled { started: false }
    )
}

/// Folds one diagnostic note into the multiplicity it observes.
fn noted(work: &mut Work, note: &crate::trace::NoteRecord) -> Result<(), AccountingError> {
    if observed_tool(work, note)? {
        return Ok(());
    }
    match note.kind.as_str() {
        "fixture-cargo-build" => {
            let millis = note
                .detail
                .parse::<u64>()
                .map_err(|source| AccountingError::Invalid {
                    problem: format!("fixture build duration is invalid: {source}"),
                })?;
            add(&mut work.builds, "fixture build accounting")?;
            sum(&mut work.build_ms, millis, "fixture build accounting")?;
        }
        "fixture-build-request"
        | "fixture-build-process"
        | "fixture-build-failed"
        | "build-cache-hit"
        | "build-cache-miss"
        | "build-cache-unavailable" => identified(work, note)?,
        "cargo-built-units" => {
            let measured =
                note.detail
                    .parse::<u64>()
                    .map_err(|_invalid| AccountingError::Invalid {
                        problem: "compiled unit accounting is invalid".to_owned(),
                    })?;
            sum(&mut work.units, measured, "compiled unit accounting")?;
        }
        "sealed-platform-request" => {
            add(&mut work.platform_requests, "platform request accounting")?;
        }
        "sealed-platform-work" => {
            let spent = crate::strictjson::decode_str::<rust_mutants_sealed::Spent>(&note.detail)
                .map_err(|source| AccountingError::Invalid {
                problem: source.to_string(),
            })?;
            work.platform.push(spent);
        }
        "sealed-platform-work-invalid" => {
            return Err(AccountingError::Invalid {
                problem: note.detail.clone(),
            });
        }
        _ => {}
    }
    Ok(())
}

/// Folds actual probe roles and producer waits without guessing a role from an executable name.
fn observed_tool(
    work: &mut Work,
    note: &crate::trace::NoteRecord,
) -> Result<bool, AccountingError> {
    let (count, duration, label) = match note.kind.as_str() {
        "cargo-probe" => (&mut work.cargo_probes, &mut work.cargo_probe_ms, "cargo"),
        "cargo-metadata" => (
            &mut work.cargo_metadata,
            &mut work.cargo_metadata_ms,
            "cargo metadata",
        ),
        "rustc-probe" => (&mut work.rustc_probes, &mut work.rustc_probe_ms, "rustc"),
        "rustc-build" => (
            &mut work.rustc_builds,
            &mut work.rustc_build_ms,
            "rustc build",
        ),
        "tool-probe" => {
            noted_probe(work, &note.detail)?;
            return Ok(true);
        }
        "host-wait" => {
            let waited = crate::strictjson::decode_str(&note.detail).map_err(|source| {
                AccountingError::Invalid {
                    problem: format!("host wait is invalid: {source}"),
                }
            })?;
            work.host_waits.push(waited);
            return Ok(true);
        }
        _ => return Ok(false),
    };
    add(count, "tool probe accounting")?;
    sum(
        duration,
        probe_millis(&note.detail, label)?,
        "tool probe accounting",
    )?;
    Ok(true)
}

/// Keeps the actual operation and every request, including a never-started probe.
fn noted_probe(work: &mut Work, detail: &str) -> Result<(), AccountingError> {
    let probe: Probe =
        crate::strictjson::decode_str(detail).map_err(|source| AccountingError::Invalid {
            problem: format!("tool probe is invalid: {source}"),
        })?;
    let held = work.probes.entry(probe.identity).or_insert(ProbeWork {
        role: probe.role,
        requests: 0,
        processes: 0,
        failed_launches: 0,
        duration_ns: 0,
    });
    add(&mut held.requests, "probe request accounting")?;
    if probe.started {
        add(&mut held.processes, "probe process accounting")?;
    } else {
        add(&mut held.failed_launches, "probe launch failure accounting")?;
    }
    sum(
        &mut held.duration_ns,
        probe.duration_ns,
        "probe duration accounting",
    )
}

/// Folds one identity-carrying note: a request, a process, or a cache answer, each under its key or unbound reason.
fn identified(work: &mut Work, note: &crate::trace::NoteRecord) -> Result<(), AccountingError> {
    match note.kind.as_str() {
        "fixture-build-request" => {
            add(&mut work.build_requests, "build request accounting")?;
            if bound(&note.detail) {
                let held = work.build_keys.entry(note.detail.clone()).or_default();
                held.requests = raised(held.requests, "bound request accounting")?;
            } else {
                let held = work.unbound.entry(note.detail.clone()).or_default();
                held.requests = raised(held.requests, "unbound request accounting")?;
            }
        }
        "fixture-build-process" | "fixture-build-failed" => launched(work, note)?,
        "build-cache-hit" => {
            if !bound(&note.detail) {
                return Err(AccountingError::Invalid {
                    problem: "a cache hit names no complete input key".to_owned(),
                });
            }
            add(&mut work.build_hits, "cache hit accounting")?;
            let held = work.build_keys.entry(note.detail.clone()).or_default();
            held.hits = raised(held.hits, "bound hit accounting")?;
        }
        "build-cache-miss" => {
            add(&mut work.build_misses, "cache miss accounting")?;
            let (identity, reason) =
                note.detail
                    .split_once(' ')
                    .ok_or_else(|| AccountingError::Invalid {
                        problem: "a cache miss names neither a key nor a reason".to_owned(),
                    })?;
            if bound(identity) {
                let held = work.build_keys.entry(identity.to_owned()).or_default();
                held.misses = raised(held.misses, "bound miss accounting")?;
                let counted = held.reasons.entry(reason.to_owned()).or_default();
                *counted = raised(*counted, "miss reason accounting")?;
            } else {
                let held = work.unbound.entry(note.detail.clone()).or_default();
                held.misses = raised(held.misses, "unbound miss accounting")?;
            }
        }
        "build-cache-unavailable" => {
            let (identity, reason) =
                note.detail
                    .split_once(' ')
                    .ok_or_else(|| AccountingError::Invalid {
                        problem: "a refused write names neither a key nor a reason".to_owned(),
                    })?;
            if !bound(identity) {
                return Err(AccountingError::Invalid {
                    problem: "a refused write names no complete input key".to_owned(),
                });
            }
            let held = work.build_keys.entry(identity.to_owned()).or_default();
            let counted = held.refused_writes.entry(reason.to_owned()).or_default();
            *counted = raised(*counted, "refused write accounting")?;
        }
        _ => {}
    }
    Ok(())
}

fn add(count: &mut u64, what: &'static str) -> Result<(), AccountingError> {
    *count = raised(*count, what)?;
    Ok(())
}

/// Folds one launch outcome: an actual process or a failed launch, each under its identity and cause.
fn launched(work: &mut Work, note: &crate::trace::NoteRecord) -> Result<(), AccountingError> {
    if note.kind == "fixture-build-process" {
        if bound(&note.detail) {
            let held = work.build_keys.entry(note.detail.clone()).or_default();
            held.processes = raised(held.processes, "bound process accounting")?;
        } else {
            let held = work.unbound.entry(note.detail.clone()).or_default();
            held.processes = raised(held.processes, "unbound process accounting")?;
        }
        return Ok(());
    }
    let failed = crate::strictjson::decode_str::<FailedLaunch>(&note.detail).map_err(|source| {
        AccountingError::Invalid {
            problem: format!("a failed launch note is not its typed record: {source}"),
        }
    })?;
    let FailedLaunch { identity, cause } = failed;
    if bound(identity.as_str()) {
        let held = work.build_keys.entry(identity).or_default();
        held.failed_launches = raised(held.failed_launches, "bound failure accounting")?;
        let counted = held.launch_causes.entry(cause).or_default();
        *counted = raised(*counted, "launch cause accounting")?;
    } else {
        let held = work.unbound.entry(identity).or_default();
        held.failed_launches = raised(held.failed_launches, "unbound failure accounting")?;
        let counted = held.launch_causes.entry(cause).or_default();
        *counted = raised(*counted, "unbound launch cause accounting")?;
    }
    add(&mut work.launch_failures, "launch failure accounting")
}

fn probe_millis(detail: &str, program: &str) -> Result<u64, AccountingError> {
    detail
        .parse::<u64>()
        .map_err(|source| AccountingError::Invalid {
            problem: format!("a {program} probe duration is invalid: {source}"),
        })
}

fn raised(count: u64, what: &'static str) -> Result<u64, AccountingError> {
    count
        .checked_add(1)
        .ok_or(AccountingError::Overflowed { what })
}

fn sum(count: &mut u64, measured: u64, what: &'static str) -> Result<(), AccountingError> {
    count
        .checked_add(measured)
        .map(|next| *count = next)
        .ok_or(AccountingError::Overflowed { what })
}

impl Drop for Costs {
    fn drop(&mut self) {
        let Ok(work) = self.work.get_mut() else {
            return;
        };
        let record = Record {
            schema: "njutest-test-cost-v3",
            origin: &self.origin,
            machine: &self.machine,
            binary: self.origin.labels().0,
            test: self.origin.labels().1,
            root: &self.root,
            work,
            sealed: self.sealed.spent(),
        };
        match serde_json::to_writer(&mut self.output, &record) {
            Ok(()) => match self.output.write_all(b"\n") {
                Ok(()) | Err(_) => {}
            },
            Err(_incomplete_record_is_refused_by_the_cost_gate) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AccountingError, Payload, UNOBSERVED_CARGO, Work, apply};

    fn note(kind: &str, detail: &str) -> Payload {
        Payload::Note {
            note: crate::trace::NoteRecord {
                kind: kind.to_owned(),
                detail: detail.to_owned(),
            },
        }
    }

    fn exec(program: &str, argument: &str, stopped: crate::execute::Stopped) -> Payload {
        Payload::Exec {
            exec: crate::trace::ExecRecord {
                argv: vec![program.to_owned(), argument.to_owned()],
                dir: None,
                env_names: Vec::new(),
                timeout_ms: None,
                quiet_ms: None,
                stopped,
                duration_ms: 7,
                output_bytes: 0,
                output_sha256: None,
                output_truncated: false,
                output_path: None,
                error: None,
                output: Vec::new(),
            },
        }
    }

    fn started() -> crate::execute::Stopped {
        crate::execute::Stopped::Exited {
            exit: crate::runner::ProcessExit::Code(0),
        }
    }

    #[test]
    fn a_cargo_command_that_never_started_is_not_a_process() {
        let mut work = Work::default();
        apply(
            &mut work,
            &exec(
                "cargo",
                "build",
                crate::execute::Stopped::NotStarted {
                    cause: crate::execute::StartFailure::Missing,
                },
            ),
        )
        .unwrap();
        apply(
            &mut work,
            &exec(
                "cargo",
                "build",
                crate::execute::Stopped::Cancelled { started: false },
            ),
        )
        .unwrap();
        assert_eq!(work.builds, 0, "no child started, so no process is counted");
    }

    #[test]
    fn a_cargo_command_the_notes_do_not_own_is_not_build_work() {
        let mut work = Work::default();
        apply(&mut work, &exec("/opt/decoy/cargo", "build", started())).unwrap();
        assert_eq!(work.builds, 0, "a filename does not make a command a build");
    }

    #[test]
    fn a_build_command_is_counted_once_whatever_reports_it() {
        let mut work = Work::default();
        apply(&mut work, &exec("cargo", "build", started())).unwrap();
        apply(&mut work, &note("fixture-cargo-build", "5")).unwrap();
        assert_eq!(
            work.builds, 1,
            "an event and its note are one command, counted once"
        );
    }

    #[test]
    fn a_cold_build_then_a_repair_each_keep_their_reason_and_their_process() {
        let key = "ab".repeat(32);
        let mut work = Work {
            unobserved_cargo: UNOBSERVED_CARGO.to_vec(),
            ..Work::default()
        };
        for reason in [
            "cold: the compilation record is absent",
            "repair: the recorded artifact changed",
        ] {
            apply(&mut work, &note("fixture-build-request", &key)).unwrap();
            apply(
                &mut work,
                &note("build-cache-miss", &format!("{key} {reason}")),
            )
            .unwrap();
            apply(&mut work, &note("fixture-build-process", &key)).unwrap();
            apply(&mut work, &note("fixture-cargo-build", "10")).unwrap();
        }
        let held = work.build_keys.get(&key).unwrap();
        assert_eq!(held.requests, 2);
        assert_eq!(held.misses, 2);
        assert_eq!(held.processes, 2);
        assert_eq!(held.reasons.get(repair_reason()).copied(), Some(1));
        assert_eq!(work.builds, 2);
        assert_eq!(work.build_misses, 2);
    }

    #[test]
    fn a_hit_keeps_its_multiplicity_without_a_process() {
        let key = "cd".repeat(32);
        let mut work = Work::default();
        for _ in 0..2 {
            apply(&mut work, &note("fixture-build-request", &key)).unwrap();
            apply(&mut work, &note("build-cache-hit", &key)).unwrap();
        }
        let held = work.build_keys.get(&key).unwrap();
        assert_eq!(held.requests, 2);
        assert_eq!(held.hits, 2);
        assert_eq!(held.processes, 0);
    }

    #[test]
    fn an_unbound_command_never_gains_a_fabricated_key() {
        let mut work = Work::default();
        apply(
            &mut work,
            &note("fixture-build-request", "unbound: inherited environment"),
        )
        .unwrap();
        apply(
            &mut work,
            &note("build-cache-miss", "unbound: inherited environment"),
        )
        .unwrap();
        apply(
            &mut work,
            &note("fixture-build-process", "unbound: inherited environment"),
        )
        .unwrap();
        apply(&mut work, &note("fixture-cargo-build", "5")).unwrap();
        assert!(work.build_keys.is_empty());
        let held = work.unbound.get("unbound: inherited environment").unwrap();
        assert_eq!((held.requests, held.misses, held.processes), (1, 1, 1));
    }

    #[test]
    fn a_direct_build_publishes_its_reason_without_a_miss() {
        let mut work = Work::default();
        let identity = "direct: native edit oracle build";
        apply(&mut work, &note("fixture-build-request", identity)).unwrap();
        apply(&mut work, &note("fixture-build-process", identity)).unwrap();
        let held = work.unbound.get(identity).unwrap();
        assert_eq!((held.requests, held.misses, held.processes), (1, 0, 1));
    }

    #[test]
    fn a_refused_write_is_attributed_to_its_key() {
        let key = "ef".repeat(32);
        let mut work = Work::default();
        apply(
            &mut work,
            &note(
                "build-cache-unavailable",
                &format!("{key} the record could not be written"),
            ),
        )
        .unwrap();
        let held = work.build_keys.get(&key).unwrap();
        assert_eq!(
            held.refused_writes
                .get("the record could not be written")
                .copied(),
            Some(1),
        );
    }

    #[test]
    fn a_failed_launch_keeps_exactly_its_requests_identity() {
        let identity = "unbound: inherited environment";
        let key = "ab".repeat(32);
        let mut work = Work::default();
        for payload in [
            note("fixture-build-request", identity),
            note("build-cache-miss", identity),
            note(
                "fixture-build-failed",
                &serde_json::json!({"identity": identity, "cause": "no such executable: it was removed"}).to_string(),
            ),
        ] {
            apply(&mut work, &payload).expect("the failed launch folds");
        }
        assert_eq!(
            work.unbound.len(),
            1,
            "a failed launch invents no second identity: {:?}",
            work.unbound.keys().collect::<Vec<_>>()
        );
        let held = work.unbound.get(identity).expect("the request's identity");
        assert_eq!(
            (
                held.requests,
                held.misses,
                held.processes,
                held.failed_launches
            ),
            (1, 1, 0, 1)
        );
        let mut bound = Work::default();
        for payload in [
            note("fixture-build-request", &key),
            note(
                "build-cache-miss",
                &format!("{key} cold: the compilation record is absent"),
            ),
            note(
                "fixture-build-failed",
                &serde_json::json!({"identity": key, "cause": "cancelled before start"})
                    .to_string(),
            ),
        ] {
            apply(&mut bound, &payload).expect("the bound failure folds");
        }
        let held = bound.build_keys.get(&key).expect("the bound identity");
        assert_eq!(
            (
                held.requests,
                held.misses,
                held.processes,
                held.failed_launches
            ),
            (1, 1, 0, 1)
        );
    }

    #[test]
    fn every_accounting_failure_carries_its_stable_code() {
        assert_eq!(
            AccountingError::Overflowed {
                what: "fixture build accounting"
            }
            .code()
            .code,
            "RM7002"
        );
        assert_eq!(
            AccountingError::Invalid {
                problem: "a note lacked its cause".to_owned()
            }
            .code()
            .code,
            "RM7003"
        );
    }

    #[test]
    fn a_hit_or_a_refused_write_without_a_complete_key_is_refused() {
        let mut work = Work::default();
        apply(&mut work, &note("build-cache-hit", "not a key")).unwrap_err();
        apply(
            &mut work,
            &note("build-cache-unavailable", "also not a key no reason"),
        )
        .unwrap_err();
    }

    fn repair_reason() -> &'static str {
        "repair: the recorded artifact changed"
    }
}
