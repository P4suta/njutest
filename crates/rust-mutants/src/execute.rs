// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Running one test process per mutant, and reading what its exit status means.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cargo::{
    CargoError, CargoErrorKind, CompileKind, CompileOptions, Driver, Message, Package, Target,
    compile,
};
use crate::instrument::{ACTIVE_ENV, CATALOG_ENV, STALE_CATALOG_EXIT};
use crate::outcome::Outcome;
use crate::runner::{Cancel, EXIT_CODE_UNAVAILABLE, RunResult, Spec, run};
use crate::trace::{ExecRecord, Recorder};

/// Selects the mutant to probe rather than to activate. Reserved here so that a stale value from an outer probe run is stripped, and used by the probe phase.
pub const PROBE_ENV: &str = "RUST_MUTANTS_PROBE";

/// Every variable the engine owns. A test process sees exactly the ones this run set, never one an outer run left behind.
pub const RESERVED_ENV: [&str; 3] = [ACTIVE_ENV, CATALOG_ENV, PROBE_ENV];

/// The variables a run composes for every test process it starts, which it therefore never lets one inherit.
///
/// The three reserved ones say which mutation is active, and an inherited one
/// would decide what somebody else's run measured. `LLVM_PROFILE_FILE` is the
/// fourth for a different reason: a measurement of this engine sets it, and an
/// instrumented test process that inherited it would write over the very
/// measurement that started the run. Removing it is not enough on its own —
/// an instrumented binary with no path writes `default_*.profraw` into its
/// working directory, which is the tree being measured — so a run puts a path
/// of its own in its place.
pub const COMPOSED_ENV: [&str; 4] = [
    ACTIVE_ENV,
    CATALOG_ENV,
    PROBE_ENV,
    crate::coverage::PROFILE_ENV,
];

/// The name a test process writes its coverage profile under, when the run is not the one measuring.
///
/// `%p` is the process and `%m` the binary, which is what keeps two test
/// processes of one run from writing one file.
pub const SPILLED_PROFILE: &str = "spilled-coverage-%p-%m.profraw";

/// The kinds of target that carry tests the engine runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum TargetKind {
    /// The library's own unit tests.
    Lib,
    /// A binary's own unit tests.
    Bin,
    /// An integration test.
    Test,
    /// An example built with `test = true`.
    Example,
    /// A procedural macro crate's own unit tests, which are an ordinary test binary.
    ProcMacro,
    /// A library's documentation examples, which cargo runs and rustdoc compiles.
    Doc,
}

impl TargetKind {
    /// Every kind, in the order a report lists them.
    pub const ALL: [Self; 6] = [
        Self::Lib,
        Self::Bin,
        Self::Test,
        Self::Example,
        Self::ProcMacro,
        Self::Doc,
    ];

    /// The name used in a target id and in reports.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Lib => "lib",
            Self::Bin => "bin",
            Self::Test => "test",
            Self::Example => "example",
            Self::ProcMacro => "proc-macro",
            Self::Doc => "doc",
        }
    }

    /// The kind named `name`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }

    /// The kind of a cargo target, or `None` for one that carries no tests the engine runs (a build script, a bench).
    ///
    /// A proc-macro crate's `--test` build is an ordinary executable that links
    /// the crate as a library and runs its unit tests in a process of its own,
    /// so what those tests reach is measurable exactly like anything else. What
    /// is not is the expansion, which runs inside the compiler during the build.
    #[must_use]
    pub fn of(target: &Target) -> Option<Self> {
        if target.is_custom_build() || target.is_bench() {
            None
        } else if target.is_proc_macro() {
            Some(Self::ProcMacro)
        } else if target.is_lib() {
            Some(Self::Lib)
        } else if target.is_bin() {
            Some(Self::Bin)
        } else if target.is_test() {
            Some(Self::Test)
        } else if target.is_example() {
            Some(Self::Example)
        } else {
            None
        }
    }
}

/// The stable name of a test target: `package/kind/name`.
#[must_use]
pub fn target_id(package: &str, kind: TargetKind, name: &str) -> String {
    format!("{package}/{}/{name}", kind.name())
}

