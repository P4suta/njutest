// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Compiling the tree and reading what the compiler said: the pristine gate, the source of every unit's file set, and the build validation and execution both stand on.

use std::ffi::OsString;
use std::time::Duration;

use super::depinfo::{Unit, units_of};
use super::locate::command_failed;
use super::messages::{Message, parse_messages};
use super::{CargoError, CargoErrorKind, CompilerObservation, Driver, InputIdentity};
use crate::error::{self, ErrorCode};
use crate::runner::{ProcessExit, Termination, run};
use crate::trace::ExecRecord;

/// How much of the message stream is kept.
const MESSAGE_OUTPUT_LIMIT: usize = 256 << 20;

/// Which command compiles the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileKind {
    /// `cargo check --all-targets`: every type question, no code generated.
    Check,
    /// `cargo test --all-targets --no-run`: the binaries a run executes, and every refusal that only happens once code is generated.
    Tests,
    /// `cargo build --all-targets --keep-going` under the test profile: the sealed build of every test harness but an example's, which builds each that can build for the sealed target whatever another refuses (ADR 0046).
    SealedTests,
    /// `cargo test --examples --no-run`: the sealed build of the examples, which only `cargo test` compiles as test harnesses.
    SealedExamples,
}

impl CompileKind {
    /// The command this kind runs, and what it asks of every target.
    const fn command(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Self::Check => ("check", &["--all-targets"]),
            Self::Tests => ("test", &["--all-targets", "--no-run"]),
            Self::SealedTests => ("build", &["--all-targets", "--keep-going"]),
            Self::SealedExamples => ("test", &["--examples", "--no-run"]),
        }
    }
}

/// What this compilation is asked to cover, before the flags that are the same either way.
fn arguments(kind: CompileKind, packages: &[String]) -> Vec<String> {
    let (command, rest) = kind.command();
    let mut args = vec![command.to_owned()];
    if packages.is_empty() || kind == CompileKind::Check {
        args.push("--workspace".to_owned());
    } else {
        for package in packages {
            args.push("--package".to_owned());
            args.push(package.clone());
        }
    }
    args.extend(rest.iter().map(|arg| (*arg).to_owned()));
    args
}

/// What a build is asked to compile, beyond the tree itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildConfig {
    /// The features to turn on, which cargo takes as one comma-separated argument.
    pub features: Vec<String>,
    /// Pass `--all-features`.
    pub all_features: bool,
    /// Pass `--no-default-features`.
    pub no_default_features: bool,
    /// The target triple to compile for.
    /// `None` is the host.
    pub target: Option<String>,
    /// The cargo profile to compile with.
    /// `None` is the command's own default.
    pub profile: Option<String>,
    /// How many compilation jobs cargo may run at once.
    /// `None` lets cargo choose.
    pub jobs: Option<u32>,
    /// Whether the compiler writes debug information into what it builds.
    pub debug: bool,
}

impl BuildConfig {
    /// Whether this asks for nothing cargo would not have done anyway.
    #[must_use]
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    /// The arguments that spell this configuration, in cargo's own order.
    #[must_use]
    pub fn arguments(&self) -> Vec<String> {
        let mut args = Vec::new();
        if self.no_default_features {
            args.push("--no-default-features".to_owned());
        }
        if self.all_features {
            args.push("--all-features".to_owned());
        }
        if !self.features.is_empty() {
            args.push("--features".to_owned());
            args.push(self.features.join(","));
        }
        for (flag, value) in [("--target", &self.target), ("--profile", &self.profile)] {
            if let Some(value) = value {
                args.push(flag.to_owned());
                args.push(value.clone());
            }
        }
        if let Some(jobs) = self.jobs {
            args.push("--jobs".to_owned());
            args.push(jobs.to_string());
        }
        args
    }

    /// Every argument that decides what a build of the tree is: this configuration's own, and what tells cargo to write no debug information, which every cargo command that builds the tree has to pass alike or rebuild what another built.
    #[must_use]
    pub fn cargo_arguments(&self) -> Vec<String> {
        let mut args = self.arguments();
        args.extend(self.without_debug_information());
        args
    }

    /// What tells cargo to write no debug information, when nothing asked for any.
    pub(crate) fn without_debug_information(&self) -> Vec<String> {
        if self.debug || self.profile.is_some() {
            return Vec::new();
        }
        ["profile.dev.debug=0", "profile.test.debug=0"]
            .into_iter()
            .flat_map(|setting| ["--config".to_owned(), setting.to_owned()])
            .collect()
    }
}

