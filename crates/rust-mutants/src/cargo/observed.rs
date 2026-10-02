// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Owned observations bound to executable content, the complete environment and source/configuration inputs.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::build_cache::{File, configurations, file, tree};
use super::{CargoError, CargoErrorKind, LocateOptions, Toolchain, VersionInfo};
use crate::runner::{Cancel, RunResult, Spec, Watch};
use crate::trace::ExecRecord;
use crate::vars::Variables;

const SCHEMA: &str = "rust-mutants-tool-observation-v1";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Located {
    pub(super) cargo: PathBuf,
    pub(super) chosen: PathBuf,
    pub(super) rustc: PathBuf,
    pub(super) sysroot: PathBuf,
    pub(super) cargo_version: VersionInfo,
    pub(super) rustc_version: VersionInfo,
}

impl Located {
    fn of(toolchain: &Toolchain) -> io::Result<Self> {
        Ok(Self {
            cargo: toolchain.cargo().to_path_buf(),
            chosen: toolchain.selecting().path().to_path_buf(),
            rustc: toolchain.rustc().to_path_buf(),
            sysroot: toolchain
                .sysroot()
                .ok_or_else(|| io::Error::other("an unobserved compiler sysroot"))?
                .to_path_buf(),
            cargo_version: toolchain.cargo_version().clone(),
            rustc_version: toolchain.rustc_version().clone(),
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Process {
    exec: ExecRecord,
    stdout: Vec<u8>,
    output: Vec<u8>,
    leader: Option<u32>,
    stdout_digest: String,
    output_digest: String,
}

impl Process {
    fn actual(spec: &Spec, result: &RunResult) -> io::Result<Self> {
        Ok(Self {
            exec: ExecRecord::of(spec, result).map_err(io::Error::other)?,
            stdout: result.stdout.clone(),
            output: result.output.clone(),
            leader: result.leader,
            stdout_digest: crate::id::digest(&result.stdout),
            output_digest: crate::id::digest(&result.output),
        })
    }

    fn verified(&self) -> bool {
        self.leader.is_some()
            && self.stdout_digest == crate::id::digest(&self.stdout)
            && self.output_digest == crate::id::digest(&self.output)
            && matches!(
                self.exec.stopped,
                crate::execute::Stopped::Exited {
                    exit: crate::runner::ProcessExit::Code(0)
                }
            )
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: String,
    key: String,
    located: Located,
    inputs: BTreeMap<PathBuf, File>,
    processes: Vec<Process>,
}

struct Collected<'a, W> {
    outer: &'a W,
    processes: RefCell<Vec<Process>>,
    unavailable: RefCell<Option<String>>,
}

impl<W: Watch> Watch for Collected<'_, W> {
    fn cancel(&self) -> &Cancel {
        self.outer.cancel()
    }

    fn exec(&self, spec: &Spec, result: &RunResult) {
        self.outer.exec(spec, result);
        match Process::actual(spec, result) {
            Ok(process) => self.processes.borrow_mut().push(process),
            Err(source) => *self.unavailable.borrow_mut() = Some(source.to_string()),
        }
    }

    fn note(&self, kind: &str, detail: &str) {
        self.outer.note(kind, detail);
    }
}

struct Owner {
    key: String,
    record: PathBuf,
    lease: std::fs::File,
    initial: BTreeMap<PathBuf, File>,
}

impl Owner {
    fn acquire<W: Watch>(
        (options, dir): (&LocateOptions, &Path),
        (cargo, rustc): (&Path, &Path),
        watch: &W,
    ) -> io::Result<Self> {
        let env = options
            .env
            .as_ref()
            .ok_or_else(|| io::Error::other("inherited observation environment"))?;
        let parent = retained(env)?;
        let mut digest = Sha256::new();
        for bytes in [
            SCHEMA.as_bytes(),
            dir.as_os_str().as_encoded_bytes(),
            cargo.as_os_str().as_encoded_bytes(),
            rustc.as_os_str().as_encoded_bytes(),
        ] {
            field(&mut digest, bytes)?;
        }
        for (name, value) in env.canonical() {
            field(&mut digest, name.as_encoded_bytes())?;
            field(&mut digest, value.as_encoded_bytes())?;
        }
        let sysroot = selecting_root((cargo, rustc), env)?;
        let initial = observation_inputs((cargo, rustc, cargo), (&sysroot, dir, env))?;
        field(
            &mut digest,
            &serde_json::to_vec(&initial).map_err(io::Error::other)?,
        )?;
        let key = hex::encode(digest.finalize());
        let directory = parent.join(&key);
        std::fs::create_dir_all(&directory)?;
        let lease = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("observation.lock"))?;
        let started = std::time::Instant::now();
        let locked = lease.lock();
        watch.note("host-wait", &serde_json::json!({
            "owner": directory.display().to_string(), "cause": "bound tool observation publication",
            "elapsed_ns": u64::try_from(started.elapsed().as_nanos()).map_err(io::Error::other)?,
            "machine": {"os": std::env::consts::OS, "cpus": std::thread::available_parallelism()?.get()}
        }).to_string());
        locked?;
        Ok(Self {
            key,
            record: directory.join("located.json"),
            lease,
            initial,
        })
    }

    fn read(&self, (options, dir): (&LocateOptions, &Path)) -> io::Result<Toolchain> {
        let record: Record = crate::strictjson::decode_slice(&std::fs::read(&self.record)?)
            .map_err(io::Error::other)?;
        if record.schema != SCHEMA || record.key != self.key || record.processes.is_empty() {
            return Err(io::Error::other("an unbound tool observation record"));
        }
        let observed =
            Toolchain::observed(record.located, options.env.clone()).map_err(io::Error::other)?;
        if observed_inputs(&observed, dir)? != record.inputs || self.initial != record.inputs {
            return Err(io::Error::other(
                "the executable, environment or graph observation inputs changed",
            ));
        }
        validate_processes(&observed, &record.processes)?;
        Ok(observed)
    }

    fn publish(
        &self,
        (toolchain, dir): (&Toolchain, &Path),
        processes: Vec<Process>,
    ) -> io::Result<()> {
        let inputs = observed_inputs(toolchain, dir)?;
        if inputs != self.initial {
            return Err(io::Error::other(
                "complete tool observation inputs changed while the actual processes ran",
            ));
        }
        validate_processes(toolchain, &processes)?;
        let record = Record {
            schema: SCHEMA.to_owned(),
            key: self.key.clone(),
            located: Located::of(toolchain)?,
            inputs,
            processes,
        };
        crate::replace::file(
            &self.record,
            &serde_json::to_vec(&record).map_err(io::Error::other)?,
        )
        .map_err(|error| error.source)
    }
}

/// Reuses only a complete, unchanged observation whose original actual processes remain retained.
pub(super) fn locate<W: Watch>(
    (options, dir): (&LocateOptions, &Path),
    (cargo, rustc): (&Path, &Path),
    watch: &W,
    fresh: impl FnOnce(&dyn Watch) -> Result<Toolchain, CargoError>,
) -> Result<Toolchain, CargoError> {
    if watch.cancel().is_cancelled() {
        return Err(CargoError::new(
            CargoErrorKind::Cancelled,
            "toolchain observation was cancelled",
        ));
    }
    let owner = match Owner::acquire((options, dir), (cargo, rustc), watch) {
        Ok(owner) => Some(owner),
        Err(source) => {
            watch.note("toolchain-observation-unbound", &source.to_string());
            None
        }
    };
    if watch.cancel().is_cancelled() {
        return Err(CargoError::new(
            CargoErrorKind::Cancelled,
            "toolchain observation was cancelled while awaiting its owner",
        ));
    }
    if let Some(owner) = &owner {
        match owner.read((options, dir)) {
            Ok(toolchain) => {
                watch.note("toolchain-observation-reuse", &owner.key);
                return Ok(toolchain);
            }
            Err(source) => watch.note("toolchain-observation-miss", &source.to_string()),
        }
    }
    let collected = Collected {
        outer: watch,
        processes: RefCell::new(Vec::new()),
        unavailable: RefCell::new(None),
    };
    let toolchain = fresh(&collected)?;
    if let Some(owner) = &owner {
        let publication = match collected.unavailable.into_inner() {
            Some(source) => Err(io::Error::other(source)),
            None => owner.publish((&toolchain, dir), collected.processes.into_inner()),
        };
        match publication {
            Ok(()) => watch.note("toolchain-observation-bound", &owner.key),
            Err(source) => watch.note("toolchain-observation-unbound", &source.to_string()),
        }
    }
    if let Some(owner) = owner {
        drop(owner.lease);
    }
    Ok(toolchain)
}

fn retained(env: &Variables) -> io::Result<PathBuf> {
    let root = match env
        .var("NJUTEST_FIXTURE_BUILD_CACHE")
        .or_else(|| env.var("XDG_CACHE_HOME"))
    {
        Some(root) => PathBuf::from(root),
        None => super::config::home(env)
            .ok_or_else(|| io::Error::other("no explicit retained observation root"))?,
    };
    if !root.is_absolute() {
        return Err(io::Error::other(
            "an observation cache root is not absolute",
        ));
    }
    Ok(root.join("rust-mutants-tool-observations-v1"))
}

fn observed_inputs(toolchain: &Toolchain, dir: &Path) -> io::Result<BTreeMap<PathBuf, File>> {
    let env = toolchain
        .env()
        .ok_or_else(|| io::Error::other("an inherited compiler environment"))?;
    let sysroot = toolchain
        .sysroot()
        .ok_or_else(|| io::Error::other("an unobserved compiler sysroot"))?;
    observation_inputs(
        (
            toolchain.cargo(),
            toolchain.rustc(),
            toolchain.selecting().path(),
        ),
        (sysroot, dir, env),
    )
}

fn observation_inputs(
    (cargo, rustc, selecting): (&Path, &Path, &Path),
    (sysroot, dir, env): (&Path, &Path, &Variables),
) -> io::Result<BTreeMap<PathBuf, File>> {
    let mut inputs = BTreeMap::new();
    for program in <[&Path; 3]>::from((cargo, rustc, selecting)) {
        known_program(program, sysroot, env)?;
        inputs.insert(program.to_path_buf(), file(program)?);
    }
    tree(dir, &dir.join("target"), &mut inputs)?;
    configurations(dir, env, &mut inputs)?;
    for ancestor in dir.ancestors() {
        for name in ["rust-toolchain", "rust-toolchain.toml"] {
            present(&ancestor.join(name), &mut inputs)?;
        }
    }
    let rustup = match env.var("RUSTUP_HOME") {
        Some(root) => PathBuf::from(root),
        None => PathBuf::from(
            env.var("HOME")
                .ok_or_else(|| io::Error::other("unknown toolchain selection home"))?,
        )
        .join(".rustup"),
    };
    present(&rustup.join("settings.toml"), &mut inputs)?;
    for entry in std::fs::read_dir(sysroot.join("lib"))? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            inputs.insert(entry.path(), file(&entry.path())?);
        }
    }
    #[cfg(windows)]
    for entry in std::fs::read_dir(sysroot.join("bin"))? {
        let entry = entry?;
        if entry
            .path()
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("dll"))
        {
            inputs.insert(entry.path(), file(&entry.path())?);
        }
    }
    Ok(inputs)
}