/// One built test binary.
///
/// Built rather than spelled as a literal, so that a field added later is a
/// method a caller may ignore instead of a compile error in every caller.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TestTarget {
    /// `package/kind/name`.
    pub id: String,
    /// The package that owns it.
    pub package: String,
    /// What kind of target it is.
    pub kind: TargetKind,
    /// The target's name.
    pub name: String,
    /// The binary cargo built.
    pub executable: PathBuf,
    /// The directory it runs in: the package's manifest directory, which is what cargo uses and what a test reading a relative path expects.
    pub cwd: PathBuf,
    /// What cargo sets for this target that the parent environment does not have: `CARGO_MANIFEST_DIR`, `CARGO_PKG_*`, `CARGO_BIN_EXE_*`.
    pub cargo_env: Vec<(OsString, OsString)>,
    /// The arguments before the harness's own, for a target cargo runs rather than one the engine starts itself. Empty for a binary, and then `executable` is the binary.
    ///
    /// A library's documentation examples are compiled by rustdoc while cargo
    /// runs them, so there is no binary to start: the target is `cargo test
    /// --doc …`, and what the harness is told goes after a `--`. Such a
    /// harness is also asked for one example by name without `--exact`, which
    /// it does not honour: a documented example is named
    /// `src/lib.rs - f (line 7)`, and a name ending in the line it is on
    /// cannot be the beginning of another one.
    pub through: Vec<OsString>,
}

impl TestTarget {
    /// One built test binary, by everything cargo says about it that is not optional.
    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "these six are what cargo says about a target and none of them has a sensible \
                  default: a builder that let one be forgotten would build a target that names \
                  no package or runs in no directory"
    )]
    pub fn new(
        id: impl Into<String>,
        package: impl Into<String>,
        kind: TargetKind,
        name: impl Into<String>,
        executable: PathBuf,
        cwd: PathBuf,
    ) -> Self {
        Self {
            id: id.into(),
            package: package.into(),
            kind,
            name: name.into(),
            executable,
            cwd,
            cargo_env: Vec::new(),
            through: Vec::new(),
        }
    }

    /// What cargo sets for this target that the parent environment does not have.
    #[must_use]
    pub fn with_cargo_env(mut self, env: Vec<(OsString, OsString)>) -> Self {
        self.cargo_env = env;
        self
    }

    /// The arguments before the harness's own, for a target cargo runs rather than one the engine starts.
    #[must_use]
    pub fn with_through(mut self, through: Vec<OsString>) -> Self {
        self.through = through;
        self
    }
}

/// The libtest summary line of one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    /// Whether the line said `ok`.
    pub ok: bool,
    /// Tests that passed.
    pub passed: u32,
    /// Tests that failed.
    pub failed: u32,
    /// Tests that were ignored.
    pub ignored: u32,
    /// Benchmarks that were measured.
    pub measured: u32,
    /// Tests the filter removed.
    pub filtered_out: u32,
}

impl Summary {
    /// How many tests actually ran.
    #[must_use]
    pub const fn tests_run(&self) -> u32 {
        self.passed.saturating_add(self.failed)
    }

    /// Whether nothing ran at all, which is what a filter matching no test looks like: exit 0 with no evidence in it.
    #[must_use]
    pub const fn ran_nothing(&self) -> bool {
        self.tests_run() == 0
    }
}

/// Reads the last `test result:` line of a captured output.
#[must_use]
pub fn parse_summary(output: &[u8]) -> Option<Summary> {
    let text = String::from_utf8_lossy(output);
    text.lines().rev().find_map(parse_summary_line)
}

fn parse_summary_line(line: &str) -> Option<Summary> {
    let rest = line.trim().strip_prefix("test result: ")?;
    let (verdict, counts) = rest.split_once('.')?;
    let mut summary = Summary {
        ok: verdict.trim() == "ok",
        passed: 0,
        failed: 0,
        ignored: 0,
        measured: 0,
        filtered_out: 0,
    };
    let mut seen: u32 = 0;
    for part in counts.split(';') {
        let part = part.trim();
        let Some((count, label)) = part.split_once(' ') else {
            continue;
        };
        let Ok(count) = count.parse::<u32>() else {
            continue;
        };
        match label.trim() {
            "passed" => summary.passed = count,
            "failed" => summary.failed = count,
            "ignored" => summary.ignored = count,
            "measured" => summary.measured = count,
            "filtered out" => summary.filtered_out = count,
            _ => continue,
        }
        seen = seen.saturating_add(1);
    }
    (seen > 0).then_some(summary)
}