/// Configures [`compile`].
#[derive(Debug, Clone)]
pub struct CompileOptions {
    /// Which command to run.
    pub kind: CompileKind,
    /// `--target-dir`, with the members a build into it may compile, which [`compile`] settles before cargo reads a file.
    pub target_dir: super::BuildDir,
    /// Pass `--locked`.
    pub locked: bool,
    /// Pass `--offline`.
    pub offline: bool,
    /// How long the check may take.
    pub timeout: Option<Duration>,
    /// What this compilation alone adds to the toolchain's environment, such as the flags a coverage build needs.
    pub env: crate::vars::Variables,
    /// The member packages this compilation is about.
    /// Empty is the whole workspace, and a check is always about the whole workspace whatever this says.
    pub packages: Vec<String>,
    /// What the project is compiled as: its features, target, profile, and how many jobs cargo may use.
    pub build: BuildConfig,
}

impl CompileOptions {
    /// Configures a check in a target directory chosen by the caller.
    #[must_use]
    pub fn new(target_dir: super::BuildDir) -> Self {
        Self {
            kind: CompileKind::Check,
            target_dir,
            locked: false,
            offline: false,
            timeout: None,
            env: crate::vars::Variables::empty(),
            packages: Vec::new(),
            build: BuildConfig::default(),
        }
    }
}

/// The whole command line one compilation runs, which is what a person would have typed.
#[must_use]
pub fn compile_arguments(options: &CompileOptions) -> Vec<OsString> {
    let mut args: Vec<OsString> = arguments(options.kind, &options.packages)
        .into_iter()
        .map(OsString::from)
        .collect();
    args.push(OsString::from("--message-format=json"));
    if options.locked {
        args.push(OsString::from("--locked"));
    }
    if options.offline {
        args.push(OsString::from("--offline"));
    }
    args.push(OsString::from("--target-dir"));
    args.push(options.target_dir.path().as_os_str().to_owned());
    if options.kind == CompileKind::SealedTests && options.build.profile.is_none() {
        args.push(OsString::from("--profile"));
        args.push(OsString::from("test"));
    }
    args.extend(
        options
            .build
            .cargo_arguments()
            .into_iter()
            .map(OsString::from),
    );
    args
}

/// How the products one compilation answered with were established.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    /// An actual cargo process ran and printed these messages, so the compiler itself is the witness of what it built.
    Compiler,
    /// The engine's content-addressed record answered, after verifying every bound input and artifact digest against the tree as it stands, so the products are a verified reuse of a real compiler run, not a compiler run of this call.
    VerifiedReuse,
    /// A waiting request shares the completed producer refusal, without certifying complete products or an independent compiler witness.
    SharedRefusal,
}

/// What a compilation's answer may be established by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Witness {
    /// Any complete establishment: an actual compiler process, or the engine's verified record of one.
    Any,
    /// An actual compiler process must run: a verified record of an earlier process is reuse, not an independent witness, and may not answer.
    Compiler,
}

/// What a compilation produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compiled {
    pub(super) completion: Completion,
    pub(super) observation: CompilerObservation,
    /// Every message, in order, for attribution.
    pub messages: Vec<Message>,
    /// The units that produced an artifact, with their sources.
    /// A failed unit produces none, so on a failed check this is partial.
    pub units: Vec<Unit>,
    /// How these products were established.
    pub provenance: Provenance,
}

impl Compiled {
    /// How the build came out, as its one final record and cargo's exit code established it together.
    #[must_use]
    pub const fn completion(&self) -> Completion {
        self.completion
    }

    /// The retained actual producer and raw streams that established these products.
    #[must_use]
    pub const fn observation(&self) -> &CompilerObservation {
        &self.observation
    }

    /// The raw stderr from this result's original actual producer.
    #[must_use]
    pub fn stderr(&self) -> &[u8] {
        self.observation.stderr()
    }
}

/// The exit code a cargo process ended with by itself, which is the one thing its `build-finished` record is held to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exited {
    code: i32,
}

impl Exited {
    pub(super) const fn from_code(code: i32) -> Self {
        Self { code }
    }

    pub(super) const fn code(self) -> i32 {
        self.code
    }

