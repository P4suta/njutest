// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the sealed target's standard library answers for the temporary and the home directory, which its platform layer does not have: an object every sealed module is linked with, whose functions read `TMPDIR` and `HOME`, and the rewrite that makes the standard library's own functions answer through them (ADR 0046).

use std::ffi::OsString;
use std::num::NonZeroU64;
use std::path::Path;

use rust_mutants_sealed::{
    Arguments, ClockPolicy, CompilationCache, Environment, Interrupt, Invocation, Limits, Preopen,
    Preopens, Redirect, SealedRunner, SealedStop, names_std_env, redirected,
};

use crate::cargo::{CargoError, CargoErrorKind, Driver};
use crate::runner::{Bound, PROBE, Spec, run};
use crate::trace::ExecRecord;

/// The export that answers `std::env::temp_dir` in a sealed module.
pub const TEMP_DIR: &str = "rust_mutants_sealed_temp_dir";

/// The export that answers `std::env::home_dir` in a sealed module.
pub const HOME_DIR: &str = "rust_mutants_sealed_home_dir";

/// The source of the object every sealed module is linked with: what the standard library answers for the two directories on a POSIX system, read from the environment the instance is given.
pub const SOURCE: &str = r#"//! What the sealed target's standard library answers for the temporary and the home directory.

use std::path::PathBuf;

#[unsafe(export_name = "rust_mutants_sealed_temp_dir")]
pub fn temp_dir() -> PathBuf {
    match std::env::var_os("TMPDIR") {
        Some(directory) => PathBuf::from(directory),
        None => PathBuf::from("/tmp"),
    }
}

#[unsafe(export_name = "rust_mutants_sealed_home_dir")]
pub fn home_dir() -> Option<PathBuf> {
    match std::env::var_os("HOME") {
        Some(home) if !home.is_empty() => Some(PathBuf::from(home)),
        Some(_) | None => None,
    }
}
"#;

/// The source of the program a toolchain is probed with: it prints the two directories as its standard library answers them.
pub const PROBE_SOURCE: &str = r#"fn main() {
    println!("{}", std::env::temp_dir().display());
    println!("{:?}", std::env::home_dir());
}
"#;

/// What the probe prints where the standard library answers from the environment it is given.
const PROBE_ANSWER: &str = "/probe/tmp\nSome(\"/probe/home\")\n";

/// The object a platform directory holds.
const OBJECT: &str = "platform.o";

/// The record a platform directory keeps the whole identity of its answered object in.
const ANSWERED: &str = "answered";

/// Every function of the standard library a sealed module is rewritten to answer through the object's.
pub const REDIRECTS: [Redirect; 2] = [
    Redirect {
        names: |name| names_std_env(name, "temp_dir"),
        export: TEMP_DIR,
    },
    Redirect {
        names: |name| names_std_env(name, "home_dir"),
        export: HOME_DIR,
    },
];

/// The object a toolchain's sealed modules are linked with, or why its standard library could not be made to answer from the environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Readied {
    /// The object's path, which is text, as a flag carries it, and whose probe answered.
    Object(String),
    /// Why it could not be made or did not answer, in words.
    Unanswered(String),
}

/// Every flag a sealed module is built with besides the tree's own: no optimisation and every name kept, the object and its exports, and the host's start.
#[must_use]
pub fn flags(object: &str) -> Vec<String> {
    let mut flags = vec!["-Copt-level=0".to_owned(), "-Cstrip=none".to_owned()];
    flags.extend(super::start_linked());
    for export in [TEMP_DIR, HOME_DIR] {
        flags.push(format!("-Clink-arg=--export={export}"));
    }
    flags.push(format!("-Clink-arg={object}"));
    flags
}