/// What a run of a test binary looked like from outside, which is all the outcome policy reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Observation {
    /// The process could not be started or supervised at all.
    pub unstarted: bool,
    /// The timeout fired and the tree was killed.
    pub timed_out: bool,
    /// The exit status, or [`EXIT_CODE_UNAVAILABLE`].
    pub exit_code: i32,
    /// Whether the runtime said the binary was built from another catalog, which is how that is recognised through a process that did not exit with it.
    pub stale_catalog: bool,
}

impl Observation {
    /// What a supervised run looked like.
    #[must_use]
    pub fn of(result: &RunResult) -> Self {
        Self {
            unstarted: result.error.is_some(),
            timed_out: result.timed_out,
            exit_code: result.exit_code,
            stale_catalog: said(&result.output, crate::instrument::STALE_CATALOG_MARKER),
        }
    }
}

/// Whether `output` holds `needle`.
fn said(output: &[u8], needle: &str) -> bool {
    output
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

/// What one run of a test binary establishes about the mutant that was active during it. See the module documentation for the order.
#[must_use]
pub const fn outcome_of(observed: Observation, summary: Option<Summary>) -> Outcome {
    if observed.unstarted {
        return Outcome::Errored;
    }
    if observed.timed_out {
        return Outcome::TimedOut;
    }
    if observed.exit_code == EXIT_CODE_UNAVAILABLE {
        return Outcome::NotRun;
    }
    if observed.exit_code == STALE_CATALOG_EXIT || observed.stale_catalog {
        return Outcome::Errored;
    }
    if observed.exit_code != 0 {
        return Outcome::Killed;
    }
    match summary {
        Some(summary) if !summary.ran_nothing() => Outcome::Survived,
        _ => Outcome::Inconclusive,
    }
}

/// What a package's own build script left for every unit of that package: where it wrote, and what it put in the environment.
///
/// Cargo tells a unit that reads a build script where the script wrote, and
/// the unit reads it back at run time through `OUT_DIR`. A test process the
/// engine starts itself is told nothing unless the engine says it, and a test
/// that reads a file its build script generated then fails for a reason that
/// is not the mutation.
fn built_by_a_script(messages: &[Message], package_id: &str) -> Vec<(OsString, OsString)> {
    let mut found = Vec::new();
    for message in messages {
        let Message::BuildScriptExecuted(script) = message else {
            continue;
        };
        if script.package_id != package_id {
            continue;
        }
        if let Some(out_dir) = &script.out_dir {
            found.push((OsString::from("OUT_DIR"), out_dir.as_os_str().to_owned()));
        }
        for (name, value) in &script.env {
            found.push((OsString::from(name), OsString::from(value)));
        }
    }
    found
}

/// The environment one test process runs with: the base the workspace was opened with, the variables cargo sets for the target, the activation, and a temporary directory of the worker's own.
#[must_use]
pub fn environment(
    context: &Context<'_>,
    target: &TestTarget,
    scratch: Option<&Path>,
) -> Vec<(OsString, OsString)> {
    let (base, active, cargo) = (context.base_env, context.active, context.cargo);
    let mut env: BTreeMap<OsString, OsString> = base
        .iter()
        .filter(|(name, _)| {
            !COMPOSED_ENV
                .iter()
                .any(|composed| name == OsStr::new(composed))
        })
        .cloned()
        .collect();
    env.extend(target.cargo_env.iter().cloned());
    if let Some(cargo) = cargo {
        env.insert(OsString::from("CARGO"), cargo.as_os_str().to_owned());
    }
    if let Some(sysroot) = context.sysroot {
        let (name, value) = library_path(sysroot, base);
        env.insert(name, value);
    }
    if let Some((id, catalog)) = active {
        env.insert(OsString::from(ACTIVE_ENV), OsString::from(id));
        env.insert(OsString::from(CATALOG_ENV), OsString::from(catalog));
    }
    if let Some(probe) = context.probe {
        env.insert(OsString::from(PROBE_ENV), probe.as_os_str().to_owned());
    }
    match (context.profile, scratch) {
        (Some(profile), _) => {
            env.insert(
                OsString::from(crate::coverage::PROFILE_ENV),
                profile.as_os_str().to_owned(),
            );
        }
        (None, Some(scratch)) => {
            env.insert(
                OsString::from(crate::coverage::PROFILE_ENV),
                scratch.join(SPILLED_PROFILE).into_os_string(),
            );
        }
        (None, None) => {}
    }
    if let Some(scratch) = scratch {
        for name in ["TMPDIR", "TMP", "TEMP"] {
            env.insert(OsString::from(name), scratch.as_os_str().to_owned());
        }
    }
    env.into_iter().collect()
}

/// The variable a dynamically linked test binary is found through, and what it should hold.
///
/// A test binary is started directly rather than through `cargo test`, and one
/// cargo built with `prefer-dynamic` — every proc-macro crate's own tests, and
/// anything else a project asks it for — then cannot find `libstd`. Cargo sets
/// this for the test binaries it runs; so does the engine, from the toolchain
/// rustc named as its own, keeping whatever the environment already had after
/// it.
fn library_path(sysroot: &Path, base: &[(OsString, OsString)]) -> (OsString, OsString) {
    let name = if cfg!(target_os = "macos") {
        "DYLD_FALLBACK_LIBRARY_PATH"
    } else if cfg!(windows) {
        "PATH"
    } else {
        "LD_LIBRARY_PATH"
    };
    let separator = OsString::from(if cfg!(windows) { ";" } else { ":" });
    let mut value = sysroot.join("lib").into_os_string();
    for triple in rustlib_targets(sysroot) {
        value.push(&separator);
        value.push(triple.into_os_string());
    }
    if let Some((_, existing)) = base
        .iter()
        .find(|(key, _)| key == OsStr::new(name))
        .filter(|(_, existing)| !existing.is_empty())
    {
        value.push(&separator);
        value.push(existing);
    }
    (OsString::from(name), value)
}

/// Every `lib/rustlib/<triple>/lib` the toolchain holds, which is where the target's own `libstd` is.
fn rustlib_targets(sysroot: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(sysroot.join("lib").join("rustlib")) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path().join("lib"))
        .filter(|path| path.is_dir())
        .collect();
    found.sort();
    found
}