    /// The code cargo ended `termination` with, or nothing where it ended some other way: a signal, a status nothing classified, a launch or supervision that failed, or a stop the run imposed, none of which is the compiler's answer about the build.
    #[must_use]
    pub const fn of(termination: &Termination) -> Option<Self> {
        match termination {
            Termination::Exited(ProcessExit::Code(code)) => Some(Self { code: *code }),
            Termination::Exited(ProcessExit::Signal(_) | ProcessExit::Unknown)
            | Termination::NotStarted { .. }
            | Termination::TimedOut
            | Termination::Stalled
            | Termination::StoppedByMonitor
            | Termination::Answered
            | Termination::MonitorFailed { .. }
            | Termination::Cancelled { .. }
            | Termination::WaitFailed { .. } => None,
        }
    }
}

/// How a cargo build that ran to its end came out, read from its exit code and its one final `build-finished` record together and never from either alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completion {
    /// Every unit compiled, and cargo exited 0.
    Built,
    /// The compiler refused something, and cargo exited with a code other than 0.
    Refused,
}

impl Completion {
    /// How the build that ended as `exited`, having printed `messages`, came out.
    ///
    /// # Errors
    /// [`CompletionError`] when the messages are not one final `build-finished` record the exit code agrees with.
    pub fn of(messages: &[Message], exited: Exited) -> Result<Self, CompletionError> {
        let records = messages
            .iter()
            .filter(|message| matches!(message, Message::BuildFinished(_)))
            .count();
        let last = match messages.last() {
            Some(Message::BuildFinished(finished)) => Some(*finished),
            Some(
                Message::CompilerArtifact(_)
                | Message::CompilerMessage(_)
                | Message::BuildScriptExecuted(_)
                | Message::Other { .. },
            )
            | None => None,
        };
        let code = exited.code;
        match (records, last) {
            (0, _) if code != 0 => Err(CompletionError::Unfinished { code }),
            (1, Some(finished)) if finished.success() == (code == 0) => Ok(if code == 0 {
                Self::Built
            } else {
                Self::Refused
            }),
            (1, Some(finished)) => Err(CompletionError::Contradicted {
                success: finished.success(),
                code,
            }),
            (records, last) => Err(CompletionError::Ambiguous {
                records,
                ends: last.is_some(),
            }),
        }
    }
}

/// Why what cargo printed is not the record of one finished build its exit code agrees with.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CompletionError {
    /// Cargo exited with a code other than 0 and printed no `build-finished` record: it stopped before it finished a build, so no unit was refused and what it said on its error stream is the answer.
    #[error("cargo exited with {code} before it finished a build")]
    Unfinished {
        /// The code it exited with.
        code: i32,
    },
    /// The stream does not end in exactly one `build-finished` record, so which build it reports, if any, is a guess.
    #[error(
        "the message stream holds {records} build-finished records and {ending}, where a \
         finished build ends in exactly one",
        ending = if *ends { "ends in one" } else { "does not end in one" }
    )]
    Ambiguous {
        /// How many records it holds.
        records: usize,
        /// Whether its last message is one.
        ends: bool,
    },
    /// The one record says what the exit code does not.
    #[error("build-finished says success={success}, but cargo exited with {code}")]
    Contradicted {
        /// What the record says.
        success: bool,
        /// The code cargo exited with.
        code: i32,
    },
}

impl CompletionError {
    /// The stable code of this failure: a cargo that stopped before it finished a build failed as a command, and a stream that is not one finished build cannot be read.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Unfinished { .. } => error::CARGO_COMMAND_FAILED,
            Self::Ambiguous { .. } | Self::Contradicted { .. } => error::CARGO_MESSAGE_UNPARSABLE,
        }
    }
}

/// What one build compiled: every unit with the files the compiler read for it, and every build script with what it told the linker, as cargo reported them rather than as a directory holds them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compilation {
    /// Every unit that produced an artifact.
    pub units: Vec<Unit>,
    /// Every build script that ran.
    pub build_scripts: Vec<super::messages::BuildScript>,
}

impl Compilation {
    /// What `compiled` reported.
    #[must_use]
    pub fn of(compiled: &Compiled) -> Self {
        Self {
            units: compiled.units.clone(),
            build_scripts: compiled
                .messages
                .iter()
                .filter_map(|message| match message {
                    Message::BuildScriptExecuted(script) => Some(script.clone()),
                    Message::CompilerArtifact(_)
                    | Message::CompilerMessage(_)
                    | Message::BuildFinished { .. }
                    | Message::Other { .. } => None,
                })
                .collect(),
        }
    }
}

