// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Native doctest programs with their actual compiler and runtool provenance.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use super::super::build_cache::{File, file, tree};
use super::super::{
    CargoError, CargoErrorKind, CompileOptions, CompilerObservation, Driver, Package, Target,
};
use super::{DoctestCapture, PreparedDoctests, build_capture};
use crate::sealed::doctest::{Baked, Expects, Held, captured};
use crate::sensitive::Sensitive;
use crate::vars::Variables;

const INVOCATION_SCHEMA: &[u8] = b"rust-mutants-native-invocation-v1\0";
const SELF_BINARY: &str = "RUSTDOC_DOCTEST_BIN_PATH";

/// The native program's original rustdoc execution protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeDoctestKind {
    /// One libtest program with its original baked test arguments and routes.
    Merged,
    /// One standalone doctest with the expectation rustdoc assigned it.
    Alone {
        /// The original doctest name.
        name: String,
        /// What a fresh execution passes by.
        expects: Expects,
    },
}

/// What an original compile-only doctest asked the compiler to establish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeCompileExpectation {
    /// The example compiles without being run.
    Compiles,
    /// The example is refused by the compiler without being run.
    Refuses,
}

/// One actual compiler-only attestation, which is never a fresh runtime verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeCompiledDoctest {
    name: String,
    expectation: NativeCompileExpectation,
}

impl NativeCompiledDoctest {
    /// The original rustdoc name, including its compile-only suffix.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The compiler expectation attested by the retained actual report.
    #[must_use]
    pub const fn expectation(&self) -> NativeCompileExpectation {
        self.expectation
    }
}

#[derive(Debug)]
struct Invocation {
    cwd: PathBuf,
    argv: Vec<OsString>,
    environment: Sensitive<Variables>,
}

/// An immutable native program and the observed runtool execution conditions.
#[derive(Debug)]
pub struct NativeDoctestProgram {
    executable: PathBuf,
    kind: NativeDoctestKind,
    invocation: Invocation,
    environment: Sensitive<Variables>,
    arguments: Vec<OsString>,
}

impl NativeDoctestProgram {
    /// The captured native executable, never a Cargo or WASM substitute.
    #[must_use]
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    /// The original merged or standalone execution protocol.
    #[must_use]
    pub const fn kind(&self) -> &NativeDoctestKind {
        &self.kind
    }

    /// The original runtool's actual working directory.
    #[must_use]
    pub fn cwd(&self) -> &Path {
        &self.invocation.cwd
    }

    /// The actual capture invocation, retained separately from native execution arguments.
    #[must_use]
    pub fn observed_argv(&self) -> &[OsString] {
        &self.invocation.argv
    }

    /// The actual capture environment, before immutable executable relocation.
    #[must_use]
    pub const fn observed_environment(&self) -> &Variables {
        self.invocation.environment.expose()
    }

    /// The observed environment with the merged executable bound to its verified self-path.
    #[must_use]
    pub const fn environment(&self) -> &Variables {
        self.environment.expose()
    }

    /// Native arguments outside the original baked harness configuration.
    #[must_use]
    pub const fn arguments(&self) -> &[OsString] {
        self.arguments.as_slice()
    }
}

/// One library's complete native inventory and original actual compiler observation.
#[derive(Debug)]
pub struct NativeDoctestProducts {
    package: String,
    library: Target,
    captured: PreparedDoctests,
    programs: Vec<NativeDoctestProgram>,
    compiler_only: Vec<NativeCompiledDoctest>,
    ignored: Vec<String>,
    files: BTreeMap<PathBuf, File>,
    capture_program: PathBuf,
    capture_state: File,
    harness_args: Vec<String>,
}

impl NativeDoctestProducts {
    /// The original package name used to derive one library target identity.
    #[must_use]
    pub fn package(&self) -> &str {
        &self.package
    }

    /// The original documented library metadata, shared by every captured program.
    #[must_use]
    pub const fn library(&self) -> &Target {
        &self.library
    }

    /// Every native program the same actual producer accounted for.
    #[must_use]
    pub fn programs(&self) -> &[NativeDoctestProgram] {
        &self.programs
    }

    /// Explicit compiler-only evidence, including an empty runtime scope.
    #[must_use]
    pub fn compiler_only(&self) -> &[NativeCompiledDoctest] {
        &self.compiler_only
    }

    /// Standalone doctests rustdoc explicitly ignored in the actual capture report.
    #[must_use]
    pub fn ignored(&self) -> &[String] {
        &self.ignored
    }

    /// The complete original actual producer, retained unchanged on verified reuse.
    #[must_use]
    pub const fn observation(&self) -> &CompilerObservation {
        self.captured.observation()
    }

    /// The original compiler report, with compile-only and ignored scope intact.
    #[must_use]
    pub fn report(&self) -> &[u8] {
        self.captured.report()
    }