fn known_program(program: &Path, sysroot: &Path, env: &Variables) -> io::Result<()> {
    if program.parent() == Some(sysroot.join("bin").as_path()) {
        return Ok(());
    }
    let home = super::config::home(env)
        .ok_or_else(|| io::Error::other("unknown executable selection home"))?;
    let rustup = home
        .join("bin")
        .join(format!("rustup{}", std::env::consts::EXE_SUFFIX));
    if file(program)? == file(&rustup)? {
        return Ok(());
    }
    Err(io::Error::other(
        "an opaque toolchain selector cannot have a complete observation key",
    ))
}

fn present(path: &Path, inputs: &mut BTreeMap<PathBuf, File>) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => {
            inputs.insert(path.to_path_buf(), file(path)?);
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => {}
        Err(source) => return Err(source),
    }
    Ok(())
}

fn validate_processes(toolchain: &Toolchain, processes: &[Process]) -> io::Result<()> {
    let mut cargo = false;
    let mut rustc = false;
    let mut sysroot = false;
    for process in processes {
        if !process.verified() {
            return Err(io::Error::other(
                "a bound observation lacks a successful actual process",
            ));
        }
        if process.exec.argv.last().is_some_and(|arg| arg == "-vV") {
            let said = std::str::from_utf8(&process.stdout).map_err(io::Error::other)?;
            let banner = super::parse_version(said).map_err(io::Error::other)?;
            cargo |= &banner == toolchain.cargo_version();
            rustc |= &banner == toolchain.rustc_version();
        } else if process
            .exec
            .argv
            .ends_with(&["--print".to_owned(), "sysroot".to_owned()])
        {
            let said = std::str::from_utf8(&process.stdout)
                .map_err(io::Error::other)?
                .trim();
            sysroot |= toolchain.sysroot() == Some(Path::new(said));
        }
    }
    if !cargo || !rustc || !sysroot {
        return Err(io::Error::other(
            "a bound toolchain lacks the paired banner and sysroot observations",
        ));
    }
    Ok(())
}