/// Compiles the tree in the driver's directory and reads what it said.
///
/// # Errors
/// [`CargoErrorKind::BuildLedger`] when the target directory cannot be settled, [`CargoErrorKind::CommandFailed`] when cargo itself could not run or timed out, [`CargoErrorKind::MessageUnparsable`] for a stream that is not messages, and the dep-info errors of [`units_of`].
pub fn compile(driver: &Driver<'_>, options: &CompileOptions) -> Result<Compiled, CargoError> {
    compile_with(driver, options, Witness::Any)
}

/// Compiles the tree and reads what it said, established as `witness` allows.
///
/// # Errors
/// As [`compile`], and a `Witness::Compiler` build is one the engine's verified record may not answer, so it always starts a process.
pub fn compile_with(
    driver: &Driver<'_>,
    options: &CompileOptions,
    witness: Witness,
) -> Result<Compiled, CargoError> {
    let mut spec = compilation_spec(driver, options)?;
    let trace = match driver.toolchain.env() {
        Some(vars) => driver.trace.costed(vars, driver.dir).map_err(|source| {
            CargoError::new(
                CargoErrorKind::CommandFailed,
                format!("test cost diagnostic: {source}"),
            )
        })?,
        None => driver.trace.clone(),
    };
    if driver.cancel.is_cancelled() {
        return Err(CargoError::new(
            CargoErrorKind::Cancelled,
            "the compilation was cancelled",
        ));
    }
    let preparation = super::build_cache::Preparation::own(options.target_dir.path(), &trace)
        .map_err(|source| {
            CargoError::new(
                CargoErrorKind::BuildLedger,
                format!(
                    "cannot own compiler preparation in {}",
                    options.target_dir.path().display()
                ),
            )
            .with_source(source)
        })?;
    if driver.cancel.is_cancelled() {
        return Err(CargoError::new(
            CargoErrorKind::Cancelled,
            "the compilation was cancelled",
        ));
    }
    let (identity, request, reused) = cached(
        (driver, options),
        witness,
        (&mut spec, &trace),
        &preparation,
    );
    if let Some(compiled) = reused {
        if driver.cancel.is_cancelled() {
            return Err(CargoError::new(
                CargoErrorKind::Cancelled,
                "the compilation was cancelled",
            ));
        }
        let compiled = compiled?;
        options.target_dir.settle()?;
        return Ok(compiled);
    }
    produce(
        (driver, options),
        witness,
        (&spec, &trace, &preparation),
        (identity, request),
    )
}