/// One execution to make.
#[derive(Debug, Clone)]
pub struct ExecRequest<'a> {
    target: &'a TestTarget,
    test: Option<String>,
    args: Vec<String>,
    timeout: Option<Duration>,
    scratch: Option<PathBuf>,
}

impl<'a> ExecRequest<'a> {
    /// Runs every test of `target`.
    #[must_use]
    pub const fn new(target: &'a TestTarget) -> Self {
        Self {
            target,
            test: None,
            args: Vec::new(),
            timeout: None,
            scratch: None,
        }
    }

    /// Runs exactly the named test.
    #[must_use]
    pub fn with_test(mut self, test: impl Into<String>) -> Self {
        self.test = Some(test.into());
        self
    }

    /// Passes further arguments to the harness.
    #[must_use]
    pub fn with_args(mut self, args: impl IntoIterator<Item = String>) -> Self {
        self.args = args.into_iter().collect();
        self
    }

    /// Bounds the run.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = timeout;
        self
    }

    /// Points the process's temporary directory at a directory of its own.
    #[must_use]
    pub fn with_scratch(mut self, scratch: impl Into<PathBuf>) -> Self {
        self.scratch = Some(scratch.into());
        self
    }

    /// The target this runs.
    #[must_use]
    pub const fn target(&self) -> &TestTarget {
        self.target
    }

    /// The command line the binary receives. A named test is passed as a filter with `--exact`, so a name that is a prefix of another cannot drag it in.
    #[must_use]
    pub fn argv(&self) -> Vec<OsString> {
        let mut argv = vec![self.target.executable.clone().into_os_string()];
        if !self.target.through.is_empty() {
            argv.extend(self.target.through.iter().cloned());
            argv.push(OsString::from("--"));
        }
        if let Some(test) = &self.test {
            argv.push(OsString::from(test));
            if self.target.through.is_empty() {
                argv.push(OsString::from("--exact"));
            }
        }
        argv.extend(self.args.iter().map(OsString::from));
        argv
    }
}