/// The object for the toolchain `driver` runs, built and probed in a directory of `root` its identity names, unless that directory keeps one answered for the whole identity.
///
/// # Errors
/// A `rustc` that could not be started, or a run that was cancelled.
pub fn ready(
    modules: &rust_mutants_sealed::ModuleOwner,
    driver: &Driver<'_>,
    root: &Path,
) -> Result<Readied, CargoError> {
    let trace = match driver.toolchain.env() {
        Some(vars) => driver
            .trace
            .costed(vars, driver.dir)
            .map_err(|source| CargoError::new(CargoErrorKind::CommandFailed, source.to_string()))?,
        None => driver.trace.clone(),
    };
    trace.note("sealed-platform-request", "1");
    let failed = |path: &Path, error: std::io::Error| {
        CargoError::new(
            CargoErrorKind::CommandFailed,
            format!("{}: {error}", path.display()),
        )
    };
    let identity = identity(driver.toolchain);
    let directory = root.join(crate::keyed::name(&identity).map_err(|error| failed(root, error))?);
    let object = directory.join(OBJECT);
    let Some(text) = object.to_str().map(str::to_owned) else {
        return Ok(Readied::Unanswered(format!(
            "{} is not text, which a flag cannot carry",
            object.display()
        )));
    };
    let answered = directory.join(ANSWERED);
    if answers(&directory, &identity).map_err(|error| failed(&directory, error))? {
        return Ok(Readied::Object(text));
    }
    std::fs::create_dir_all(&directory).map_err(|error| failed(&directory, error))?;
    match std::fs::remove_file(&answered) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(failed(&answered, error)),
    }
    let source = directory.join("platform.rs");
    std::fs::write(&source, SOURCE).map_err(|error| failed(&source, error))?;
    let built = compiled(
        driver,
        &directory,
        [
            "--crate-type=lib",
            "--crate-name=rust_mutants_sealed_platform",
            "--emit=obj",
            "-Copt-level=0",
            "-Cdebuginfo=0",
        ]
        .map(OsString::from)
        .to_vec(),
        (&source, &object),
    )?;
    if let Some(said) = built {
        return Ok(Readied::Unanswered(format!(
            "the object did not build: {said}"
        )));
    }
    let probe = directory.join("probe.rs");
    std::fs::write(&probe, PROBE_SOURCE).map_err(|error| failed(&probe, error))?;
    let module = directory.join("probe.wasm");
    let mut arguments = vec![OsString::from("--crate-name=probe")];
    arguments.extend(flags(&text).iter().map(OsString::from));
    if let Some(said) = compiled(driver, &directory, arguments, (&probe, &module))? {
        return Ok(Readied::Unanswered(format!(
            "the probe did not build: {said}"
        )));
    }
    let bytes = std::fs::read(&module).map_err(|error| failed(&module, error))?;
    let cache = retained_cache(driver.toolchain.env())?;
    if let Some(said) = unanswered(modules, &bytes, &trace, Some(&cache)) {
        return Ok(Readied::Unanswered(said));
    }
    std::fs::write(&answered, &identity).map_err(|error| failed(&answered, error))?;
    Ok(Readied::Object(text))
}