    /// The same verified host capture helper available to sealed preparation.
    #[must_use]
    pub fn capture_program(&self) -> &Path {
        &self.capture_program
    }

    /// The exact requested harness arguments bound into this compiler producer.
    #[must_use]
    pub fn harness_args(&self) -> &[String] {
        &self.harness_args
    }

    /// Verifies captured bytes and original runtime library inputs before and after execution.
    ///
    /// # Errors
    /// Refuses any changed, missing, added or aliased product, helper or runtime input.
    pub fn verify(&self) -> Result<(), CargoError> {
        self.captured.verify_native_runtime_inputs()?;
        if inventory(self.captured.directory()).map_err(refused)? != self.files
            || file(&self.capture_program).map_err(refused)? != self.capture_state
        {
            return Err(refused(io::Error::other("native doctest products changed")));
        }
        Ok(())
    }
}

fn refused(source: io::Error) -> CargoError {
    CargoError::new(
        CargoErrorKind::BuildLedger,
        "cannot establish verified native doctest products",
    )
    .with_source(source)
}

fn inventory(directory: &Path) -> io::Result<BTreeMap<PathBuf, File>> {
    let mut files = BTreeMap::new();
    tree(directory, Path::new(""), &mut files)?;
    Ok(files)
}

/// Prepares a native library once, retaining only complete actual compiler products.
///
/// # Errors
/// Refuses unsupported targets, incomplete identities, missing invocation custody or unaccounted products.
pub fn prepare_native_doctests(
    driver: &Driver<'_>,
    package: &Package,
    compile: &CompileOptions,
    harness_args: &[String],
) -> Result<NativeDoctestProducts, CargoError> {
    let mut libraries = package
        .targets
        .iter()
        .filter(|target| target.is_lib() && !target.is_proc_macro() && target.doctest);
    let library = libraries
        .next()
        .ok_or_else(|| refused(io::Error::other("no documented native library")))?
        .clone();
    if libraries.next().is_some() {
        return Err(refused(io::Error::other(
            "ambiguous documented native library",
        )));
    }
    if compile
        .build
        .target
        .as_ref()
        .is_some_and(|target| target != driver.toolchain.host())
    {
        return Err(refused(io::Error::other(
            "native doctests require the selected executable host toolchain",
        )));
    }
    let capture_program =
        build_capture(driver, &compile.target_dir.path().join("doctests/capture"))?;
    let capture_state = file(&capture_program).map_err(refused)?;
    let mut options = compile.clone();
    options.target_dir = compile.target_dir.nested("doctests/native");
    options.build.target = Some(driver.toolchain.host().to_owned());
    let directory = options.target_dir.path().join("captured");
    let prepared = super::prepared::configured_native(
        driver,
        &DoctestCapture {
            package: &package.name,
            capture: (&capture_program, &directory),
            compile: &options,
            baked: Baked::Run,
        },
        harness_args,
    )?;
    products(
        (package, library),
        prepared,
        (capture_program, capture_state),
        (driver.toolchain.host(), harness_args),
    )
}

fn products(
    (package, library): (&Package, Target),
    prepared: PreparedDoctests,
    (capture_program, capture_state): (PathBuf, File),
    (host, harness_args): (&str, &[String]),
) -> Result<NativeDoctestProducts, CargoError> {
    if prepared.observation().identity().complete_key().is_none() {
        return Err(refused(io::Error::other(
            "native doctest input identity is incomplete",
        )));
    }
    let files = inventory(prepared.directory()).map_err(refused)?;
    let held = Held::read(prepared.directory()).map_err(refused)?;
    let captured = captured(prepared.report(), &held)
        .map_err(|source| refused(io::Error::other(source.to_string())))?;
    let accounted = captured
        .merged
        .len()
        .checked_add(captured.alone.len())
        .ok_or_else(|| refused(io::Error::other("native program count overflow")))?;
    if !captured.unbuilt.is_empty() || accounted != held.binaries.len() {
        return Err(refused(io::Error::other(
            "unaccounted native doctest compilation",
        )));
    }
    let mut programs = Vec::new();
    for binary in captured.merged {
        programs.push(
            program(binary, NativeDoctestKind::Merged, &capture_program, host).map_err(refused)?,
        );
    }
    for alone in captured.alone {
        programs.push(
            program(
                alone.binary,
                NativeDoctestKind::Alone {
                    name: alone.name,
                    expects: alone.expects,
                },
                &capture_program,
                host,
            )
            .map_err(refused)?,
        );
    }
    accounted_files(prepared.directory(), &files, &programs).map_err(refused)?;
    let lines = crate::execute::parse_lines(prepared.report())
        .map_err(|source| refused(io::Error::other(source)))?;
    if lines
        .failed
        .iter()
        .any(|name| compiler_expectation(name).is_some())
    {
        return Err(refused(io::Error::other(
            "a compile-only native doctest was refused unexpectedly",
        )));
    }
    let compiler_only = lines
        .passed
        .into_iter()
        .filter_map(|name| {
            compiler_expectation(&name)
                .map(|expectation| NativeCompiledDoctest { name, expectation })
        })
        .collect();
    let products = NativeDoctestProducts {
        package: package.name.clone(),
        library,
        captured: prepared,
        programs,
        compiler_only,
        ignored: lines.ignored,
        files,
        capture_program,
        capture_state,
        harness_args: harness_args.to_vec(),
    };
    products.verify()?;
    Ok(products)
}