/// What a test process runs with: the environment the workspace was opened with, and the mutant to activate (its identity and the catalog it came from), or `None` for the instrumented baseline.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// The environment the workspace was opened with.
    pub base_env: &'a [(OsString, OsString)],
    /// The cargo that built the tree, which cargo itself puts in `CARGO` for every process it runs.
    pub cargo: Option<&'a Path>,
    /// The toolchain directory a dynamically linked test binary finds `libstd` under. `None` starts it with whatever the environment already said.
    pub sysroot: Option<&'a Path>,
    /// The mutant to activate: `(identity, catalog digest)`.
    pub active: Option<(&'a str, &'a str)>,
    /// Where a probe process appends what it infected. `None` runs a process that records nothing.
    pub probe: Option<&'a Path>,
    /// Where a coverage-instrumented process writes what it executed. `None` runs a process that measures nothing.
    pub profile: Option<&'a Path>,
}

/// What one mutant execution established.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MutantResult {
    /// What the execution says about the mutant.
    pub outcome: Outcome,
    /// The target that ran.
    pub target: String,
    /// The exit status, or [`EXIT_CODE_UNAVAILABLE`].
    pub exit_code: i32,
    /// How long it took, supervision included.
    pub duration: Duration,
    /// The tail of the combined output.
    pub output: Vec<u8>,
    /// The harness's summary line, when it printed one.
    pub summary: Option<Summary>,
    /// How many tests ran, when the summary said.
    pub tests_run: Option<u32>,
}

/// Runs one test process and reads what it means.
#[must_use]
pub fn exec(
    request: &ExecRequest<'_>,
    context: &Context<'_>,
    cancel: &Cancel,
    trace: &Recorder,
) -> MutantResult {
    let target = request.target;
    let mut spec = Spec::new(request.argv());
    spec.dir = Some(target.cwd.clone());
    spec.env = Some(environment(context, target, request.scratch.as_deref()));
    spec.timeout = request.timeout;
    let result = run(&spec, cancel);
    trace.exec(ExecRecord::of(&spec, &result));
    let summary = parse_summary(&result.output);
    MutantResult {
        outcome: outcome_of(Observation::of(&result), summary),
        target: target.id.clone(),
        exit_code: result.exit_code,
        duration: result.duration,
        output: result.output,
        summary,
        tests_run: summary.map(|summary| summary.tests_run()),
    }
}

/// Configures [`build`].
#[derive(Debug, Clone, Default)]
pub struct BuildOptions {
    /// `--target-dir`.
    pub target_dir: Option<PathBuf>,
    /// Pass `--locked`.
    pub locked: bool,
    /// Pass `--offline`.
    pub offline: bool,
    /// The member packages whose test binaries are wanted. Empty is the whole workspace.
    pub packages: Vec<String>,
}

/// Builds the test binaries of a tree and reports them.
///
/// # Errors
/// Whatever stopped cargo from building, and a message stream that could
/// not be read.
pub fn build(
    driver: &Driver<'_>,
    packages: &[Package],
    options: &BuildOptions,
) -> Result<Vec<TestTarget>, CargoError> {
    let compiled = compile(
        driver,
        &CompileOptions {
            kind: CompileKind::Tests,
            packages: options.packages.clone(),
            target_dir: options.target_dir.clone(),
            locked: options.locked,
            offline: options.offline,
            timeout: None,
            env: Vec::new(),
        },
    )?;
    if !compiled.success {
        return Err(CargoError::new(
            CargoErrorKind::CommandFailed,
            "the test binaries could not be built",
        ));
    }
    Ok(targets_of(
        &compiled.messages,
        packages,
        options.target_dir.as_deref(),
    ))
}