/// Selects the retained module-cache capability without treating a platform-build directory as its owner.
fn retained_cache(vars: Option<&crate::vars::Variables>) -> Result<CompilationCache, CargoError> {
    let supplied = vars.and_then(|vars| {
        vars.var("XDG_CACHE_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| vars.var("LOCALAPPDATA").map(std::path::PathBuf::from))
            .or_else(|| {
                vars.var("HOME")
                    .map(|home| std::path::PathBuf::from(home).join(".cache"))
            })
    });
    let selected = match &supplied {
        Some(root) => super::module_cache(root, vars),
        None => super::module_cache(Path::new(""), vars),
    };
    selected.map_err(|source| {
        CargoError::new(
            CargoErrorKind::CommandFailed,
            "the sealed platform requires a retained module-cache owner",
        )
        .with_source(std::io::Error::other(source))
    })
}

/// The identity of the platform object a toolchain's compiler makes from the sources.
fn identity(toolchain: &crate::cargo::Toolchain) -> String {
    crate::id::digest(
        [
            toolchain.rustc_version().summary.as_str(),
            SOURCE,
            PROBE_SOURCE,
            &super::start_linked().join(" "),
        ]
        .join("\0")
        .as_bytes(),
    )
}

/// Whether `directory` holds the object `identity` names, its probe answered, and its record keeps that whole identity.
fn answers(directory: &Path, identity: &str) -> std::io::Result<bool> {
    let held = |name: &str| match std::fs::metadata(directory.join(name)) {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    };
    if !held(OBJECT)? {
        return Ok(false);
    }
    match std::fs::read(directory.join(ANSWERED)) {
        Ok(kept) => Ok(kept == identity.as_bytes()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// What `rustc` said where it did not compile `source` into `output` for the sealed target with `arguments`, or nothing where it did.
///
/// # Errors
/// A `rustc` that could not be started, or a run that was cancelled.
fn compiled(
    driver: &Driver<'_>,
    directory: &Path,
    arguments: Vec<OsString>,
    (source, output): (&Path, &Path),
) -> Result<Option<String>, CargoError> {
    let mut argv = vec![
        driver.toolchain.rustc().as_os_str().to_owned(),
        OsString::from("--edition=2024"),
        OsString::from("--target"),
        OsString::from(super::TARGET),
    ];
    argv.extend(arguments);
    argv.push(OsString::from("-o"));
    argv.push(output.as_os_str().to_owned());
    argv.push(source.as_os_str().to_owned());
    let mut spec = Spec::new(argv, Bound::After(PROBE));
    spec.dir = Some(directory.to_path_buf());
    spec.env = driver.toolchain.env().cloned();
    let result = run(&spec, driver.cancel);
    driver.trace.exec_result(ExecRecord::of(&spec, &result));
    if driver.cancel.is_cancelled() {
        return Err(CargoError::new(
            CargoErrorKind::Cancelled,
            "the sealed target's platform object was cancelled",
        ));
    }
    if result.succeeded() {
        return Ok(None);
    }
    Ok(Some(
        crate::telling::LosslessBytes::new(&result.output).to_string(),
    ))
}

/// Why the probe module `bytes` does not answer the two directories from the environment, or nothing where it does.
fn unanswered(
    modules: &rust_mutants_sealed::ModuleOwner,
    bytes: &[u8],
    trace: &crate::trace::Recorder,
    cache: Option<&CompilationCache>,
) -> Option<String> {
    let rewritten = match redirected(bytes, &REDIRECTS) {
        Ok(rewritten) if rewritten.counts.iter().all(|count| *count > 0) => rewritten.bytes,
        Ok(rewritten) => {
            return Some(format!(
                "the probe holds no function of the standard library's to answer the temporary \
                 and the home directory through, as the rewrite counted them: {:?}",
                rewritten.counts
            ));
        }
        Err(error) => return Some(format!("the probe could not be rewritten: {error}")),
    };
    let runner = match SealedRunner::with_compiler(
        modules,
        super::bench::WATCHDOG,
        rust_mutants_sealed::CompilerTier::faithful(),
        cache,
    ) {
        Ok(runner) => runner,
        Err(error) => return Some(format!("the host could not start: {error}")),
    };
    let module = match runner.prepare(&rewritten) {
        Ok(module) => module,
        Err(error) => return Some(format!("the probe is not a module the host runs: {error}")),
    };
    let invocation = match probing() {
        Ok(invocation) => invocation,
        Err(error) => return Some(format!("the probe's invocation: {error}")),
    };
    let answer = module.invoke(&invocation, &Interrupt::of(Vec::new()));
    if let Some(spent) = runner.spent() {
        match serde_json::to_string(&spent) {
            Ok(detail) => trace.note("sealed-platform-work", &detail),
            Err(source) => trace.note("sealed-platform-work-invalid", &source.to_string()),
        }
    }
    match answer {
        Ok(transcript)
            if transcript.stop() == SealedStop::Returned
                && transcript.stdout().bytes() == PROBE_ANSWER.as_bytes() =>
        {
            None
        }
        Ok(transcript) => Some(format!(
            "the probe ended {:?} having printed {}",
            transcript.stop(),
            crate::telling::LosslessBytes::new(transcript.stdout().bytes())
        )),
        Err(error) => Some(format!("the probe did not run: {error}")),
    }
}

/// The invocation the probe runs with: the two directories named, and nothing else to reach.
fn probing() -> Result<Invocation, rust_mutants_sealed::SealedError> {
    Ok(Invocation {
        arguments: Arguments::new(vec!["probe".to_owned()])?,
        environment: Environment::new(vec![
            ("TMPDIR".to_owned(), "/probe/tmp".to_owned()),
            ("HOME".to_owned(), "/probe/home".to_owned()),
        ])?,
        preopens: Preopens::new(vec![Preopen::Root { start: None }])?,
        seed: 0,
        fuel: super::bench::CONTROL_FUEL,
        limits: Limits {
            memory: super::bench::MEMORY,
            stdout: 1 << 16,
            stderr: 1 << 16,
            overlay: 0,
        },
        clock: ClockPolicy {
            realtime_origin: 0,
            monotonic_origin: 0,
            nanos_per_fuel: NonZeroU64::MIN,
        },
        halt: None,
    })
}

#[cfg(test)]
mod tests;
