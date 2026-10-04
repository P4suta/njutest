// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Owned observations bound to executable content, the complete environment and source/configuration inputs.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::build_cache::toolchain::{Identities, Retained};
use super::build_cache::{File, configurations, file};
use super::{CargoError, CargoErrorKind, LocateOptions, Toolchain, VersionInfo};
use crate::runner::{Cancel, RunResult, Spec, Watch};
use crate::trace::ExecRecord;
use crate::vars::Variables;

const SCHEMA: &str = "rust-mutants-tool-observation-v4";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Inputs {
    files: BTreeMap<PathBuf, File>,
    programs: BTreeMap<PathBuf, Executable>,
    loader_digest: String,
    exclusions: Vec<String>,
}

impl Inputs {
    fn admitted_identities(&self) -> io::Result<BTreeMap<PathBuf, File>> {
        let mut files = self.files.clone();
        for program in self.programs.values() {
            if let Some(previous) = files.insert(program.resolved.clone(), program.content.clone())
                && previous != program.content
            {
                return Err(io::Error::other(
                    "one original executable has conflicting content identities",
                ));
            }
        }
        Ok(files)
    }

    fn verify(&self, actual: &Self, root: &Path) -> io::Result<()> {
        let mut changes = Vec::new();
        for path in self.files.keys().chain(
            actual
                .files
                .keys()
                .filter(|path| !self.files.contains_key(*path)),
        ) {
            if self.files.get(path) != actual.files.get(path) {
                changes.push(InputChange::File {
                    path: path.clone(),
                    before: self.files.get(path).cloned(),
                    after: actual.files.get(path).cloned(),
                });
            }
        }
        for path in self.programs.keys().chain(
            actual
                .programs
                .keys()
                .filter(|path| !self.programs.contains_key(*path)),
        ) {
            if self.programs.get(path) != actual.programs.get(path) {
                changes.push(InputChange::Program {
                    path: path.clone(),
                    before: self.programs.get(path).cloned(),
                    after: actual.programs.get(path).cloned(),
                });
            }
        }
        if self.loader_digest != actual.loader_digest {
            changes.push(InputChange::Loader {
                before: self.loader_digest.clone(),
                after: actual.loader_digest.clone(),
            });
        }
        if self.exclusions != actual.exclusions {
            changes.push(InputChange::Exclusions {
                before: self.exclusions.clone(),
                after: actual.exclusions.clone(),
            });
        }
        if changes.is_empty() {
            return Ok(());
        }
        Err(io::Error::other(ObservationInputsChangedError {
            root: root.to_path_buf(),
            changes,
        }))
    }
}