/// The test binaries a build produced, in target id order.
#[must_use]
pub fn targets_of(
    messages: &[Message],
    packages: &[Package],
    target_dir: Option<&Path>,
) -> Vec<TestTarget> {
    let mut targets = Vec::new();
    for message in messages {
        let Message::CompilerArtifact(artifact) = message else {
            continue;
        };
        let Some(executable) = &artifact.executable else {
            continue;
        };
        if !artifact.profile.test {
            continue;
        }
        let Some(kind) = TargetKind::of(&artifact.target) else {
            continue;
        };
        let Some(package) = packages
            .iter()
            .find(|package| package.id == artifact.package_id)
        else {
            continue;
        };
        let mut env = cargo_environment(package, kind, target_dir);
        env.extend(built_by_a_script(messages, &artifact.package_id));
        targets.push(
            TestTarget::new(
                target_id(&package.name, kind, &artifact.target.name),
                package.name.clone(),
                kind,
                artifact.target.name.clone(),
                executable.clone(),
                package.manifest_dir().to_path_buf(),
            )
            .with_cargo_env(env),
        );
    }
    targets.sort_by(|a, b| a.id.cmp(&b.id));
    targets.dedup_by(|a, b| a.id == b.id);
    targets
}

/// One target for each library whose documentation cargo would run, which is a target this engine does not start itself.
///
/// A documentation example is compiled by rustdoc while cargo runs it, so
/// there is no binary in the build's messages to find: what there is, is a
/// command. `doctest = false` on the library is cargo's own way of saying
/// there is nothing to run, and it is honoured.
///
/// `packages` is the workspace's own members and never the whole resolved
/// graph. A dependency's documentation is not this run's to measure, and
/// asking cargo to run it asks cargo to resolve that dependency's own
/// dev-dependencies, which a lock file for this workspace never pinned.
#[must_use]
pub fn documentation_targets(
    members: &[&Package],
    cargo: &Path,
    arguments: &[OsString],
) -> Vec<TestTarget> {
    let mut targets = Vec::new();
    for package in members {
        for target in &package.targets {
            if !target.is_lib() || target.is_proc_macro() || !target.doctest {
                continue;
            }
            let mut through = vec![OsString::from("test"), OsString::from("--doc")];
            through.extend(arguments.iter().cloned());
            through.push(OsString::from("--package"));
            through.push(OsString::from(&package.name));
            targets.push(TestTarget {
                id: target_id(&package.name, TargetKind::Doc, &target.name),
                package: package.name.clone(),
                kind: TargetKind::Doc,
                name: target.name.clone(),
                executable: cargo.to_path_buf(),
                cwd: package.manifest_dir().to_path_buf(),
                cargo_env: Vec::new(),
                through,
            });
        }
    }
    targets.sort_by(|a, b| a.id.cmp(&b.id));
    targets.dedup_by(|a, b| a.id == b.id);
    targets
}

/// What cargo sets for a test process, reproduced from the metadata.
fn cargo_environment(
    package: &Package,
    kind: TargetKind,
    target_dir: Option<&Path>,
) -> Vec<(OsString, OsString)> {
    let mut env: Vec<(OsString, OsString)> = vec![
        (
            OsString::from("CARGO_MANIFEST_DIR"),
            package.manifest_dir().as_os_str().to_owned(),
        ),
        (
            OsString::from("CARGO_MANIFEST_PATH"),
            package.manifest_path.as_os_str().to_owned(),
        ),
        (
            OsString::from("CARGO_PKG_NAME"),
            OsString::from(&package.name),
        ),
        (
            OsString::from("CARGO_PKG_VERSION"),
            OsString::from(&package.version),
        ),
    ];
    if matches!(kind, TargetKind::Test | TargetKind::Example) {
        if let Some(target_dir) = target_dir {
            env.push((
                OsString::from("CARGO_TARGET_TMPDIR"),
                target_dir.join("tmp").into_os_string(),
            ));
        }
        for target in &package.targets {
            if target.is_bin()
                && let Some(target_dir) = target_dir
            {
                env.push((
                    OsString::from(format!("CARGO_BIN_EXE_{}", target.name)),
                    target_dir.join("debug").join(&target.name).into_os_string(),
                ));
            }
        }
    }
    env
}