fn field(digest: &mut Sha256, bytes: &[u8]) -> io::Result<()> {
    digest.update(
        u64::try_from(bytes.len())
            .map_err(io::Error::other)?
            .to_be_bytes(),
    );
    digest.update(bytes);
    Ok(())
}

/// The observation purpose, exhaustively separated from compiler artifact and independent witness caches.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub(super) enum Role {
    Metadata,
    CargoBanner,
    RustcCfg,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    schema: String,
    key: String,
    role: Role,
    inputs: BTreeMap<PathBuf, File>,
    process: Process,
}

/// A single owner of one complete reusable command observation.
pub(super) struct Response {
    key: String,
    record: PathBuf,
    role: Role,
    inputs: BTreeMap<PathBuf, File>,
    lease: std::fs::File,
    toolchain: Toolchain,
}

impl Response {
    pub(super) fn open<W: Watch>(
        spec: &Spec,
        toolchain: &Toolchain,
        role: Role,
        watch: &W,
    ) -> io::Result<Self> {
        let (dir, env) = match (&spec.dir, &spec.env) {
            (Some(dir), Some(env)) => (dir, env),
            (Some(_), None) | (None, Some(_) | None) => {
                return Err(io::Error::other("an unbound observation command"));
            }
        };
        let inputs = response_inputs(spec, toolchain, role)?;
        let mut digest = Sha256::new();
        field(&mut digest, SCHEMA.as_bytes())?;
        field(
            &mut digest,
            &serde_json::to_vec(&role).map_err(io::Error::other)?,
        )?;
        field(&mut digest, dir.as_os_str().as_encoded_bytes())?;
        for argument in &spec.argv {
            field(&mut digest, argument.as_encoded_bytes())?;
        }
        for (name, value) in env.canonical() {
            field(&mut digest, name.as_encoded_bytes())?;
            field(&mut digest, value.as_encoded_bytes())?;
        }
        field(
            &mut digest,
            &serde_json::to_vec(&inputs).map_err(io::Error::other)?,
        )?;
        let key = hex::encode(digest.finalize());
        let directory = retained(env)?.join(&key);
        std::fs::create_dir_all(&directory)?;
        let lease = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("response.lock"))?;
        let started = std::time::Instant::now();
        let locked = lease.lock();
        watch.note("host-wait", &serde_json::json!({
            "owner": directory.display().to_string(), "cause": "bound command observation publication",
            "elapsed_ns": u64::try_from(started.elapsed().as_nanos()).map_err(io::Error::other)?,
            "machine": {"os": std::env::consts::OS, "cpus": std::thread::available_parallelism()?.get()}
        }).to_string());
        locked?;
        Ok(Self {
            key,
            record: directory.join("answer.json"),
            role,
            inputs,
            lease,
            toolchain: toolchain.clone(),
        })
    }

    pub(super) fn read(&self) -> io::Result<Vec<u8>> {
        let answer: Answer = crate::strictjson::decode_slice(&std::fs::read(&self.record)?)
            .map_err(io::Error::other)?;
        if answer.schema != SCHEMA
            || answer.key != self.key
            || answer.role != self.role
            || answer.inputs != self.inputs
            || !answer.process.verified()
        {
            return Err(io::Error::other(
                "an observation has no complete successful original process",
            ));
        }
        Ok(answer.process.stdout)
    }

    pub(super) fn publish(&self, spec: &Spec, result: &RunResult) -> io::Result<()> {
        if !result.succeeded() || result.stdout_truncated || result.leader.is_none() {
            return Err(io::Error::other("an incomplete actual observation"));
        }
        if response_inputs(spec, &self.toolchain, self.role)? != self.inputs {
            return Err(io::Error::other(
                "complete observation inputs changed during execution",
            ));
        }
        let answer = Answer {
            schema: SCHEMA.to_owned(),
            key: self.key.clone(),
            role: self.role,
            inputs: self.inputs.clone(),
            process: Process::actual(spec, result)?,
        };
        crate::replace::file(
            &self.record,
            &serde_json::to_vec(&answer).map_err(io::Error::other)?,
        )
        .map_err(|error| error.source)
    }

    pub(super) fn key(&self) -> &str {
        &self.key
    }
}

