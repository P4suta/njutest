// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One owned actual capture producer and its complete immutable inventory.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::super::build_cache::{FailedStage, File, Preparation, Request, file, tree};
use super::super::{
    BuildDir, CargoError, CargoErrorKind, CompileOptions, CompilerObservation, Driver,
    InputIdentity, Witness, products_name,
};
use super::{DoctestCapture, Exited, REPORT_LIMIT, capture_arguments, command_failed};
use crate::runner::{RunResult, Spec, run};
use crate::trace::{ExecRecord, Recorder};

mod runtime;

const SCHEMA: &str = "rust-mutants-capture-products-v2";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Kind {
    Program,
    Listing,
    Ignored,
    Run,
    NativeRun,
}

impl Kind {
    const fn name(self) -> &'static str {
        match self {
            Self::Program => "capture-program",
            Self::Listing => "doctest-listing",
            Self::Ignored => "doctest-ignored",
            Self::Run => "doctest-run",
            Self::NativeRun => "native-doctest-run",
        }
    }

    const fn of(baked: crate::sealed::doctest::Baked) -> Self {
        match baked {
            crate::sealed::doctest::Baked::List => Self::Listing,
            crate::sealed::doctest::Baked::ListIgnored => Self::Ignored,
            crate::sealed::doctest::Baked::Run => Self::Run,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: String,
    key: String,
    kind: Kind,
    report: Vec<u8>,
    stdout: Vec<u8>,
    observation: CompilerObservation,
    exit: i32,
    files: BTreeMap<PathBuf, File>,
    runtime: runtime::Attestation,
}

#[derive(Debug, Clone, Copy)]
enum Origin {
    Published,
    Actual,
}

#[derive(Debug)]
struct Product {
    origin: Origin,
    directory: PathBuf,
    report: Vec<u8>,
    observation: CompilerObservation,
    runtime: runtime::Inputs,
}

/// An actual doctest report and captured binaries kept together under immutable ownership.
#[derive(Debug)]
pub struct PreparedDoctests(Product);

impl PreparedDoctests {
    /// The exact rustdoc report separated from the same producer's Cargo message records.
    #[must_use]
    pub fn report(&self) -> &[u8] {
        &self.0.report
    }

    /// The immutable inventory this report's original actual producer captured.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.0.directory
    }

    /// The original actual process, retained unchanged on reuse.
    #[must_use]
    pub const fn observation(&self) -> &CompilerObservation {
        &self.0.observation
    }

    pub(in crate::cargo) fn verify_native_runtime_inputs(&self) -> Result<(), CargoError> {
        self.0.runtime.verify().map_err(refused)
    }
}

fn refused(source: io::Error) -> CargoError {
    CargoError::new(
        CargoErrorKind::BuildLedger,
        "cannot establish owned capture products",
    )
    .with_source(source)
}

fn trace(driver: &Driver<'_>) -> Result<Recorder, CargoError> {
    match driver.toolchain.env() {
        Some(env) => driver
            .trace
            .costed(env, driver.dir)
            .map_err(|source| refused(io::Error::other(source))),
        None => Ok(driver.trace.clone()),
    }
}

enum Binding {
    Complete(Box<Request>),
    Unbound(String),
}

impl Binding {
    fn request(&self) -> Option<&Request> {
        match self {
            Self::Complete(request) => Some(request),
            Self::Unbound(_) => None,
        }
    }

    fn identity(&self) -> InputIdentity {
        match self {
            Self::Complete(request) => InputIdentity::Complete(request.key.clone()),
            Self::Unbound(reason) => InputIdentity::Unbound(reason.clone()),
        }
    }
}

