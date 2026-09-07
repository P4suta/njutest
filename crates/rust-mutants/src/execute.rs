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
use crate::instrument::{ACTIVE_ENV, CATALOG_ENV, STALE_CATALOG_EXIT, TOUCH_ENV};
use crate::outcome::Outcome;
use crate::runner::{Cancel, EXIT_CODE_UNAVAILABLE, RunResult, Spec, run};
use crate::trace::{ExecRecord, Recorder};

/// Selects the mutant to probe rather than to activate. Reserved here so that a stale value from an outer probe run is stripped, and used by the probe phase.
pub const PROBE_ENV: &str = "RUST_MUTANTS_PROBE";

/// Every variable the engine owns. A test process sees exactly the ones this run set, never one an outer run left behind.
pub const RESERVED_ENV: [&str; 4] = [ACTIVE_ENV, CATALOG_ENV, PROBE_ENV, TOUCH_ENV];

/// The variables a run composes for every test process it starts, which it therefore never lets one inherit.
///
/// The four reserved ones say which mutation is active and what the guards are
/// to record, and an inherited one would decide what somebody else's run
/// measured — or append this run's touches to a file another run is reading.
/// `LLVM_PROFILE_FILE` is the fifth for a different reason: a measurement of
/// this engine sets it, and an instrumented test process that inherited it
/// would write over the very measurement that started the run. Removing it is
/// not enough on its own — an instrumented binary with no path writes
/// `default_*.profraw` into its working directory, which is the tree being
/// measured — so a run puts a path of its own in its place.
pub const COMPOSED_ENV: [&str; 5] = [
    ACTIVE_ENV,
    CATALOG_ENV,
    PROBE_ENV,
    TOUCH_ENV,
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
    /// Whether the target is built with the libtest harness.
    ///
    /// A target with `harness = false` is a program that prints what it likes
    /// and says what it found by exiting, so its exit status is the whole
    /// answer and there is no summary line to read. Reading one anyway leaves
    /// every mutation of such a target undecided.
    pub harness: bool,
    /// What a run could not establish about this target, each named.
    pub limitations: Vec<String>,
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
            harness: true,
            limitations: Vec::new(),
            cargo_env: Vec::new(),
            through: Vec::new(),
        }
    }

    /// Whether the target is built with the libtest harness, which decides how its silence is read.
    #[must_use]
    pub const fn with_harness(mut self, harness: bool) -> Self {
        self.harness = harness;
        self
    }

    /// What a run could not establish about this target.
    #[must_use]
    pub fn with_limitations(mut self, limitations: Vec<String>) -> Self {
        self.limitations = limitations;
        self
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

/// What each test of one run said, by name.
///
/// The summary line says how many, and a person reading a survivor needs to
/// know which: the test that killed a mutant is the sentence a report can
/// hand somebody, and the tests that passed are the ones that could have.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lines {
    /// Every test that passed, in the order the harness printed them.
    pub passed: Vec<String>,
    /// Every test that failed.
    pub failed: Vec<String>,
    /// Every test that was ignored.
    pub ignored: Vec<String>,
}

impl Lines {
    /// Whether the harness printed no verdict at all.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.passed.is_empty() && self.failed.is_empty() && self.ignored.is_empty()
    }
}

/// Reads every `test <name> ... <verdict>` line of a captured output.
///
/// The failures block at the end names each failed test a second time, and a
/// name is one test however often the harness prints it, so only the verdict
/// lines are read.
#[must_use]
pub fn parse_lines(output: &[u8]) -> Lines {
    let text = String::from_utf8_lossy(output);
    let mut lines = Lines::default();
    for line in text.lines() {
        let Some((name, verdict)) = verdict_of(line) else {
            continue;
        };
        match verdict {
            "ok" => lines.passed.push(name.to_owned()),
            "FAILED" => lines.failed.push(name.to_owned()),
            _ if verdict.starts_with("ignored") => lines.ignored.push(name.to_owned()),
            _ => {}
        }
    }
    lines
}