fn produce(
    (driver, options): (&Driver<'_>, &CompileOptions),
    witness: Witness,
    (spec, trace, preparation): (
        &crate::runner::Spec,
        &crate::trace::Recorder,
        &super::build_cache::Preparation,
    ),
    (identity, request): (InputIdentity, Option<super::build_cache::Request>),
) -> Result<Compiled, CargoError> {
    if let Err(error) = prepare(options, witness, request.is_some()) {
        failed(
            (driver, options, request.as_ref()),
            (spec, None),
            (&error, super::build_cache::FailedStage::Preparation),
            (preparation, trace),
        );
        return Err(error);
    }
    let result = run(spec, driver.cancel);
    note_launch(trace, &identity, &result);
    let mut compiled = match completed((driver, options), (spec, &result, trace), identity, witness)
    {
        Ok(compiled) => compiled,
        Err(error) => {
            failed(
                (driver, options, request.as_ref()),
                (spec, Some(&result)),
                (&error, super::build_cache::FailedStage::Process),
                (preparation, trace),
            );
            return Err(error);
        }
    };
    let request = request.map(|request| match witness {
        Witness::Any => request,
        Witness::Compiler => request.independent(compiled.observation().id()),
    });
    if let (Some(request), Some(env)) = (request, &spec.env)
        && let Err(source) = request.unchanged(driver, options).and_then(|()| {
            let exit = Exited::of(&result.termination)
                .ok_or_else(|| std::io::Error::other("compiler exit is not observed"))?;
            request.write(
                &mut compiled,
                (&result.stdout, env, options.target_dir.path()),
                (exit, preparation),
            )
        })
    {
        trace.note(
            "build-cache-unavailable",
            &format!("{} {source}", request.key),
        );
        trace.note("fixture-build-uncacheable", &source.to_string());
        let error = CargoError::new(
            CargoErrorKind::BuildLedger,
            format!(
                "cannot publish immutable compiler products for {}",
                request.key
            ),
        )
        .with_source(source);
        failed(
            (driver, options, Some(&request)),
            (spec, Some(&result)),
            (&error, super::build_cache::FailedStage::Publication),
            (preparation, trace),
        );
        return Err(error);
    }
    Ok(compiled)
}

pub(super) fn note_launch(
    trace: &crate::trace::Recorder,
    identity: &InputIdentity,
    result: &crate::runner::RunResult,
) {
    if result.leader.is_some() {
        trace.note("fixture-build-process", identity.detail());
    } else {
        let cause = match result.termination.error() {
            Some(failure) => failure.to_string(),
            None => "cancelled before start".to_owned(),
        };
        let failed = serde_json::json!({"identity": identity.detail(), "cause": cause});
        trace.note("fixture-build-failed", &failed.to_string());
    }
}

fn prepare(options: &CompileOptions, witness: Witness, bound: bool) -> Result<(), CargoError> {
    options.target_dir.settle()?;
    if witness == Witness::Compiler || bound {
        options.target_dir.independent()?;
    }
    Ok(())
}

fn failed(
    (driver, options, request): (
        &Driver<'_>,
        &CompileOptions,
        Option<&super::build_cache::Request>,
    ),
    (spec, result): (&crate::runner::Spec, Option<&crate::runner::RunResult>),
    (error, stage): (&CargoError, super::build_cache::FailedStage),
    (preparation, trace): (&super::build_cache::Preparation, &crate::trace::Recorder),
) {
    let Some(request) = request else {
        return;
    };
    let published = request
        .unchanged(driver, options)
        .and_then(|()| request.fail((error, stage), (spec, result), preparation));
    match published {
        Ok(()) => trace.note(
            "compiler-preparation-failed",
            &serde_json::json!({
                "identity": request.key, "stage": stage, "cause": error.message(),
            })
            .to_string(),
        ),
        Err(source) => trace.note("compiler-failure-publication-refused", &source.to_string()),
    }
}

fn compilation_spec(
    driver: &Driver<'_>,
    options: &CompileOptions,
) -> Result<crate::runner::Spec, CargoError> {
    let mut spec = driver
        .toolchain
        .command(driver.dir, compile_arguments(options));
    if !options.env.is_empty() {
        let Some(mut env) = spec.env.clone() else {
            return Err(CargoError::new(
                CargoErrorKind::CommandFailed,
                "the compilation adds variables to the toolchain's environment, and the \
                 toolchain was given none: it inherits this process's, which only the \
                 composition root reads, so there is nothing to add them to",
            ));
        };
        env.overlay(&options.env);
        spec.env = Some(env);
    }
    spec.structured_stdout = Some(MESSAGE_OUTPUT_LIMIT);
    spec.timeout = options.timeout;
    Ok(spec)
}

pub(super) fn input_identity(
    (driver, options): (&Driver<'_>, &CompileOptions),
) -> Result<InputIdentity, CargoError> {
    let mut spec = compilation_spec(driver, options)?;
    Ok(match &mut spec.env {
        Some(env) => match super::build_cache::Request::of(driver, options, env) {
            Ok(request) => InputIdentity::Complete(request.key),
            Err(source) => InputIdentity::Unbound(format!("unbound: {source}")),
        },
        None => InputIdentity::Unbound("unbound: inherited environment".to_owned()),
    })
}

fn cached(
    (driver, options): (&Driver<'_>, &CompileOptions),
    witness: Witness,
    (spec, trace): (&mut crate::runner::Spec, &crate::trace::Recorder),
    preparation: &super::build_cache::Preparation,
) -> (
    InputIdentity,
    Option<super::build_cache::Request>,
    Option<Result<Compiled, CargoError>>,
) {
    match &mut spec.env {
        Some(env) => match super::build_cache::Request::of(driver, options, env) {
            Ok(request) => {
                let identity = InputIdentity::Complete(request.key.clone());
                trace.note("fixture-build-request", identity.detail());
                trace.note("build-cache-bound", &request.key);
                if witness == Witness::Compiler {
                    trace.note("compiler-witness", identity.detail());
                    trace.note(
                        "build-cache-miss",
                        &format!("{} independent-control", request.key),
                    );
                    return (identity, Some(request), None);
                }
                match request.failure((env, preparation)) {
                    Ok(error) => {
                        trace.note("compiler-failure-shared", &request.key);
                        return (identity, Some(request), Some(Err(error)));
                    }
                    Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
                    Err(source) => trace.note("compiler-failure-not-shared", &source.to_string()),
                }
                match request.read(driver, options, (env, preparation)) {
                    Ok(compiled) => {
                        let kind = match compiled.completion() {
                            Completion::Built => "build-cache-hit",
                            Completion::Refused => "build-cache-refusal",
                        };
                        trace.note(kind, &request.key);
                        trace.note("cargo-built-units", "0");
                        return (identity, Some(request), Some(Ok(compiled)));
                    }
                    Err(source) => {
                        let class = if source.kind() == std::io::ErrorKind::NotFound {
                            "cold"
                        } else {
                            "repair"
                        };
                        trace.note(
                            "build-cache-miss",
                            &format!("{} {class}: {source}", request.key),
                        );
                    }
                }
                (identity, Some(request), None)
            }
            Err(source) => {
                let identity = InputIdentity::Unbound(format!("unbound: {source}"));
                trace.note("fixture-build-request", identity.detail());
                trace.note("fixture-build-uncacheable", &source.to_string());
                trace.note("build-cache-miss", identity.detail());
                if witness == Witness::Compiler {
                    trace.note("compiler-witness", identity.detail());
                }
                (identity, None, None)
            }
        },
        None => {
            let identity = InputIdentity::Unbound("unbound: inherited environment".to_owned());
            trace.note("fixture-build-request", identity.detail());
            trace.note("fixture-build-uncacheable", identity.detail());
            trace.note("build-cache-miss", identity.detail());
            if witness == Witness::Compiler {
                trace.note("compiler-witness", identity.detail());
            }
            (identity, None, None)
        }
    }
}

fn completed(
    (driver, options): (&Driver<'_>, &CompileOptions),
    (spec, result, trace): (
        &crate::runner::Spec,
        &crate::runner::RunResult,
        &crate::trace::Recorder,
    ),
    identity: InputIdentity,
    witness: Witness,
) -> Result<Compiled, CargoError> {
    let millis = u64::try_from(result.duration.as_millis()).map_err(|_overflow| {
        CargoError::new(
            CargoErrorKind::CommandFailed,
            "fixture build duration exceeds its diagnostic width",
        )
    })?;
    if result.leader.is_some() {
        trace.note("fixture-cargo-build", &millis.to_string());
    }
    trace.exec_result(ExecRecord::of(spec, result));
    if driver.cancel.is_cancelled() {
        return Err(CargoError::new(
            CargoErrorKind::Cancelled,
            "the compilation was cancelled",
        ));
    }
    if let Termination::Cancelled { .. } = &result.termination {
        return Err(CargoError::new(
            CargoErrorKind::Cancelled,
            "the compilation was cancelled",
        ));
    }
    let Some(exited) = Exited::of(&result.termination) else {
        return Err(command_failed(spec, result));
    };
    if result.stdout_truncated {
        return Err(CargoError::new(
            CargoErrorKind::MessageUnparsable,
            "the compiler printed more than the engine keeps",
        ));
    }
    let messages = parse_messages(&result.stdout)?;
    let units = fresh_units(&messages)?;
    trace.note("cargo-built-units", &units.to_string());
    let completion = match Completion::of(&messages, exited) {
        Ok(completion) => completion,
        Err(CompletionError::Unfinished { .. }) => return Err(command_failed(spec, result)),
        Err(
            unread @ (CompletionError::Ambiguous { .. } | CompletionError::Contradicted { .. }),
        ) => {
            return Err(CargoError::new(
                CargoErrorKind::MessageUnparsable,
                unread.to_string(),
            ));
        }
    };
    let units = units_of(&messages, driver.dir)?;
    options.target_dir.record_reads(&units)?;
    Ok(Compiled {
        completion,
        observation: CompilerObservation::actual((spec, result), identity, witness)?,
        messages,
        units,
        provenance: Provenance::Compiler,
    })
}

/// Counts only compiler artifacts Cargo actually rebuilt.
pub(super) fn fresh_units(messages: &[Message]) -> Result<u64, CargoError> {
    messages
        .iter()
        .try_fold(0_u64, |count, message| match message {
            Message::CompilerArtifact(artifact) if !artifact.fresh => {
                count.checked_add(1).ok_or_else(|| {
                    CargoError::new(
                        CargoErrorKind::MessageUnparsable,
                        "compiled unit accounting overflowed",
                    )
                })
            }
            Message::CompilerArtifact(_)
            | Message::CompilerMessage(_)
            | Message::BuildScriptExecuted(_)
            | Message::BuildFinished(_)
            | Message::Other { .. } => Ok(count),
        })
}