fn compiler_expectation(name: &str) -> Option<NativeCompileExpectation> {
    if name.ends_with(" - compile fail") {
        Some(NativeCompileExpectation::Refuses)
    } else if name.ends_with(" - compile") {
        Some(NativeCompileExpectation::Compiles)
    } else {
        None
    }
}

fn program(
    binary: PathBuf,
    kind: NativeDoctestKind,
    helper: &Path,
    host: &str,
) -> io::Result<NativeDoctestProgram> {
    native_format(host, &std::fs::read(&binary)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if std::fs::metadata(&binary)?.permissions().mode() & 0o111 == 0 {
            return Err(io::Error::other(
                "captured native doctest is not executable",
            ));
        }
    }
    let invocation = invocation(&std::fs::read(binary.with_extension("invocation"))?)?;
    let [original_helper, directory, original_binary] = invocation.argv.as_slice() else {
        return Err(io::Error::other("unknown native runtool routing protocol"));
    };
    if Path::new(original_helper) != helper
        || !Path::new(directory).is_absolute()
        || !Path::new(original_binary).is_absolute()
        || !invocation.cwd.is_absolute()
    {
        return Err(io::Error::other("unknown native runtool routing protocol"));
    }
    let environment = execution_environment(&kind, &invocation, &binary)?;
    Ok(NativeDoctestProgram {
        executable: binary,
        kind,
        invocation,
        environment: Sensitive::new(environment),
        arguments: Vec::new(),
    })
}

fn execution_environment(
    kind: &NativeDoctestKind,
    invocation: &Invocation,
    binary: &Path,
) -> io::Result<Variables> {
    if invocation
        .environment
        .expose()
        .holds(crate::sealed::doctest::RUN_ONE)
    {
        return Err(io::Error::other(
            "inherited native merged route is not a complete library execution",
        ));
    }
    let [_, _, original_binary] = invocation.argv.as_slice() else {
        return Err(io::Error::other("unknown native runtool routing protocol"));
    };
    let mut environment = invocation.environment.expose().clone();
    match kind {
        NativeDoctestKind::Merged => {
            if environment
                .var(SELF_BINARY)
                .is_some_and(|path| path != original_binary)
            {
                return Err(io::Error::other(
                    "native merged self-path differs from its actual captured binary",
                ));
            }
            environment.set(SELF_BINARY, binary);
        }
        NativeDoctestKind::Alone {
            name: _,
            expects: _,
        } => {}
    }
    Ok(environment)
}

fn accounted_files(
    directory: &Path,
    files: &BTreeMap<PathBuf, File>,
    programs: &[NativeDoctestProgram],
) -> io::Result<()> {
    let mut expected = std::collections::BTreeSet::new();
    for program in programs {
        for extension in ["wasm", "claim", "invocation"] {
            expected.insert(program.executable.with_extension(extension));
        }
    }
    if files
        .keys()
        .any(|path| !path.starts_with(directory.join("compiler")) && !expected.contains(path))
    {
        return Err(io::Error::other("unknown native doctest inventory member"));
    }
    Ok(())
}

fn native_format(host: &str, bytes: &[u8]) -> io::Result<()> {
    let arm = match host.split('-').next() {
        Some("aarch64") => true,
        Some("x86_64") => false,
        Some(_) | None => return Err(io::Error::other("unsupported native doctest architecture")),
    };
    let accepted = if host.contains("-apple-darwin") {
        bytes.get(..4) == Some(b"\xcf\xfa\xed\xfe")
            && bytes.len() >= 32
            && bytes.get(4..8) == Some(if arm { &[12, 0, 0, 1] } else { &[7, 0, 0, 1] })
            && bytes.get(12..16) == Some(&[2, 0, 0, 0])
    } else if host.contains("-linux-") {
        bytes.get(..4) == Some(b"\x7fELF")
            && bytes.len() >= 64
            && bytes.get(4..7) == Some(&[2, 1, 1])
            && matches!(bytes.get(16..18), Some([2 | 3, 0]))
            && bytes.get(18..20) == Some(if arm { &[183, 0] } else { &[62, 0] })
    } else if host.contains("-windows-") {
        pe(bytes, arm)?
    } else {
        false
    };
    if accepted {
        Ok(())
    } else {
        Err(io::Error::other(
            "captured doctest is not a supported native executable",
        ))
    }
}