impl Drop for Response {
    fn drop(&mut self) {
        if let Err(source) = self.lease.unlock() {
            drop(source);
            std::process::abort();
        }
    }
}

fn response_inputs(
    spec: &Spec,
    toolchain: &Toolchain,
    role: Role,
) -> io::Result<BTreeMap<PathBuf, File>> {
    let (dir, env) = match (&spec.dir, &spec.env) {
        (Some(dir), Some(env)) => (dir, env),
        (Some(_), None) | (None, Some(_) | None) => {
            return Err(io::Error::other("an unbound observation command"));
        }
    };
    if env
        .var("RUST_TARGET_PATH")
        .is_some_and(|path| !path.is_empty())
    {
        return Err(io::Error::other("an external target search graph"));
    }
    match role {
        Role::Metadata => {
            if !spec.argv.iter().any(|arg| arg == "--locked")
                || !spec.argv.iter().any(|arg| arg == "--offline")
                || !std::fs::metadata(dir.join("Cargo.lock"))?.is_file()
            {
                return Err(io::Error::other("an unlocked metadata graph"));
            }
        }
        Role::CargoBanner | Role::RustcCfg => {}
    }
    let program = spec
        .argv
        .first()
        .ok_or_else(|| io::Error::other("an unnamed observation executable"))?;
    let sysroot = toolchain
        .sysroot()
        .ok_or_else(|| io::Error::other("an unobserved observation sysroot"))?;
    if Path::new(program).parent() != Some(sysroot.join("bin").as_path())
        && selecting_root((Path::new(program), Path::new(program)), env)? != sysroot
    {
        return Err(io::Error::other("an opaque observation selector"));
    }
    let mut inputs = observation_inputs(
        (toolchain.cargo(), toolchain.rustc(), Path::new(program)),
        (sysroot, dir, env),
    )?;
    for argument in &spec.argv {
        let path = Path::new(argument);
        if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        {
            let path = dir.join(path);
            inputs.insert(path.clone(), file(&path)?);
        }
    }
    if role == Role::Metadata
        && inputs
            .keys()
            .filter(|path| path.file_name().is_some_and(|name| name == "Cargo.toml"))
            .any(|path| !super::build_cache::plain_manifest(path))
    {
        return Err(io::Error::other(
            "metadata requires an unobserved dependency graph",
        ));
    }
    Ok(inputs)
}