#[derive(Debug, Serialize)]
enum InputChange {
    File {
        path: PathBuf,
        before: Option<File>,
        after: Option<File>,
    },
    Program {
        path: PathBuf,
        before: Option<Executable>,
        after: Option<Executable>,
    },
    Loader {
        before: String,
        after: String,
    },
    Exclusions {
        before: Vec<String>,
        after: Vec<String>,
    },
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
#[error("{code}: observation inputs changed for {}: {changes:?}", root.display(), code = ObservationInputsChangedError::code().code)]
struct ObservationInputsChangedError {
    root: PathBuf,
    changes: Vec<InputChange>,
}

impl ObservationInputsChangedError {
    const fn code() -> crate::error::ErrorCode {
        crate::error::COMPILER_INPUT_UNREADABLE
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Executable {
    resolved: PathBuf,
    content: File,
}

impl Executable {
    fn of(path: &Path, identities: &Identities) -> io::Result<Self> {
        let resolved = std::fs::canonicalize(path)?;
        let content = super::build_cache::toolchain::identity(&resolved, identities)?;
        if std::fs::canonicalize(path)? != resolved {
            return Err(io::Error::other(format!(
                "the observed executable {} changed its resolution",
                path.display()
            )));
        }
        Ok(Self { resolved, content })
    }
}

#[derive(Clone, Serialize, Deserialize)]
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

    fn matches(&self, spec: &Spec) -> bool {
        if self.exec.argv.len() != spec.argv.len()
            || !self
                .exec
                .argv
                .iter()
                .zip(&spec.argv)
                .all(|(actual, expected)| std::ffi::OsStr::new(actual) == expected)
            || self.exec.dir.as_deref() != spec.dir.as_deref().and_then(Path::to_str)
        {
            return false;
        }
        let Some(env) = &spec.env else {
            return false;
        };
        let mut names: Vec<_> = self
            .exec
            .env_names
            .iter()
            .map(|name| env.spelling().canonical(std::ffi::OsStr::new(name)))
            .collect();
        names.sort();
        names
            == env
                .canonical()
                .into_iter()
                .map(|(name, _value)| name)
                .collect::<Vec<_>>()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: String,
    key: String,
    located: Located,
    inputs: Inputs,
    processes: Vec<Process>,
    identities: Retained,
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
    cursor: PathBuf,
    lease: std::fs::File,
    initial: Inputs,
    identities: Identities,
    exclusions: Vec<crate::glob::Pattern>,
}

fn identity_publication<W: Watch>(
    parent: &Path,
    (cargo, rustc): (&Path, &Path),
    watch: &W,
) -> io::Result<(PathBuf, std::fs::File)> {
    let mut publication = Sha256::new();
    for bytes in [
        SCHEMA.as_bytes(),
        b"owned-input-identities",
        cargo.as_os_str().as_encoded_bytes(),
        rustc.as_os_str().as_encoded_bytes(),
    ] {
        field(&mut publication, bytes)?;
    }
    let key = hex::encode(publication.finalize());
    let directory = parent.join(crate::keyed::name(&key)?);
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
    crate::keyed::bind(&directory, &key)?;
    Ok((directory.join("located.json"), lease))
}

impl Owner {
    fn acquire<W: Watch>(
        (options, dir): (&LocateOptions, &Path),
        (cargo, rustc): (&Path, &Path),
        exclusions: &[crate::glob::Pattern],
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
        let (cursor, lease) = identity_publication(&parent, (cargo, rustc), watch)?;
        let identities = match restore_identities(&cursor) {
            Ok(identities) => identities,
            Err(source) => {
                watch.note("toolchain-identity-miss", &source.to_string());
                Identities::empty()
            }
        };
        let initial = super::build_cache::toolchain::observation_environment((None, rustc), env)
            .and_then(|()| selecting_root((cargo, rustc), env, &identities))
            .and_then(|sysroot| {
                observation_inputs(
                    (cargo, rustc, cargo),
                    (&sysroot, dir, env),
                    (&identities, exclusions),
                )
            });
        watch.note(
            "toolchain-input-work",
            &serde_json::to_string(&identities.work()?).map_err(io::Error::other)?,
        );
        let initial = initial?;
        field(
            &mut digest,
            &serde_json::to_vec(&initial).map_err(io::Error::other)?,
        )?;
        let key = hex::encode(digest.finalize());
        let products = parent.join(crate::keyed::name(&key)?);
        std::fs::create_dir_all(&products)?;
        crate::keyed::bind(&products, &key)?;
        Ok(Self {
            key,
            record: products.join("located.json"),
            cursor,
            lease,
            initial,
            identities,
            exclusions: exclusions.to_vec(),
        })
    }

    fn read(&self, (options, dir): (&LocateOptions, &Path)) -> io::Result<Toolchain> {
        let mut record: Record = crate::strictjson::decode_slice(&std::fs::read(&self.record)?)
            .map_err(io::Error::other)?;
        if record.schema != SCHEMA || record.key != self.key || record.processes.is_empty() {
            return Err(io::Error::other("an unbound tool observation record"));
        }
        let observed = Toolchain::observed(record.located.clone(), options.env.clone())
            .map_err(io::Error::other)?
            .with_identities(&self.identities)
            .with_exclusions(&self.exclusions);
        record
            .inputs
            .verify(&observed_inputs(&observed, dir)?, dir)?;
        record.inputs.verify(&self.initial, dir)?;
        validate_processes(&observed, dir, &record.processes)?;
        record.identities.merge(
            &self
                .identities
                .retain(&record.inputs.admitted_identities()?)?,
        )?;
        self.identities.accept(record.identities.clone())?;
        let bytes = serde_json::to_vec(&record).map_err(io::Error::other)?;
        crate::replace::file(&self.record, &bytes).map_err(|error| error.source)?;
        crate::replace::file(&self.cursor, &bytes).map_err(|error| error.source)?;
        self.identities
            .attach((&self.record, &self.cursor), &self.key)?;
        Ok(observed)
    }

    fn publish(
        &self,
        (toolchain, dir): (&Toolchain, &Path),
        processes: Vec<Process>,
    ) -> io::Result<()> {
        let inputs = observed_inputs(toolchain, dir)?;
        self.initial.verify(&inputs, dir)?;
        validate_processes(toolchain, dir, &processes)?;
        let identities = toolchain
            .identities()
            .retain(&inputs.admitted_identities()?)?;
        let record = Record {
            schema: SCHEMA.to_owned(),
            key: self.key.clone(),
            located: Located::of(toolchain)?,
            inputs,
            processes,
            identities,
        };
        crate::replace::file(
            &self.record,
            &serde_json::to_vec(&record).map_err(io::Error::other)?,
        )
        .map_err(|error| error.source)?;
        crate::replace::file(
            &self.cursor,
            &serde_json::to_vec(&record).map_err(io::Error::other)?,
        )
        .map_err(|error| error.source)?;
        self.identities
            .attach((&self.record, &self.cursor), &self.key)
    }
}

fn restore_identities(path: &Path) -> io::Result<Identities> {
    let bytes = std::fs::read(path)?;
    let record: Record = crate::strictjson::decode_slice(&bytes).map_err(io::Error::other)?;
    if record.schema != SCHEMA
        || record.key.len() != 64
        || !record
            .key
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        || record.processes.is_empty()
    {
        return Err(io::Error::other(
            "an unbound toolchain identity publication",
        ));
    }
    let owner = path
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| io::Error::other("the identity publication has no owner"))?;
    if std::fs::read(
        owner
            .join(crate::keyed::name(&record.key)?)
            .join("located.json"),
    )? != bytes
    {
        return Err(io::Error::other(
            "the identity cursor is not its original publication",
        ));
    }
    let original = Toolchain::observed(record.located, None).map_err(io::Error::other)?;
    let root = record
        .processes
        .first()
        .and_then(|process| process.exec.dir.as_deref())
        .ok_or_else(|| io::Error::other("the original toolchain producer has no source root"))?;
    validate_processes(&original, Path::new(root), &record.processes)?;
    Identities::restore(record.identities, &record.inputs.admitted_identities()?)
}

pub(in crate::cargo) fn persist_identities(
    (path, cursor): (&Path, &Path),
    key: &str,
    identities: &Identities,
) -> io::Result<()> {
    let directory = cursor
        .parent()
        .ok_or_else(|| io::Error::other("the identity publication has no owner"))?;
    let lease = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(directory.join("observation.lock"))?;
    lease.lock()?;
    let mut record: Record =
        crate::strictjson::decode_slice(&std::fs::read(path)?).map_err(io::Error::other)?;
    if record.schema != SCHEMA || record.key != key {
        return Err(io::Error::other(
            "the original identity publication changed ownership",
        ));
    }
    let mut published: Record =
        crate::strictjson::decode_slice(&std::fs::read(cursor)?).map_err(io::Error::other)?;
    let repointed = published.key != record.key;
    let current = restore_identities(cursor)?;
    record
        .identities
        .merge(&current.retain(&BTreeMap::new())?)?;
    record
        .identities
        .merge(&identities.retain(&record.inputs.admitted_identities()?)?)?;
    if repointed {
        published.identities.merge(&record.identities)?;
        let owner = cursor
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| io::Error::other("the identity publication has no owner"))?;
        let bytes = serde_json::to_vec(&published).map_err(io::Error::other)?;
        crate::replace::file(cursor, &bytes).map_err(|error| error.source)?;
        crate::replace::file(
            &owner
                .join(crate::keyed::name(&published.key)?)
                .join("located.json"),
            &bytes,
        )
        .map_err(|error| error.source)?;
        let older = serde_json::to_vec(&record).map_err(io::Error::other)?;
        crate::replace::file(path, &older).map_err(|error| error.source)?;
        return identities.accept(published.identities);
    }
    let bytes = serde_json::to_vec(&record).map_err(io::Error::other)?;
    crate::replace::file(path, &bytes).map_err(|error| error.source)?;
    crate::replace::file(cursor, &bytes).map_err(|error| error.source)?;
    identities.accept(record.identities)
}

/// Reuses only a complete, unchanged observation whose original actual processes remain retained.
pub(super) fn locate<W: Watch>(
    (options, dir, exclusions): (&LocateOptions, &Path, &[crate::glob::Pattern]),
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
    let owner = match Owner::acquire((options, dir), (cargo, rustc), exclusions, watch) {
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
    let toolchain = fresh(&collected)?.with_exclusions(exclusions);
    let toolchain = match &owner {
        Some(owner) => toolchain.with_identities(&owner.identities),
        None => toolchain,
    };
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

fn observed_inputs(toolchain: &Toolchain, dir: &Path) -> io::Result<Inputs> {
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
        (toolchain.identities(), toolchain.observation_exclusions()),
    )
}

fn observation_inputs(
    (cargo, rustc, selecting): (&Path, &Path, &Path),
    (sysroot, dir, env): (&Path, &Path, &Variables),
    (identities, exclusions): (&Identities, &[crate::glob::Pattern]),
) -> io::Result<Inputs> {
    super::build_cache::toolchain::observation_environment((Some(sysroot), rustc), env)?;
    let loaders = super::build_cache::loaders::Inputs::observation(env, identities)?;
    super::build_cache::toolchain::environment(sysroot, rustc, env, &loaders)?;
    let mut inputs = BTreeMap::new();
    let mut programs = BTreeMap::new();
    let selected_rustc = selecting.with_file_name(format!("rustc{}", std::env::consts::EXE_SUFFIX));
    let direct_cargo = sysroot
        .join("bin")
        .join(format!("cargo{}", std::env::consts::EXE_SUFFIX));
    let direct_rustc = sysroot
        .join("bin")
        .join(format!("rustc{}", std::env::consts::EXE_SUFFIX));
    for program in [
        cargo,
        rustc,
        selecting,
        selected_rustc.as_path(),
        direct_cargo.as_path(),
        direct_rustc.as_path(),
    ] {
        known_program(program, sysroot, env, identities)?;
        programs.insert(program.to_path_buf(), Executable::of(program, identities)?);
    }
    source_tree(dir, dir, exclusions, &mut inputs)?;
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
            inputs.insert(
                entry.path(),
                super::build_cache::toolchain::identity(&entry.path(), identities)?,
            );
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
            inputs.insert(
                entry.path(),
                super::build_cache::toolchain::identity(&entry.path(), identities)?,
            );
        }
    }
    identities.persist()?;
    Ok(Inputs {
        files: inputs,
        programs,
        loader_digest: loaders.digest().to_owned(),
        exclusions: exclusions.iter().map(ToString::to_string).collect(),
    })
}