/// The name and verdict of one `test <name> ... <verdict>` line.
fn verdict_of(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim_end().strip_prefix("test ")?;
    let (name, verdict) = rest.rsplit_once(" ... ")?;
    let name = name.trim();
    (!name.is_empty()).then_some((name, verdict.trim()))
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
pub const fn outcome_of(observed: Observation, summary: Option<Summary>, harness: bool) -> Outcome {
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
    if observed.exit_code == crate::probe::runtime::UNAVAILABLE_EXIT {
        return Outcome::Errored;
    }
    if observed.exit_code != 0 {
        return Outcome::Killed;
    }
    if !harness {
        return Outcome::Survived;
    }
    match summary {
        Some(summary) if !summary.ran_nothing() => Outcome::Survived,
        _ => Outcome::Inconclusive,
    }
}

/// Where the build put each binary, by package and by target name.
///
/// An integration test finds a binary of its own package through
/// `CARGO_BIN_EXE_<name>`, and cargo points it at the file it actually built:
/// under the profile the build used, with whatever extension this platform
/// puts on an executable. Composing the path from a profile name and a target
/// name guesses at both, and on Windows guesses wrong.
fn binaries_built(messages: &[Message]) -> BTreeMap<String, BTreeMap<String, PathBuf>> {
    let mut found: BTreeMap<String, BTreeMap<String, PathBuf>> = BTreeMap::new();
    for message in messages {
        let Message::CompilerArtifact(artifact) = message else {
            continue;
        };
        let Some(executable) = &artifact.executable else {
            continue;
        };
        if artifact.profile.test || !artifact.target.is_bin() {
            continue;
        }
        let _replaced = found
            .entry(artifact.package_id.clone())
            .or_default()
            .insert(artifact.target.name.clone(), executable.clone());
    }
    found
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
    /// The signal the process died from, on the platforms that have them.
    pub signal: Option<i32>,
    /// Every test that failed, by name, which is what a report hands a person reading a kill.
    pub failed_tests: Vec<String>,
    /// Every test that passed, by name, which is every test that could have noticed the mutation and did not.
    pub passed_tests: Vec<String>,
    /// Every test the harness was told to skip.
    pub ignored_tests: Vec<String>,
}

/// Whether the target said anything about the mutation, which is what decides whether the next target is asked.
///
/// A run walks the targets a route holds and stops at the first that answers.
/// `Inconclusive` is the outcome of a libtest target that ran no test or
/// printed no summary: it said nothing, so the next target is asked. A target
/// with no libtest harness answers by exiting and is never inconclusive, and
/// a documentation target with no examples is inconclusive exactly like a
/// libtest one that ran nothing.
#[must_use]
pub const fn answered(outcome: Outcome) -> bool {
    !matches!(outcome, Outcome::Inconclusive)
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
    let lines = parse_lines(&result.output);
    MutantResult {
        outcome: outcome_of(Observation::of(&result), summary, target.harness),
        target: target.id.clone(),
        exit_code: result.exit_code,
        duration: result.duration,
        output: result.output,
        summary,
        tests_run: summary.map(|summary| summary.tests_run()),
        signal: result.signal,
        failed_tests: lines.failed,
        passed_tests: lines.passed,
        ignored_tests: lines.ignored,
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
    /// What the project is compiled as: its features, target, profile, and how many jobs cargo may use.
    pub build: crate::cargo::BuildConfig,
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
            build: options.build.clone(),
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
    let binaries = binaries_built(messages);
    let mut harnesses: BTreeMap<String, BTreeMap<(String, String), bool>> = BTreeMap::new();
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
        let mut env = cargo_environment(
            package,
            kind,
            target_dir,
            binaries
                .get(&artifact.package_id)
                .unwrap_or(&BTreeMap::new()),
        );
        env.extend(built_by_a_script(messages, &artifact.package_id));
        let harness = harnesses
            .entry(package.id.clone())
            .or_insert_with(|| crate::cargo::manifest::harnesses(&package.manifest_path))
            .get(&(kind.name().to_owned(), artifact.target.name.clone()))
            .copied()
            .unwrap_or(true);
        targets.push(
            TestTarget::new(
                target_id(&package.name, kind, &artifact.target.name),
                package.name.clone(),
                kind,
                artifact.target.name.clone(),
                executable.clone(),
                package.manifest_dir().to_path_buf(),
            )
            .with_harness(harness)
            .with_limitations(if harness {
                Vec::new()
            } else {
                vec![crate::limitation::CUSTOM_HARNESS.to_owned()]
            })
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
            targets.push(
                TestTarget::new(
                    target_id(&package.name, TargetKind::Doc, &target.name),
                    package.name.clone(),
                    TargetKind::Doc,
                    target.name.clone(),
                    cargo.to_path_buf(),
                    package.manifest_dir().to_path_buf(),
                )
                .with_through(through)
                .with_limitations(vec![crate::limitation::DOCTESTS_ROUTED_BY_FILE.to_owned()]),
            );
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
    binaries: &BTreeMap<String, PathBuf>,
) -> Vec<(OsString, OsString)> {
    let mut env = package_environment(package);
    if matches!(kind, TargetKind::Test | TargetKind::Example) {
        if let Some(target_dir) = target_dir {
            env.push((
                OsString::from("CARGO_TARGET_TMPDIR"),
                target_dir.join("tmp").into_os_string(),
            ));
        }
        for target in &package.targets {
            if !target.is_bin() {
                continue;
            }
            if let Some(built) = binaries.get(&target.name) {
                env.push((
                    OsString::from(format!("CARGO_BIN_EXE_{}", target.name)),
                    built.as_os_str().to_owned(),
                ));
            }
        }
    }
    env
}

/// What cargo tells every unit of a package about the package.
///
/// Cargo sets each of these for every compilation and every test process, and
/// sets the empty string where the manifest says nothing rather than leaving
/// the variable out. A run that starts the test process itself has to do the
/// same, because `env!("CARGO_PKG_DESCRIPTION")` compiles either way and a
/// test that reads one back would otherwise see something the project never
/// wrote.
#[must_use]
pub fn package_environment(package: &Package) -> Vec<(OsString, OsString)> {
    let (major, minor, patch, pre) = version_parts(&package.version);
    let said = |value: Option<&str>| OsString::from(value.unwrap_or_default());
    let named =
        |value: Option<&Path>| value.map_or_else(OsString::new, |one| one.as_os_str().to_owned());
    vec![
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
        (
            OsString::from("CARGO_PKG_VERSION_MAJOR"),
            OsString::from(major),
        ),
        (
            OsString::from("CARGO_PKG_VERSION_MINOR"),
            OsString::from(minor),
        ),
        (
            OsString::from("CARGO_PKG_VERSION_PATCH"),
            OsString::from(patch),
        ),
        (OsString::from("CARGO_PKG_VERSION_PRE"), OsString::from(pre)),
        (
            OsString::from("CARGO_PKG_AUTHORS"),
            OsString::from(package.authors.join(":")),
        ),
        (
            OsString::from("CARGO_PKG_DESCRIPTION"),
            said(package.description.as_deref()),
        ),
        (
            OsString::from("CARGO_PKG_HOMEPAGE"),
            said(package.homepage.as_deref()),
        ),
        (
            OsString::from("CARGO_PKG_REPOSITORY"),
            said(package.repository.as_deref()),
        ),
        (
            OsString::from("CARGO_PKG_LICENSE"),
            said(package.license.as_deref()),
        ),
        (
            OsString::from("CARGO_PKG_LICENSE_FILE"),
            named(package.license_file.as_deref()),
        ),
        (
            OsString::from("CARGO_PKG_RUST_VERSION"),
            said(package.rust_version.as_deref()),
        ),
        (
            OsString::from("CARGO_PKG_README"),
            named(package.readme.as_deref()),
        ),
    ]
}

/// A semantic version cut the way cargo cuts it: three numbers and whatever follows the first hyphen.
fn version_parts(version: &str) -> (&str, &str, &str, &str) {
    let (numbers, pre) = version.split_once('-').unwrap_or((version, ""));
    let (numbers, _build) = numbers.split_once('+').unwrap_or((numbers, ""));
    let mut parts = numbers.split('.');
    (
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
        pre,
    )
}