/// Obtains a typed reusable observation or records the complete actual command that answered it.
pub(super) fn run<W: Watch>(
    spec: &Spec,
    toolchain: &Toolchain,
    role: Role,
    watch: &W,
) -> Result<Vec<u8>, CargoError> {
    let owned = match Response::open(spec, toolchain, role, watch) {
        Ok(owned) => Some(owned),
        Err(source) => {
            watch.note("tool-observation-unbound", &source.to_string());
            None
        }
    };
    if watch.cancel().is_cancelled() {
        return Err(CargoError::new(
            CargoErrorKind::Cancelled,
            "the command observation was cancelled",
        ));
    }
    if let Some(owned) = &owned {
        match owned.read() {
            Ok(stdout) => {
                watch.note("tool-observation-reuse", owned.key());
                return Ok(stdout);
            }
            Err(source) => watch.note("tool-observation-miss", &source.to_string()),
        }
    }
    let result = crate::runner::run(spec, watch.cancel());
    watch.exec(spec, &result);
    match role {
        Role::Metadata => {}
        Role::CargoBanner => crate::trace::record_probe(
            crate::trace::ProbeSite {
                vars: spec.env.as_ref(),
                root: toolchain_root(spec)?,
            },
            crate::trace::ProbeRole::CargoBanner,
            spec,
            &result,
        )?,
        Role::RustcCfg => crate::trace::record_probe(
            crate::trace::ProbeSite {
                vars: spec.env.as_ref(),
                root: toolchain_root(spec)?,
            },
            crate::trace::ProbeRole::RustcCfg,
            spec,
            &result,
        )?,
    }
    let name = match role {
        Role::Metadata => "cargo-metadata",
        Role::CargoBanner => "cargo-probe",
        Role::RustcCfg => "rustc-probe",
    };
    if result.leader.is_some() {
        let millis = u64::try_from(result.duration.as_millis())
            .map_err(|source| CargoError::new(CargoErrorKind::CommandFailed, source.to_string()))?;
        watch.note(name, &millis.to_string());
    }
    if !result.succeeded() {
        return Err(super::command_failed(spec, &result));
    }
    if result.stdout_truncated {
        return Err(CargoError::new(
            CargoErrorKind::CommandFailed,
            "the actual observation exceeded its output bound",
        ));
    }
    if let Some(owned) = &owned {
        match owned.publish(spec, &result) {
            Ok(()) => watch.note("tool-observation-bound", owned.key()),
            Err(source) => watch.note("tool-observation-unbound", &source.to_string()),
        }
    }
    Ok(result.stdout)
}