fn pe(bytes: &[u8], arm: bool) -> io::Result<bool> {
    if bytes.get(..2) != Some(b"MZ") {
        return Ok(false);
    }
    let offset: [u8; 4] = bytes
        .get(60..64)
        .ok_or_else(|| io::Error::other("incomplete native PE header"))?
        .try_into()
        .map_err(io::Error::other)?;
    let offset = usize::try_from(u32::from_le_bytes(offset)).map_err(io::Error::other)?;
    let end = offset
        .checked_add(26)
        .ok_or_else(|| io::Error::other("native PE header overflow"))?;
    let Some(header) = bytes.get(offset..end) else {
        return Ok(false);
    };
    Ok(header.get(..4) == Some(b"PE\0\0")
        && header.get(4..6) == Some(if arm { &[0x64, 0xaa] } else { &[0x64, 0x86] })
        && header.get(22).is_some_and(|flags| flags & 2 != 0)
        && header.get(24..26) == Some(&[0x0b, 2]))
}

struct Frames<'a>(&'a [u8]);

impl<'a> Frames<'a> {
    fn take(&mut self, size: usize) -> io::Result<&'a [u8]> {
        let (held, rest) = self
            .0
            .split_at_checked(size)
            .ok_or_else(|| io::Error::other("incomplete native invocation"))?;
        self.0 = rest;
        Ok(held)
    }

    fn number(&mut self) -> io::Result<usize> {
        let bytes: [u8; 8] = self.take(8)?.try_into().map_err(io::Error::other)?;
        usize::try_from(u64::from_le_bytes(bytes)).map_err(io::Error::other)
    }

    fn string(&mut self) -> io::Result<OsString> {
        let size = self.number()?;
        os_string(self.take(size)?)
    }

    fn count(&mut self, framing: usize) -> io::Result<usize> {
        let count = self.number()?;
        let remaining = self
            .0
            .len()
            .checked_div(framing)
            .ok_or_else(|| io::Error::other("invalid native invocation framing"))?;
        if count > remaining {
            return Err(io::Error::other(
                "native invocation count exceeds its actual records",
            ));
        }
        Ok(count)
    }
}

fn invocation(bytes: &[u8]) -> io::Result<Invocation> {
    let mut frames = Frames(bytes);
    if frames.take(INVOCATION_SCHEMA.len())? != INVOCATION_SCHEMA {
        return Err(io::Error::other("unknown native invocation schema"));
    }
    let platform = frames.take(1)?;
    if platform != [u8::from(cfg!(windows))] {
        return Err(io::Error::other(
            "native invocation belongs to another host platform",
        ));
    }
    let cwd = PathBuf::from(frames.string()?);
    let mut argv = Vec::new();
    for _ in 0..frames.count(8)? {
        argv.push(frames.string()?);
    }
    let mut environment = Variables::empty();
    for _ in 0..frames.count(16)? {
        let name = frames.string()?;
        let value = frames.string()?;
        if name.is_empty() || environment.var_os(&name).is_some() {
            return Err(io::Error::other(
                "native invocation has ambiguous environment names",
            ));
        }
        environment.set(name, value);
    }
    if !frames.0.is_empty() {
        return Err(io::Error::other("unaccounted native invocation bytes"));
    }
    Ok(Invocation {
        cwd,
        argv,
        environment: Sensitive::new(environment),
    })
}

pub(super) fn invocation_runtime_inputs(bytes: &[u8]) -> io::Result<(Variables, PathBuf)> {
    invocation(bytes).map(|invocation| (invocation.environment.expose().clone(), invocation.cwd))
}

#[cfg(unix)]
fn os_string(bytes: &[u8]) -> io::Result<OsString> {
    use std::os::unix::ffi::OsStringExt as _;
    if bytes.contains(&0) {
        return Err(io::Error::other("native OS string contains a nul byte"));
    }
    Ok(OsString::from_vec(bytes.to_vec()))
}

#[cfg(windows)]
fn os_string(bytes: &[u8]) -> io::Result<OsString> {
    use std::os::windows::ffi::OsStringExt as _;
    let (units, remainder) = bytes.as_chunks::<2>();
    let wide: Vec<u16> = units.iter().copied().map(u16::from_le_bytes).collect();
    if !remainder.is_empty() || wide.contains(&0) {
        return Err(io::Error::other("incomplete native Windows OS string"));
    }
    Ok(OsString::from_wide(&wide))
}

#[cfg(test)]
mod tests;