fn source_tree(
    root: &Path,
    directory: &Path,
    exclusions: &[crate::glob::Pattern],
    inputs: &mut BTreeMap<PathBuf, File>,
) -> io::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if path.starts_with(root.join("target")) {
            continue;
        }
        let relative = path.strip_prefix(root).map_err(io::Error::other)?;
        let components = relative
            .components()
            .map(|component| {
                component
                    .as_os_str()
                    .to_str()
                    .ok_or_else(|| io::Error::other("a non-textual source graph path"))
            })
            .collect::<io::Result<Vec<_>>>()?;
        if exclusions
            .iter()
            .any(|pattern| pattern.matches(&components.join("/")))
        {
            continue;
        }
        if entry.file_type()?.is_dir() {
            source_tree(root, &path, exclusions, inputs)?;
        } else {
            inputs.insert(path.clone(), file(&path)?);
        }
    }
    Ok(())
}

fn known_program(
    program: &Path,
    sysroot: &Path,
    env: &Variables,
    identities: &Identities,
) -> io::Result<()> {
    if program.parent() == Some(sysroot.join("bin").as_path()) {
        return Ok(());
    }
    let home = super::config::home(env)
        .ok_or_else(|| io::Error::other("unknown executable selection home"))?;
    let rustup = home
        .join("bin")
        .join(format!("rustup{}", std::env::consts::EXE_SUFFIX));
    if Executable::of(program, identities)?.content == Executable::of(&rustup, identities)?.content
    {
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

fn validate_processes(toolchain: &Toolchain, dir: &Path, processes: &[Process]) -> io::Result<()> {
    let mut cargo = false;
    let mut rustc = false;
    let mut sysroot = false;
    for process in processes {
        if !process.verified() || process.exec.dir.as_deref() != dir.to_str() {
            return Err(io::Error::other(
                "a bound observation lacks a successful actual process",
            ));
        }
        let role = located_role(toolchain, &process.exec.argv)?;
        match role {
            LocatedRole::CargoBanner | LocatedRole::RustcBanner => {
                let said = std::str::from_utf8(&process.stdout).map_err(io::Error::other)?;
                let banner = super::parse_version(said).map_err(io::Error::other)?;
                match role {
                    LocatedRole::CargoBanner => cargo |= &banner == toolchain.cargo_version(),
                    LocatedRole::RustcBanner => rustc |= &banner == toolchain.rustc_version(),
                    LocatedRole::Sysroot => {
                        return Err(io::Error::other("a sysroot cannot supply a banner"));
                    }
                }
            }
            LocatedRole::Sysroot => {
                let said = std::str::from_utf8(&process.stdout)
                    .map_err(io::Error::other)?
                    .trim();
                sysroot |= toolchain.sysroot() == Some(Path::new(said));
            }
        }
    }
    if !cargo || !rustc || !sysroot {
        return Err(io::Error::other(
            "a bound toolchain lacks the paired banner and sysroot observations",
        ));
    }
    Ok(())
}

enum LocatedRole {
    CargoBanner,
    RustcBanner,
    Sysroot,
}

fn located_role(toolchain: &Toolchain, argv: &[String]) -> io::Result<LocatedRole> {
    let program = argv
        .first()
        .ok_or_else(|| io::Error::other("an unnamed observation"))?;
    let program = Path::new(program);
    let cargo = [toolchain.cargo(), toolchain.selecting().path()].contains(&program);
    let selected_rustc = toolchain
        .selecting()
        .path()
        .with_file_name(format!("rustc{}", std::env::consts::EXE_SUFFIX));
    let rustc = [toolchain.rustc(), selected_rustc.as_path()].contains(&program);
    if argv.len() == 2 && argv.last().is_some_and(|arg| arg == "-vV") {
        if cargo {
            return Ok(LocatedRole::CargoBanner);
        }
        if rustc {
            return Ok(LocatedRole::RustcBanner);
        }
    }
    if rustc && argv.len() == 3 && argv.ends_with(&["--print".to_owned(), "sysroot".to_owned()]) {
        return Ok(LocatedRole::Sysroot);
    }
    Err(io::Error::other(
        "the actual executable and observation purpose differ from the bound toolchain",
    ))
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
enum Role {
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
    inputs: Inputs,
    process: Process,
}

/// A single owner of one complete reusable command observation.
struct Response {
    key: String,
    record: PathBuf,
    role: Role,
    inputs: Inputs,
    lease: std::fs::File,
    toolchain: Toolchain,
}

impl Response {
    fn open<W: Watch>(
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
        let inputs = response_inputs(spec, toolchain, role);
        watch.note(
            "tool-observation-input-work",
            &serde_json::to_string(&toolchain.identities().work()?).map_err(io::Error::other)?,
        );
        let inputs = inputs?;
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
        let directory = retained(env)?.join(crate::keyed::name(&key)?);
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
        crate::keyed::bind(&directory, &key)?;
        Ok(Self {
            key,
            record: directory.join("answer.json"),
            role,
            inputs,
            lease,
            toolchain: toolchain.clone(),
        })
    }

    fn read(&self, spec: &Spec) -> io::Result<Vec<u8>> {
        let answer: Answer = crate::strictjson::decode_slice(&std::fs::read(&self.record)?)
            .map_err(io::Error::other)?;
        if answer.schema != SCHEMA
            || answer.key != self.key
            || answer.role != self.role
            || answer.inputs != self.inputs
            || !answer.process.verified()
            || !answer.process.matches(spec)
        {
            return Err(io::Error::other(
                "an observation has no complete successful original process",
            ));
        }
        validate_answer_inputs(self.role, &self.inputs, &answer.process.stdout)?;
        Ok(answer.process.stdout)
    }

    fn publish(&self, spec: &Spec, result: &RunResult) -> io::Result<()> {
        if !result.succeeded() || result.stdout_truncated || result.leader.is_none() {
            return Err(io::Error::other("an incomplete actual observation"));
        }
        self.inputs.verify(
            &response_inputs(spec, &self.toolchain, self.role)?,
            toolchain_root(spec).map_err(io::Error::other)?,
        )?;
        validate_answer_inputs(self.role, &self.inputs, &result.stdout)?;
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

    fn key(&self) -> &str {
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

fn response_inputs(spec: &Spec, toolchain: &Toolchain, role: Role) -> io::Result<Inputs> {
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
    super::build_cache::toolchain::observation_environment(
        (Some(sysroot), toolchain.rustc()),
        env,
    )?;
    if Path::new(program).parent() != Some(sysroot.join("bin").as_path())
        && selecting_root(
            (Path::new(program), Path::new(program)),
            env,
            toolchain.identities(),
        )? != sysroot
    {
        return Err(io::Error::other("an opaque observation selector"));
    }
    let mut inputs = observation_inputs(
        (toolchain.cargo(), toolchain.rustc(), Path::new(program)),
        (sysroot, dir, env),
        (toolchain.identities(), toolchain.observation_exclusions()),
    )?;
    for argument in &spec.argv {
        let path = Path::new(argument);
        if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        {
            let path = dir.join(path);
            inputs.files.insert(path.clone(), file(&path)?);
        }
    }
    if role == Role::Metadata
        && inputs
            .files
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

fn validate_answer_inputs(role: Role, inputs: &Inputs, stdout: &[u8]) -> io::Result<()> {
    match role {
        Role::Metadata => {
            let metadata: super::Metadata =
                crate::strictjson::decode_slice(stdout).map_err(io::Error::other)?;
            for package in &metadata.packages {
                for path in std::iter::once(package.manifest_path.as_path()).chain(
                    package
                        .targets
                        .iter()
                        .map(|target| target.src_path.as_path()),
                ) {
                    if !inputs.files.contains_key(path) {
                        return Err(io::Error::other(format!(
                            "metadata read outside its owned source graph: {}",
                            path.display()
                        )));
                    }
                }
            }
        }
        Role::CargoBanner | Role::RustcCfg => {}
    }
    Ok(())
}

/// Obtains a typed reusable observation or records the complete actual command that answered it.
fn run<W: Watch>(
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
        match owned.read(spec) {
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

/// Obtains metadata under the caller's actual execution recorder, with no second standalone publication.
pub(super) fn metadata<W: Watch>(
    spec: &Spec,
    toolchain: &Toolchain,
    watch: &W,
) -> Result<Vec<u8>, CargoError> {
    run(spec, toolchain, Role::Metadata, watch)
}

/// A standalone probe has one actual process publisher and cannot carry a costed execution watch.
#[derive(Clone, Copy)]
pub(super) enum Standalone {
    CargoBanner,
    RustcCfg,
}

pub(super) fn standalone(
    spec: &Spec,
    toolchain: &Toolchain,
    purpose: Standalone,
    cancel: &Cancel,
) -> Result<Vec<u8>, CargoError> {
    let trace = crate::trace::Recorder::disabled();
    let watch = crate::runner::Watched::new(cancel, &trace);
    let role = match purpose {
        Standalone::CargoBanner => Role::CargoBanner,
        Standalone::RustcCfg => Role::RustcCfg,
    };
    run(spec, toolchain, role, &watch)
}

fn selecting_root(
    (cargo, rustc): (&Path, &Path),
    env: &Variables,
    identities: &Identities,
) -> io::Result<PathBuf> {
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
    let chosen =
        chosen.ok_or_else(|| io::Error::other("the selected toolchain is not installed"))?;
    super::build_cache::toolchain::observation_environment((Some(&chosen), rustc), env)?;
    let selector = Executable::of(&rustup, identities)?;
    if Executable::of(cargo, identities)?.content != selector.content
        || Executable::of(rustc, identities)?.content != selector.content
    {
        return Err(io::Error::other("an opaque initial toolchain selector"));
    }
    Ok(chosen)
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    fn one_publication_with_a_supplemental_capture<W: crate::runner::Watch>(
        (source, cache, supplemental): (&Path, &Path, &Path),
        watch: &W,
    ) -> std::io::Result<(Toolchain, crate::vars::Variables)> {
        let mut env: crate::vars::Variables = njutest_devkit::paths::environment_for_a_run()
            .into_iter()
            .collect();
        env.set("RUSTC_WRAPPER", "");
        env.set("RUSTC_WORKSPACE_WRAPPER", "");
        env.set("NJUTEST_FIXTURE_BUILD_CACHE", cache);
        let options = LocateOptions {
            cargo: Some(njutest_devkit::paths::cargo_binary()),
            env: Some(env.clone()),
            ..LocateOptions::default()
        };
        let owner = Toolchain::locate(&options, source, watch).map_err(std::io::Error::other)?;
        let p = supplemental.join("p");
        std::fs::write(&p, b"genuine")?;
        super::super::build_cache::toolchain::identity(&p, owner.identities())?;
        owner.identities().persist()?;
        Ok((owner, env))
    }

    #[test]
    fn a_consistently_rewritten_cursor_is_refused_without_its_publication() {
        let source = tempfile::tempdir().expect("owned observation source");
        let cache = tempfile::tempdir().expect("owned observation publication");
        let extra = tempfile::tempdir().expect("owned supplemental inputs");
        let cargo = njutest_devkit::paths::cargo_binary();
        let rustc = cargo.with_file_name(format!("rustc{}", std::env::consts::EXE_SUFFIX));
        let cancel = Cancel::new();
        let trace = recorder();
        let watch = Watched::new(&cancel, &trace);
        let (_, env) = one_publication_with_a_supplemental_capture(
            (source.path(), cache.path(), extra.path()),
            &watch,
        )
        .expect("one genuine publication with a supplemental capture");
        let p = extra.path().join("p");
        let (cursor, lease) = super::identity_publication(
            &super::retained(&env).expect("owned publication root"),
            (&cargo, &rustc),
            &watch,
        )
        .expect("owned publication read lease");
        drop(lease);
        let genuine = std::fs::read(&cursor).expect("published cursor bytes");
        let mut rewritten: serde_json::Value = crate::strictjson::decode_slice(&genuine)
            .map_err(std::io::Error::other)
            .expect("published record");
        let key = std::fs::canonicalize(&p)
            .expect("canonical supplemental object")
            .display()
            .to_string();
        let forged = "b".repeat(64);
        let identities = rewritten
            .get_mut("identities")
            .expect("the publication retains its identities");
        identities
            .get_mut("files")
            .and_then(|files| files.get_mut(key.as_str()))
            .expect("the genuine publication retains the supplemental capture")
            .get_mut("content")
            .and_then(|content| content.as_object_mut())
            .expect("a retained file content")
            .insert(String::from("digest"), serde_json::json!(&forged));
        let captures = identities
            .get_mut("captures")
            .and_then(|captures| captures.get_mut(key.as_str()))
            .and_then(|captures| captures.as_array_mut())
            .expect("the genuine publication retains its actual captures");
        for capture in captures {
            capture
                .get_mut("content")
                .and_then(|content| content.as_object_mut())
                .expect("a retained capture content")
                .insert(String::from("digest"), serde_json::json!(&forged));
        }
        std::fs::write(
            &cursor,
            serde_json::to_vec(&rewritten)
                .map_err(std::io::Error::other)
                .expect("rewritten record bytes"),
        )
        .expect("consistently rewritten cursor");
        let refusal = super::restore_identities(&cursor)
            .expect_err("two mutually consistent maps cannot attest alone");
        assert!(
            refusal.to_string().contains("not its original publication"),
            "{refusal}"
        );
        std::fs::write(&cursor, &genuine).expect("restored genuine cursor");
        let restored = super::restore_identities(&cursor).expect("genuine original publication");
        assert_eq!(
            super::super::build_cache::toolchain::identity(&p, &restored)
                .expect("genuine retained capture"),
            super::super::build_cache::toolchain::identity(&p, &super::Identities::empty())
                .expect("independently read genuine capture")
        );
    }

    #[test]
    fn independent_identity_owners_keep_both_actual_supplemental_captures() {
        let source = tempfile::tempdir().expect("owned observation source");
        let cache = tempfile::tempdir().expect("owned observation publication");
        let extra = tempfile::tempdir().expect("owned supplemental inputs");
        let mut env: crate::vars::Variables = njutest_devkit::paths::environment_for_a_run()
            .into_iter()
            .collect();
        env.set("RUSTC_WRAPPER", "");
        env.set("RUSTC_WORKSPACE_WRAPPER", "");
        env.set("NJUTEST_FIXTURE_BUILD_CACHE", cache.path());
        let cargo = njutest_devkit::paths::cargo_binary();
        let rustc = cargo.with_file_name(format!("rustc{}", std::env::consts::EXE_SUFFIX));
        let options = LocateOptions {
            cargo: Some(cargo.clone()),
            env: Some(env.clone()),
            ..LocateOptions::default()
        };
        let cancel = Cancel::new();
        let trace = recorder();
        let watch = Watched::new(&cancel, &trace);
        let first = Toolchain::locate(&options, source.path(), &watch).expect("actual first owner");
        let second =
            Toolchain::locate(&options, source.path(), &watch).expect("actual second owner");
        let p = extra.path().join("p");
        let q = extra.path().join("q");
        std::fs::write(&p, b"first").expect("first actual supplemental input");
        std::fs::write(&q, b"other").expect("second actual supplemental input");
        for path in [&p, &q] {
            super::super::build_cache::toolchain::tests::settle(path);
        }
        super::super::build_cache::toolchain::identity(&p, first.identities())
            .expect("first actual capture");
        super::super::build_cache::toolchain::identity(&q, second.identities())
            .expect("second actual capture");
        first
            .identities()
            .persist()
            .expect("first actual publication");
        second
            .identities()
            .persist()
            .expect("second actual publication");
        let (cursor, lease) = super::identity_publication(
            &super::retained(&env).expect("owned publication root"),
            (&cargo, &rustc),
            &watch,
        )
        .expect("owned publication read lease");
        drop(lease);
        let restored = super::restore_identities(&cursor).expect("original actual captures");
        for path in [&p, &q] {
            super::super::build_cache::toolchain::identity(path, &restored)
                .expect("unchanged actual input");
        }
        assert_eq!(
            restored.work().expect("actual cumulative publication work"),
            (0, 0, Vec::new()),
            "serialized owners must retain both genuine additions"
        );
    }

    fn two_content_keys<W: crate::runner::Watch>(
        (source, cache): (&Path, &Path),
        (trace, watch): (&Recorder, &W),
    ) -> std::io::Result<(Toolchain, PathBuf, [String; 2])> {
        let mut env: crate::vars::Variables = njutest_devkit::paths::environment_for_a_run()
            .into_iter()
            .collect();
        env.set("RUSTC_WRAPPER", "");
        env.set("RUSTC_WORKSPACE_WRAPPER", "");
        env.set("NJUTEST_FIXTURE_BUILD_CACHE", cache);
        let options = LocateOptions {
            cargo: Some(njutest_devkit::paths::cargo_binary()),
            env: Some(env.clone()),
            ..LocateOptions::default()
        };
        let older = Toolchain::locate(&options, source, watch).map_err(std::io::Error::other)?;
        std::fs::write(source.join("later"), b"changed source inputs")?;
        let newer = Toolchain::locate(&options, source, watch).map_err(std::io::Error::other)?;
        if older.cargo_version() != newer.cargo_version() {
            return Err(std::io::Error::other(
                "both actual content keys must observe one toolchain",
            ));
        }
        let keys = trace
            .events()
            .iter()
            .filter_map(|event| match &event.payload {
                Payload::Note { note } if note.kind == "toolchain-observation-bound" => {
                    Some(note.detail.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        println!("actual publication keys: {keys:?}");
        let [older_key, newer_key] = keys.as_slice() else {
            return Err(std::io::Error::other(format!(
                "both actual content keys must publish: {keys:?}"
            )));
        };
        let root = super::retained(&env)?;
        Ok((older, root, [older_key.clone(), newer_key.clone()]))
    }

    #[test]
    fn an_older_content_key_neither_rehashes_nor_repoints_the_identity_cursor() {
        let source = tempfile::tempdir().expect("owned observation source");
        let cache = tempfile::tempdir().expect("owned observation publication");
        let extra = tempfile::tempdir().expect("owned supplemental inputs");
        let cargo = njutest_devkit::paths::cargo_binary();
        let cancel = Cancel::new();
        let trace = recorder();
        let watch = Watched::new(&cancel, &trace);
        let (older, root, keys) = two_content_keys((source.path(), cache.path()), (&trace, &watch))
            .expect("both actual content key publications");
        let Some(newer_key) = keys.last() else {
            panic!("both actual content keys must publish: {keys:?}");
        };
        let newer_record = root
            .join(crate::keyed::name(newer_key).expect("a publication key"))
            .join("located.json");
        let (cursor, lease) = super::identity_publication(
            &root,
            (
                &cargo,
                &cargo.with_file_name(format!("rustc{}", std::env::consts::EXE_SUFFIX)),
            ),
            &watch,
        )
        .expect("owned publication read lease");
        drop(lease);
        let cursor_bytes = std::fs::read(&cursor).expect("current cursor publication");
        assert_eq!(
            cursor_bytes,
            std::fs::read(&newer_record).expect("newer original publication"),
            "the cursor must hold the newer actual content key"
        );
        let p = extra.path().join("p");
        std::fs::write(&p, b"older").expect("older actual supplemental input");
        super::super::build_cache::toolchain::identity(&p, older.identities())
            .expect("older actual capture");
        let before = older.identities().work().expect("older actual work");
        println!("actual older owner work before the superseded publication: {before:?}");
        older
            .identities()
            .persist()
            .expect("a superseded content key still publishes its actual captures");
        assert_eq!(
            older.identities().work().expect("unchanged older work"),
            before,
            "the superseded older publication must read no input bytes"
        );
        let cursor_record: serde_json::Value = crate::strictjson::decode_slice(
            &std::fs::read(&cursor).expect("current cursor publication"),
        )
        .map_err(std::io::Error::other)
        .expect("cursor publication record");
        assert_eq!(
            cursor_record.get("key").and_then(serde_json::Value::as_str),
            Some(newer_key.as_str()),
            "the superseded older publication must not repoint the cursor"
        );
        assert_eq!(
            std::fs::read(&cursor).expect("current cursor publication"),
            std::fs::read(&newer_record).expect("newer original publication"),
            "the cursor must stay bound to the newer original publication"
        );
        let restored = super::restore_identities(&cursor).expect("bound cursor publication");
        assert_eq!(
            restored.work().expect("restored publication work"),
            (0, 0, Vec::new()),
            "the retained captures restore without reading"
        );
        assert_eq!(
            super::super::build_cache::toolchain::identity(&p, &restored)
                .expect("cumulative retained capture"),
            super::super::build_cache::toolchain::identity(&p, &super::Identities::empty())
                .expect("independently read older capture"),
            "the superseded publication's captures must be retained cumulatively"
        );
    }

    use std::path::{Path, PathBuf};

    use crate::cargo::{LocateOptions, Toolchain};
    use crate::runner::{Cancel, Watched};
    use crate::trace::{MemorySink, Payload, Recorder, Sink};

    fn recorder() -> Recorder {
        Recorder::wall(
            Sink::Memory(MemorySink::unbounded()),
            crate::testkit::trace::standalone_context(),
        )
    }

    #[test]
    fn a_refused_observation_digests_no_loader_inputs() {
        let directory = tempfile::tempdir().expect("owned observation inputs");
        let fallback = directory.path().join("fallback");
        std::fs::create_dir_all(&fallback).expect("owned fallback namespace");
        for name in ["one", "two", "three"] {
            std::fs::write(fallback.join(name), b"owned-one").expect("owned loader input");
        }
        let mut env = crate::vars::Variables::of([(
            "DYLD_FALLBACK_LIBRARY_PATH".into(),
            fallback.into_os_string(),
        )]);
        env.set("HOME", directory.path());
        for name in [
            "LD_LIBRARY_PATH",
            "RUSTC_WRAPPER",
            "RUSTC_WORKSPACE_WRAPPER",
            "RUSTC",
            "RUSTDOC",
            "LD_PRELOAD",
            "DYLD_INSERT_LIBRARIES",
            "DYLD_LIBRARY_PATH",
            "DYLD_FRAMEWORK_PATH",
            "DYLD_FALLBACK_FRAMEWORK_PATH",
            "COMPILER_PATH",
            "GCC_EXEC_PREFIX",
            "LIBRARY_PATH",
            "CARGO_TARGET_OWNED_LINKER",
            "CARGO_TARGET_OWNED_RUNNER",
            "CARGO_TARGET_OWNED_RUSTC",
            "CARGO_TARGET_OWNED_RUSTDOC",
            "CARGO_TARGET_OWNED_RUSTC_WRAPPER",
            "CARGO_TARGET_OWNED_RUSTC_WORKSPACE_WRAPPER",
            "CARGO_TARGET_OWNED_RUSTFLAGS",
            "CARGO_TARGET_OWNED_RUSTDOCFLAGS",
        ] {
            let mut rejected = env.clone();
            rejected.set(name, "opaque");
            let identities = super::Identities::empty();
            let result = super::observation_inputs(
                (directory.path(), directory.path(), directory.path()),
                (directory.path(), directory.path(), &rejected),
                (&identities, &[]),
            );
            let refusal = result.expect_err("an opaque graph is still refused");
            assert!(refusal.to_string().contains(name), "{name}: {refusal}");
            let work = identities.work().expect("actual file-boundary work");
            println!("{name}: actual observation input work: {work:?}");
            assert_eq!(
                work,
                (0, 0, Vec::new()),
                "{name}: actual input work: {work:?}"
            );
        }
    }

    #[test]
    fn a_refused_rustdoc_selector_digests_no_executables_or_loaders() {
        let directory = tempfile::tempdir().expect("owned selector observation");
        let mut env: crate::vars::Variables = njutest_devkit::paths::environment_for_a_run()
            .into_iter()
            .collect();
        env.set("RUSTC_WRAPPER", "");
        env.set("RUSTC_WORKSPACE_WRAPPER", "");
        env.set("RUSTDOC", "opaque");
        env.set("NJUTEST_FIXTURE_BUILD_CACHE", directory.path());
        let bin = super::super::config::home(&env)
            .expect("actual selector home")
            .join("bin");
        let cargo = bin.join("cargo");
        let rustc = bin.join("rustc");
        let options = LocateOptions {
            cargo: Some(cargo.clone()),
            env: Some(env),
            ..LocateOptions::default()
        };
        let cancel = Cancel::new();
        let trace = recorder();
        let result = super::Owner::acquire(
            (&options, directory.path()),
            (&cargo, &rustc),
            &[],
            &Watched::new(&cancel, &trace),
        );
        let refusal = match result {
            Err(source) => source,
            Ok(_) => panic!("opaque RUSTDOC must refuse ownership"),
        };
        assert!(refusal.to_string().contains("RUSTDOC"), "{refusal}");
        let work: Vec<(u64, u64, Vec<PathBuf>)> = trace
            .events()
            .iter()
            .filter_map(|event| match &event.payload {
                Payload::Note { note } if note.kind == "toolchain-input-work" => Some(
                    crate::strictjson::decode_slice(note.detail.as_bytes())
                        .expect("actual selector work"),
                ),
                _ => None,
            })
            .collect();
        println!("actual refused selector work: {work:?}");
        assert_eq!(work, vec![(0, 0, Vec::new())]);
    }

    #[test]
    fn a_bound_loader_namespace_counts_real_reads_and_reuses_only_stable_contents() {
        let directory = tempfile::tempdir().expect("owned loader inputs");
        let library = super::super::build_cache::loaders::tests::test_library(b"owned-one");
        let size = u64::try_from(library.len()).expect("a small library");
        for name in ["one", "two", "three"] {
            std::fs::write(directory.path().join(name), &library).expect("owned input");
        }
        for name in ["one", "two", "three"] {
            super::super::build_cache::toolchain::tests::settle(&directory.path().join(name));
        }
        let env = crate::vars::Variables::of([(
            "DYLD_FALLBACK_LIBRARY_PATH".into(),
            directory.path().as_os_str().to_os_string(),
        )]);
        let identities = super::Identities::empty();
        let first = super::super::build_cache::loaders::Inputs::observation(&env, &identities)
            .expect("actual bound fallback inputs");
        let root = std::fs::canonicalize(directory.path()).expect("canonical owned inputs");
        let expected = (
            3,
            3 * size,
            ["one", "three", "two"].map(|name| root.join(name)).to_vec(),
        );
        assert_eq!(identities.work().expect("actual file work"), expected);
        println!("actual captured loader work: {expected:?}");
        let again = super::super::build_cache::loaders::Inputs::observation(&env, &identities)
            .expect("the same retained loader namespace");
        assert_eq!(first.digest(), again.digest());
        assert_eq!(
            identities.work().expect("memo reuse does not read"),
            expected
        );
        std::fs::write(
            root.join("one"),
            super::super::build_cache::loaders::tests::test_library(b"other-one"),
        )
        .expect("changed actual loader input");
        let changed = super::super::build_cache::loaders::Inputs::observation(&env, &identities)
            .expect("changed inputs are freshly captured");
        assert_ne!(first.digest(), changed.digest());
        let (attempts, bytes, paths) = identities.work().expect("actual changed file work");
        assert_eq!((attempts, bytes), (4, 4 * size));
        assert_eq!(paths.last(), Some(&root.join("one")));
        println!("actual changed loader work: {attempts} attempts, {bytes} bytes, {paths:?}");
    }

    #[test]
    fn a_known_rustup_selector_reuses_its_actual_toolchain_observation() {
        let directory = tempfile::tempdir().expect("owned observation source and cache");
        let source = directory.path().join("source");
        std::fs::create_dir_all(&source).expect("source directory");
        let mut env: crate::vars::Variables = njutest_devkit::paths::environment_for_a_run()
            .into_iter()
            .collect();
        env.set("RUSTC_WRAPPER", "");
        env.set(
            "NJUTEST_FIXTURE_BUILD_CACHE",
            directory.path().join("cache"),
        );
        let options = LocateOptions {
            cargo: None,
            search_path: env.search_path().map(std::ffi::OsStr::to_os_string),
            env: Some(env),
        };
        let cancel = Cancel::new();
        let first_trace = recorder();
        let first = Toolchain::locate(&options, &source, &Watched::new(&cancel, &first_trace))
            .expect("actual known selector observation");
        assert!(
            first_trace
                .events()
                .iter()
                .any(|event| { matches!(&event.payload, Payload::Exec { .. }) }),
            "the original observation retains actual processes"
        );
        let second_trace = recorder();
        let second = Toolchain::locate(&options, &source, &Watched::new(&cancel, &second_trace))
            .expect("unchanged selector observation");
        assert_eq!(first.cargo_version(), second.cargo_version());
        assert_eq!(first.rustc_version(), second.rustc_version());
        let repeated = second_trace.events();
        let first_work = first
            .identities()
            .work()
            .expect("first actual toolchain reads");
        let second_work = second
            .identities()
            .work()
            .expect("second actual toolchain reads");
        println!("first actual toolchain digest work: {first_work:?}");
        println!("second actual toolchain digest work: {second_work:?}");
        assert!(
            first_work.1 > 0,
            "the original compiler inputs were actually hashed"
        );
        assert_eq!(
            second_work,
            (0, 0, Vec::new()),
            "stable inputs retain actual identities"
        );
        let processes = repeated
            .iter()
            .filter(|event| matches!(&event.payload, Payload::Exec { .. }))
            .count();
        assert_eq!(
            processes, 0,
            "unchanged banners reuse actual provenance: {repeated:?}"
        );
        assert!(repeated.iter().any(|event| {
            matches!(&event.payload, Payload::Note { note } if note.kind == "toolchain-observation-reuse")
        }));
    }
}