fn selecting_root((cargo, rustc): (&Path, &Path), env: &Variables) -> io::Result<PathBuf> {
    if cargo.parent() == rustc.parent()
        && let Some(parent) = cargo
            .parent()
            .filter(|parent| parent.file_name().is_some_and(|name| name == "bin"))
        && let Some(sysroot) = parent.parent()
        && installed(sysroot)?
    {
        return Ok(sysroot.to_path_buf());
    }
    let home = super::config::home(env).ok_or_else(|| io::Error::other("unknown selector home"))?;
    let rustup = home
        .join("bin")
        .join(format!("rustup{}", std::env::consts::EXE_SUFFIX));
    if file(cargo)? != file(&rustup)? || file(rustc)? != file(&rustup)? {
        return Err(io::Error::other("an opaque initial toolchain selector"));
    }
    let named = env
        .var("RUSTUP_TOOLCHAIN")
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::other("the known selector has no explicit toolchain binding"))?;
    let root = match env.var("RUSTUP_HOME") {
        Some(root) => PathBuf::from(root),
        None => PathBuf::from(
            env.var("HOME")
                .ok_or_else(|| io::Error::other("unknown toolchain home"))?,
        )
        .join(".rustup"),
    };
    let mut chosen = None;
    for entry in std::fs::read_dir(root.join("toolchains"))? {
        let entry = entry?;
        let entry_name = entry.file_name();
        let entry_name = entry_name
            .to_str()
            .ok_or_else(|| io::Error::other("non-textual toolchain identity"))?;
        if entry_name == named || entry_name.starts_with(&format!("{named}-")) {
            if chosen.is_some() {
                return Err(io::Error::other("the toolchain selection is ambiguous"));
            }
            chosen = Some(entry.path());
        }
    }
    chosen.ok_or_else(|| io::Error::other("the selected toolchain is not installed"))
}

fn toolchain_root(spec: &Spec) -> Result<&Path, CargoError> {
    spec.dir.as_deref().ok_or_else(|| {
        CargoError::new(
            CargoErrorKind::CommandFailed,
            "an actual toolchain probe has no named source root",
        )
    })
}

fn installed(sysroot: &Path) -> io::Result<bool> {
    match std::fs::metadata(sysroot.join("lib")) {
        Ok(metadata) => Ok(metadata.is_dir()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(source),
    }
}