fn binding(
    (driver, options): (&Driver<'_>, &CompileOptions),
    spec: &mut Spec,
    (kind, paths): (Kind, &[PathBuf]),
    trace: &Recorder,
) -> Binding {
    let result = match &mut spec.env {
        Some(env) => Request::of(driver, options, env)
            .and_then(|request| request.augment(kind.name(), &spec.argv, paths))
            .and_then(|request| {
                if kind != Kind::Program {
                    plain_sources(&request)?;
                }
                Ok(request)
            }),
        None => Err(io::Error::other("an inherited capture environment")),
    };
    let binding = match result {
        Ok(request) => Binding::Complete(Box::new(request)),
        Err(source) => Binding::Unbound(format!("unbound: {source}")),
    };
    trace.note(
        if kind == Kind::Program {
            "capture-program-request"
        } else {
            "fixture-build-request"
        },
        binding.identity().detail(),
    );
    if kind != Kind::Program
        && let Some(request) = binding.request()
    {
        trace.note("build-cache-bound", &request.key);
    }
    binding
}

fn record_path(directory: &Path, key: &str) -> io::Result<PathBuf> {
    Ok(directory.join(format!("{}.capture.json", crate::keyed::name(key)?)))
}

fn inventory(directory: &Path) -> io::Result<BTreeMap<PathBuf, File>> {
    let mut files = BTreeMap::new();
    tree(directory, Path::new(""), &mut files)?;
    files
        .into_iter()
        .map(|(path, state)| {
            let relative = path.strip_prefix(directory).map_err(io::Error::other)?;
            Ok((relative.to_path_buf(), state))
        })
        .collect()
}

fn read(
    (directory, request): (&Path, &Request),
    kind: Kind,
    spec: &Spec,
    identities: &super::super::build_cache::toolchain::Identities,
) -> io::Result<Product> {
    let record: Record =
        crate::strictjson::decode_slice(&std::fs::read(record_path(directory, &request.key)?)?)
            .map_err(io::Error::other)?;
    if record.schema != SCHEMA
        || record.key != request.key
        || record.kind != kind
        || !record
            .observation
            .verifies(&record.stdout, &request.key, record.exit)
    {
        return Err(io::Error::other("an unverified capture producer"));
    }
    let directory = directory.join(products_name(&request.key, record.observation.id())?);
    if inventory(&directory)? != record.files {
        return Err(io::Error::other("capture products changed"));
    }
    let (report, mut messages) = streams(&record.stdout, kind)?;
    if report != record.report {
        return Err(io::Error::other(
            "capture report differs from its actual producer",
        ));
    }
    request.capture_messages(&mut messages, &directory)?;
    let dir = spec
        .dir
        .as_deref()
        .ok_or_else(|| io::Error::other("capture source identity"))?;
    let units = super::super::units_of(&messages, dir).map_err(io::Error::other)?;
    if !request.covers(&units)? {
        return Err(io::Error::other("unverified captured compiler inputs"));
    }
    verify(kind, &record.report, &directory)?;
    match (&record.runtime, kind) {
        (runtime::Attestation::Original(_), Kind::NativeRun)
        | (
            runtime::Attestation::NotNative,
            Kind::Program | Kind::Listing | Kind::Ignored | Kind::Run,
        ) => {}
        (runtime::Attestation::NotNative, Kind::NativeRun)
        | (
            runtime::Attestation::Original(_),
            Kind::Program | Kind::Listing | Kind::Ignored | Kind::Run,
        ) => return Err(io::Error::other("capture runtime provenance differs")),
    }
    let runtime = record
        .runtime
        .restore(&directory, &record.files, identities)?;
    Ok(Product {
        origin: Origin::Published,
        directory,
        report: record.report,
        observation: record.observation,
        runtime,
    })
}

fn verify(kind: Kind, report: &[u8], directory: &Path) -> io::Result<()> {
    match kind {
        Kind::Program => {
            file(&directory.join(format!("capture{}", std::env::consts::EXE_SUFFIX)))?;
        }
        Kind::Listing | Kind::Ignored => {
            let held = crate::sealed::doctest::Held::read(directory)?;
            let listing = crate::sealed::doctest::listing(report)
                .map_err(|source| io::Error::other(source.to_string()))?;
            crate::sealed::doctest::merged_binaries(&listing, &held)
                .map_err(|source| io::Error::other(source.to_string()))?;
        }
        Kind::Run | Kind::NativeRun => {
            let held = crate::sealed::doctest::Held::read(directory)?;
            crate::sealed::doctest::captured(report, &held)
                .map_err(|source| io::Error::other(source.to_string()))?;
        }
    }
    Ok(())
}

fn streams(stdout: &[u8], kind: Kind) -> io::Result<(Vec<u8>, Vec<super::super::Message>)> {
    if kind == Kind::Program {
        return Ok((stdout.to_vec(), Vec::new()));
    }
    let mut report = Vec::new();
    let mut messages = Vec::new();
    for line in stdout.split_inclusive(|byte| *byte == b'\n') {
        if line.starts_with(b"{") {
            messages.extend(super::super::parse_messages(line).map_err(io::Error::other)?);
        } else {
            report.extend_from_slice(line);
        }
    }
    Ok((report, messages))
}

fn publish(
    (directory, staging): (&Path, tempfile::TempDir),
    (request, kind): (&Request, Kind),
    (result, observation): (&RunResult, CompilerObservation),
    (report, runtime): (Vec<u8>, runtime::Inputs),
) -> io::Result<Product> {
    let files = inventory(staging.path())?;
    let product = directory.join(products_name(&request.key, observation.id())?);
    std::fs::rename(staging.path(), &product)?;
    if inventory(&product)? != files {
        return Err(io::Error::other(
            "capture products changed during publication",
        ));
    }
    let exited = Exited::of(&result.termination)
        .ok_or_else(|| io::Error::other("an incomplete capture process"))?;
    if kind == Kind::NativeRun {
        runtime.verify()?;
    }
    let record = Record {
        schema: SCHEMA.to_owned(),
        key: request.key.clone(),
        kind,
        report: report.clone(),
        stdout: result.stdout.clone(),
        observation: observation.clone(),
        exit: exited.code(),
        files,
        runtime: runtime.attestation(),
    };
    let mut pending = tempfile::NamedTempFile::new_in(directory)?;
    pending.write_all(&serde_json::to_vec(&record).map_err(io::Error::other)?)?;
    pending
        .persist(record_path(directory, &request.key)?)
        .map_err(io::Error::other)?;
    Ok(Product {
        origin: Origin::Published,
        directory: product,
        report,
        observation,
        runtime,
    })
}

fn completed(
    (driver, options): (&Driver<'_>, &CompileOptions),
    (spec, binding): (&Spec, &Binding),
    (directory, staging, preparation): (&Path, tempfile::TempDir, &Preparation),
    (kind, trace): (Kind, &Recorder),
) -> Result<Product, CargoError> {
    let result = run(spec, driver.cancel);
    trace.exec_result(ExecRecord::of(spec, &result));
    if kind != Kind::Program {
        super::super::compile::note_launch(trace, &binding.identity(), &result);
    }
    if driver.cancel.is_cancelled() || Exited::of(&result.termination).is_none() {
        let error = command_failed(spec, &result);
        failed(
            (binding.request(), options),
            (driver, spec),
            (&error, preparation),
            (trace, Some(&result), FailedStage::Process),
        );
        return Err(error);
    }
    if result.stdout_truncated || kind == Kind::Program && !result.succeeded() {
        let error = command_failed(spec, &result);
        failed(
            (binding.request(), options),
            (driver, spec),
            (&error, preparation),
            (trace, Some(&result), FailedStage::Process),
        );
        return Err(error);
    }
    let product = products(
        (driver, options),
        (spec, binding),
        (directory, staging),
        (kind, trace, &result),
    );
    match product {
        Ok(product) => {
            match product.origin {
                Origin::Published => preparation
                    .publish(product.observation.id())
                    .map_err(refused)?,
                Origin::Actual => {}
            }
            Ok(product)
        }
        Err(error) => {
            failed(
                (binding.request(), options),
                (driver, spec),
                (&error, preparation),
                (trace, Some(&result), FailedStage::Publication),
            );
            Err(error)
        }
    }
}

fn products(
    (driver, options): (&Driver<'_>, &CompileOptions),
    (spec, binding): (&Spec, &Binding),
    (directory, staging): (&Path, tempfile::TempDir),
    (kind, trace, result): (Kind, &Recorder, &RunResult),
) -> Result<Product, CargoError> {
    let observation =
        CompilerObservation::actual((spec, result), binding.identity(), Witness::Any)?;
    let (report, mut messages) = streams(&result.stdout, kind).map_err(refused)?;
    if let Some(request) = binding.request() {
        request
            .capture_files(&mut messages, staging.path())
            .map_err(refused)?;
        let units = super::super::units_of(&messages, driver.dir)?;
        if !request.covers(&units).map_err(refused)? {
            return Err(refused(io::Error::other(
                "doctest compilation read outside its bound graph",
            )));
        }
        request.unchanged(driver, options).map_err(refused)?;
        if let Err(source) = verify(kind, &report, staging.path()) {
            trace.note("capture-unaccounted", &source.to_string());
            return Ok(Product {
                origin: Origin::Actual,
                directory: staging.keep(),
                report,
                observation,
                runtime: runtime::Inputs::NotNative,
            });
        }
        let runtime = match kind {
            Kind::NativeRun => runtime::Inputs::capture(
                staging.path(),
                &inventory(staging.path()).map_err(refused)?,
                driver.toolchain.identities(),
            )
            .map_err(refused)?,
            Kind::Program | Kind::Listing | Kind::Ignored | Kind::Run => runtime::Inputs::NotNative,
        };
        publish(
            (directory, staging),
            (request, kind),
            (result, observation),
            (report, runtime),
        )
        .map_err(refused)
    } else {
        Ok(Product {
            origin: Origin::Actual,
            directory: staging.keep(),
            report,
            observation,
            runtime: runtime::Inputs::NotNative,
        })
    }
}

fn cached(
    (directory, binding): (&Path, &Binding),
    (driver, spec, preparation): (&Driver<'_>, &Spec, &Preparation),
    (kind, trace): (Kind, &Recorder),
) -> Option<Result<Product, CargoError>> {
    let Some(request) = binding.request() else {
        trace.note(
            if kind == Kind::Program {
                "capture-program-miss"
            } else {
                "build-cache-miss"
            },
            binding.identity().detail(),
        );
        return None;
    };
    if let Some(env) = &spec.env {
        match request.failure((env, preparation)) {
            Ok(error) => {
                trace.note("capture-producer-refusal", &request.key);
                return Some(Err(error));
            }
            Err(source) => trace.note("capture-refusal-miss", &source.to_string()),
        }
    }
    match read(
        (directory, request),
        kind,
        spec,
        driver.toolchain.identities(),
    ) {
        Ok(product) => {
            trace.note(
                if kind == Kind::Program {
                    "capture-program-reuse"
                } else {
                    "build-cache-hit"
                },
                &request.key,
            );
            Some(Ok(product))
        }
        Err(source) => {
            trace.note(
                if kind == Kind::Program {
                    "capture-program-miss"
                } else {
                    "build-cache-miss"
                },
                &format!("{}: {source}", request.key),
            );
            None
        }
    }
}

fn failed(
    (request, options): (Option<&Request>, &CompileOptions),
    (driver, spec): (&Driver<'_>, &Spec),
    (error, preparation): (&CargoError, &Preparation),
    (trace, actual, stage): (&Recorder, Option<&RunResult>, FailedStage),
) {
    let Some(request) = request else {
        return;
    };
    let result = request
        .unchanged(driver, options)
        .and_then(|()| request.fail((error, stage), (spec, actual), preparation));
    match result {
        Ok(()) => trace.note("capture-producer-failed", &request.key),
        Err(source) => trace.note("capture-failure-publication-refused", &source.to_string()),
    }
}

pub(super) fn program(driver: &Driver<'_>, directory: &Path) -> Result<PathBuf, CargoError> {
    let trace = trace(driver)?;
    let preparation = Preparation::own(directory, &trace).map_err(refused)?;
    let source = directory.join("capture.rs");
    crate::replace::file(&source, crate::sealed::doctest::CAPTURE_SOURCE.as_bytes())
        .map_err(|failure| refused(failure.source))?;
    let mut options = CompileOptions::new(
        BuildDir::new(directory.to_path_buf(), Vec::new()).rooted(driver.dir.to_path_buf()),
    );
    options.locked = true;
    options.offline = true;
    let program = format!("capture{}", std::env::consts::EXE_SUFFIX);
    let mut spec = Spec::new(
        [
            driver.toolchain.rustc().as_os_str().to_owned(),
            OsString::from("--edition=2021"),
            OsString::from("--crate-name=capture"),
            OsString::from("-o"),
            directory.join(&program).into_os_string(),
            source.as_os_str().to_owned(),
        ],
        crate::runner::Bound::After(crate::runner::PROBE),
    );
    spec.dir = Some(directory.to_path_buf());
    spec.env = driver.toolchain.env().cloned();
    spec.structured_stdout = Some(REPORT_LIMIT);
    let request = binding(
        (driver, &options),
        &mut spec,
        (Kind::Program, &[source]),
        &trace,
    );
    if let Some(result) = cached(
        (directory, &request),
        (driver, &spec, &preparation),
        (Kind::Program, &trace),
    ) {
        return result.map(|product| product.directory.join(program));
    }
    let staging = tempfile::Builder::new()
        .prefix("capture-")
        .tempdir_in(directory)
        .map_err(refused)?;
    *spec
        .argv
        .get_mut(4)
        .ok_or_else(|| refused(io::Error::other("capture compiler output argument")))? =
        staging.path().join(&program).into_os_string();
    let result = completed(
        (driver, &options),
        (&spec, &request),
        (directory, staging, &preparation),
        (Kind::Program, &trace),
    );
    result.map(|product| product.directory.join(program))
}

/// Compiles and captures doctests once for a complete bound graph, retaining the actual report and immutable inventory.
///
/// # Errors
/// A compiler or publication refusal, including a captured input outside the complete graph.
pub fn capture_prepared_doctests(
    driver: &Driver<'_>,
    capture: &DoctestCapture<'_>,
) -> Result<PreparedDoctests, CargoError> {
    prepare(driver, capture, None, Kind::of(capture.baked))
}

pub(super) fn configured_native(
    driver: &Driver<'_>,
    capture: &DoctestCapture<'_>,
    harness_args: &[String],
) -> Result<PreparedDoctests, CargoError> {
    match capture.baked {
        crate::sealed::doctest::Baked::Run => {
            prepare(driver, capture, Some(harness_args), Kind::NativeRun)
        }
        crate::sealed::doctest::Baked::List | crate::sealed::doctest::Baked::ListIgnored => {
            Err(refused(io::Error::other(
                "native harness arguments require a run capture",
            )))
        }
    }
}

fn prepare(
    driver: &Driver<'_>,
    capture: &DoctestCapture<'_>,
    harness_args: Option<&[String]>,
    kind: Kind,
) -> Result<PreparedDoctests, CargoError> {
    let trace = trace(driver)?;
    let preparation =
        Preparation::own(capture.compile.target_dir.path(), &trace).map_err(refused)?;
    let directory = capture.capture.1;
    std::fs::create_dir_all(directory).map_err(refused)?;
    let mut spec = capture_spec(driver, capture, harness_args)?;
    let rustdoc = driver
        .toolchain
        .sysroot()
        .ok_or_else(|| refused(io::Error::other("unobserved rustdoc")))?
        .join("bin")
        .join(format!("rustdoc{}", std::env::consts::EXE_SUFFIX));
    let request = binding(
        (driver, capture.compile),
        &mut spec,
        (kind, &[capture.capture.0.to_path_buf(), rustdoc]),
        &trace,
    );
    if let Some(result) = cached(
        (directory, &request),
        (driver, &spec, &preparation),
        (kind, &trace),
    ) {
        return result.map(PreparedDoctests);
    }
    capture.compile.target_dir.settle()?;
    if request.request().is_some() {
        capture.compile.target_dir.independent()?;
    }
    let staging = tempfile::Builder::new()
        .prefix("doctests-")
        .tempdir_in(directory)
        .map_err(refused)?;
    let staged = DoctestCapture {
        capture: (capture.capture.0, staging.path()),
        ..*capture
    };
    spec.argv = capture_spec(driver, &staged, harness_args)?.argv;
    let result = completed(
        (driver, capture.compile),
        (&spec, &request),
        (directory, staging, &preparation),
        (kind, &trace),
    );
    result.map(PreparedDoctests)
}

fn capture_spec(
    driver: &Driver<'_>,
    capture: &DoctestCapture<'_>,
    harness_args: Option<&[String]>,
) -> Result<Spec, CargoError> {
    let mut args = capture_arguments(capture)?;
    let split = args
        .iter()
        .rposition(|arg| arg == "--")
        .ok_or_else(|| refused(io::Error::other("doctest arguments")))?;
    if let Some(harness_args) = harness_args {
        let tail = split
            .checked_add(1)
            .ok_or_else(|| refused(io::Error::other("doctest argument boundary overflow")))?;
        args.truncate(tail);
        args.extend(harness_args.iter().map(OsString::from));
    }
    args.insert(split, OsString::from("--message-format=json"));
    let mut spec = driver.toolchain.command(driver.dir, args);
    if let Some(env) = &mut spec.env {
        env.overlay(&capture.compile.env);
    }
    spec.structured_stdout = Some(REPORT_LIMIT);
    spec.timeout = capture.compile.timeout;
    Ok(spec)
}

#[derive(Default)]
struct DocInputs {
    external: bool,
}

impl DocInputs {
    fn tokens(&mut self, text: &str) {
        let compact: String = text
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();
        self.external |= ["include!", "include_str!", "include_bytes!"]
            .iter()
            .any(|name| compact.contains(name));
    }
}

impl<'ast> syn::visit::Visit<'ast> for DocInputs {
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        self.tokens(&mac.tokens.to_string());
        self.external |= mac.path.segments.last().is_some_and(|segment| {
            ["include", "include_str", "include_bytes"]
                .iter()
                .any(|name| segment.ident == *name)
        });
        syn::visit::visit_macro(self, mac);
    }

    fn visit_attribute(&mut self, attr: &'ast syn::Attribute) {
        if attr.path().is_ident("doc")
            && let syn::Meta::NameValue(value) = &attr.meta
        {
            match &value.value {
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(literal),
                    ..
                }) => self.tokens(&literal.value()),
                _ => self.external = true,
            }
        }
        syn::visit::visit_attribute(self, attr);
    }
}

fn plain_sources(request: &Request) -> io::Result<()> {
    use syn::visit::Visit as _;
    for path in request.sources() {
        let bytes = std::fs::read(path)?;
        let text = std::str::from_utf8(&bytes).map_err(io::Error::other)?;
        let external = crate::parsing::apart(|parsing| {
            let file = parsing.file(text)?;
            let mut inputs = DocInputs::default();
            inputs.visit_file(&file);
            Ok::<_, crate::parsing::ReadingError>(inputs.external)
        })
        .map_err(io::Error::other)?
        .map_err(io::Error::other)?;
        if external {
            return Err(io::Error::other(
                "doctest source may read an unobserved macro input",
            ));
        }
    }
    Ok(())
}
