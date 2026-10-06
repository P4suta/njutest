// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Verified content-addressed compilations that answer without starting Cargo.

use std::collections::BTreeMap;
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use self::folding::{Folding, Input};
use super::{
    CAPTURED_COMPILER, COMPILATIONS as DIRECTORY, CompileOptions, Compiled, Completion, Driver,
    Exited, Message, products_name,
};
use crate::vars::Variables;

mod failure;
pub(super) mod folding;
pub(super) mod loaders;
pub(super) mod toolchain;
pub(super) use failure::FailedStage;

const SCHEMA: &str = "rust-mutants-compilation-v3";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct File {
    size: u64,
    digest: String,
    mode: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: String,
    key: String,
    stdout: Vec<u8>,
    stdout_digest: String,
    files: BTreeMap<PathBuf, File>,
    environment: BTreeMap<String, Option<String>>,
    exit: i32,
    generation: String,
    observation: super::CompilerObservation,
}

struct Augmentation {
    role: String,
    arguments: Vec<std::ffi::OsString>,
    paths: Vec<PathBuf>,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct InputPath(PathBuf);

impl InputPath {
    fn of(path: &Path) -> io::Result<Self> {
        std::fs::canonicalize(path).map(Self)
    }
}

struct BoundInputs(BTreeMap<InputPath, File>);

impl BoundInputs {
    fn of(files: BTreeMap<PathBuf, File>) -> io::Result<Self> {
        let mut held = Self(BTreeMap::new());
        for (path, state) in files {
            held.insert(&path, state)?;
        }
        Ok(held)
    }

    fn insert(&mut self, path: &Path, state: File) -> io::Result<()> {
        let identity = InputPath::of(path)?;
        if identity.0 != path && file(&identity.0)? != state {
            return Err(io::Error::other(
                "compiler input changed while binding its identity",
            ));
        }
        let expected = state.clone();
        match self.0.insert(identity, state) {
            Some(previous) if previous != expected => Err(io::Error::other(
                "one compiler input was observed with conflicting content",
            )),
            Some(_) | None => Ok(()),
        }
    }

    fn covers(&self, units: &[super::Unit]) -> io::Result<bool> {
        for path in units.iter().flat_map(|unit| &unit.inputs) {
            if !self.0.contains_key(&InputPath::of(path)?) {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

pub(super) struct Request {
    pub(super) key: String,
    folded: folding::Folded,
    record: PathBuf,
    inputs: BoundInputs,
    target: PathBuf,
    environment: Variables,
    augmentation: Option<Augmentation>,
}

impl Request {
    pub(super) fn of(
        driver: &Driver<'_>,
        options: &CompileOptions,
        env: &mut Variables,
    ) -> io::Result<Self> {
        let root = source_root(driver, options)?;
        let environment = env.clone();
        let mut flags = BTreeMap::new();
        let classified = flag_files(root, options, env, &mut flags);
        if !flags.is_empty() {
            fingerprint_link_inputs(&flags, env)?;
        }
        classified?;
        let mut inputs = BTreeMap::new();
        inputs.extend(flags);
        tree(root, options.target_dir.path(), &mut inputs)?;
        match options.target_dir.cache_roots() {
            Some(roots) => {
                for package in roots.iter().filter(|package| !package.starts_with(root)) {
                    tree(package, options.target_dir.path(), &mut inputs)?;
                }
            }
            None => {
                if inputs
                    .keys()
                    .filter(|path| path.file_name().is_some_and(|name| name == "Cargo.toml"))
                    .any(|path| !plain_manifest(path))
                {
                    return Err(io::Error::other(
                        "an opaque or incomplete compilation graph",
                    ));
                }
            }
        }
        configurations(root, env, &mut inputs)?;
        let loaders = toolchain::inputs((driver.toolchain, root), options, env, &mut inputs)?;
        let inputs = BoundInputs::of(inputs)?;
        let mut folding = Folding::new();
        folding.field(&Input::Derivation, SCHEMA.as_bytes());
        folding.nest(loaders.folded());
        folding.field(
            &Input::Versions,
            format!(
                "{:?}/{:?}",
                driver.toolchain.cargo_version(),
                driver.toolchain.rustc_version()
            )
            .as_bytes(),
        );
        for argument in super::compile_arguments(options) {
            folding.field(&Input::Arguments, argument.as_encoded_bytes());
        }
        folding.field(&Input::Root, root.as_os_str().as_encoded_bytes());
        for (path, state) in &inputs.0 {
            let input = Input::File(path.0.clone());
            folding.field(&input, path.0.as_os_str().as_encoded_bytes());
            folding.field(&input, state.digest.as_bytes());
            folding.field(&input, &state.mode.to_be_bytes());
        }
        for (name, value) in env
            .canonical()
            .into_iter()
            .filter(|(name, _value)| compilation_input(env.spelling(), name))
        {
            let input = Input::Variable(name.clone());
            folding.field(&input, name.as_encoded_bytes());
            folding.field(&input, value.as_encoded_bytes());
        }
        let folded = folding.finish();
        let key = folded.digest().to_owned();
        Ok(Self {
            record: options
                .target_dir
                .path()
                .join(DIRECTORY)
                .join(format!("{}.json", crate::keyed::name(&key)?)),
            key,
            folded,
            inputs,
            target: options.target_dir.path().to_path_buf(),
            environment,
            augmentation: None,
        })
    }

    pub(super) fn read(
        &self,
        driver: &Driver<'_>,
        options: &CompileOptions,
        (env, preparation): (&Variables, &Preparation),
    ) -> io::Result<Compiled> {
        let record: Record = crate::strictjson::decode_slice(&std::fs::read(&self.record)?)
            .map_err(io::Error::other)?;
        if options.target_dir.path() != self.target
            || record.schema != SCHEMA
            || record.key != self.key
            || record.stdout_digest != crate::id::HexDigest::of(&record.stdout).as_str()
            || !record
                .observation
                .verifies(&record.stdout, &self.key, record.exit)
        {
            return Err(io::Error::other("unverified compilation record"));
        }
        let mut messages = super::parse_messages(&record.stdout).map_err(io::Error::other)?;
        let completion =
            Completion::of(&messages, Exited::from_code(record.exit)).map_err(io::Error::other)?;
        if !plain_messages(&messages) {
            return Err(io::Error::other("opaque compilation"));
        }
        if completion == Completion::Refused
            && (!preparation.shares(&record.generation)
                || complete_environment(env)? != record.environment)
        {
            return Err(io::Error::other(
                "a compiler refusal belongs to an earlier request",
            ));
        }
        for (name, value) in &record.environment {
            if &environment_value(env, name) != value {
                return Err(io::Error::other("compiler environment changed"));
            }
        }
        let products = self.products(record.observation.id())?;
        if completion == Completion::Built || !record.files.is_empty() {
            verified_files(&record.files, &products)?;
        }
        self.replay(&mut messages, &record.files, &products)?;
        let units = super::units_of(&messages, driver.dir).map_err(io::Error::other)?;
        if !self.inputs.covers(&units)? {
            return Err(io::Error::other("compiler read outside the bound inputs"));
        }
        let names: std::collections::BTreeSet<&String> =
            units.iter().flat_map(|unit| unit.env.keys()).collect();
        if completion == Completion::Built
            && (names != record.environment.keys().collect()
                || names
                    .iter()
                    .any(|name| !compilation_input(env.spelling(), std::ffi::OsStr::new(name))))
        {
            return Err(io::Error::other(
                "unverified compiler environment dependencies",
            ));
        }
        Ok(Compiled {
            completion,
            observation: record.observation,
            messages,
            units,
            provenance: match completion {
                Completion::Built => super::Provenance::VerifiedReuse,
                Completion::Refused => super::Provenance::SharedRefusal,
            },
        })
    }

    fn replay(
        &self,
        messages: &mut [Message],
        files: &BTreeMap<PathBuf, File>,
        products: &Path,
    ) -> io::Result<()> {
        for message in messages {
            if let Message::CompilerArtifact(artifact) = message {
                for path in artifact
                    .filenames
                    .iter_mut()
                    .chain(&mut artifact.executable)
                {
                    *path = self.product(path, products)?;
                }
                if artifact
                    .filenames
                    .iter()
                    .chain(&artifact.executable)
                    .any(|path| !files.contains_key(path))
                {
                    return Err(io::Error::other("unverified artifact in Cargo messages"));
                }
                for path in artifact.filenames.iter().chain(&artifact.executable) {
                    if let Some(depinfo) = super::dep_info_path(path, &artifact.target.name)
                        && held(&depinfo)?
                        && !files.contains_key(&depinfo)
                    {
                        return Err(io::Error::other("unverified compiler dependency record"));
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn write(
        &self,
        compiled: &mut Compiled,
        (stdout, env, target): (&[u8], &Variables, &Path),
        (exit, preparation): (Exited, &Preparation),
    ) -> io::Result<()> {
        if !plain_messages(&compiled.messages) || !self.inputs.covers(&compiled.units)? {
            return Err(io::Error::other("incomplete or opaque compiler inputs"));
        }
        let names = compiled.units.iter().flat_map(|unit| unit.env.keys());
        if names
            .clone()
            .any(|name| !compilation_input(env.spelling(), std::ffi::OsStr::new(name)))
        {
            return Err(io::Error::other(
                "the compiler reads a volatile diagnostic variable",
            ));
        }
        let environment = match compiled.completion() {
            Completion::Built => names
                .map(|name| (name.clone(), environment_value(env, name)))
                .collect(),
            Completion::Refused => complete_environment(env)?,
        };
        let frozen = self.freeze_products(compiled, target)?;
        let parent = self
            .record
            .parent()
            .ok_or_else(|| io::Error::other("cache record parent"))?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        let generation = temporary
            .path()
            .file_name()
            .ok_or_else(|| io::Error::other("publication identity"))?
            .to_str()
            .ok_or_else(|| io::Error::other("publication identity is not text"))?
            .to_owned();
        let record = Record {
            schema: SCHEMA.to_owned(),
            key: self.key.clone(),
            stdout: stdout.to_vec(),
            stdout_digest: crate::id::HexDigest::of(stdout).as_str().to_owned(),
            files: frozen,
            environment,
            exit: exit.code(),
            generation: generation.clone(),
            observation: compiled.observation().clone(),
        };
        temporary.write_all(&serde_json::to_vec(&record).map_err(io::Error::other)?)?;
        temporary.persist(&self.record).map_err(io::Error::other)?;
        preparation.publish(&generation)?;
        let products = self.products(compiled.observation().id())?;
        for message in &mut compiled.messages {
            if let Message::CompilerArtifact(artifact) = message {
                for path in artifact
                    .filenames
                    .iter_mut()
                    .chain(&mut artifact.executable)
                {
                    *path = self.product(path, &products)?;
                }
            }
        }
        Ok(())
    }

    fn freeze_products(
        &self,
        compiled: &Compiled,
        target: &Path,
    ) -> io::Result<BTreeMap<PathBuf, File>> {
        let mut files = BTreeMap::new();
        for message in &compiled.messages {
            if let Message::CompilerArtifact(artifact) = message {
                for path in artifact.filenames.iter().chain(&artifact.executable) {
                    if !bound_artifact(path, target) {
                        return Err(io::Error::other(
                            "artifact outside the owned build directory",
                        ));
                    }
                    files.insert(path.clone(), file(path)?);
                    if let Some(depinfo) = super::dep_info_path(path, &artifact.target.name)
                        && held(&depinfo)?
                    {
                        files.insert(depinfo.clone(), file(&depinfo)?);
                    }
                }
            }
        }
        let parent = self
            .record
            .parent()
            .ok_or_else(|| io::Error::other("cache record parent"))?;
        std::fs::create_dir_all(parent)?;
        let staging = tempfile::Builder::new()
            .prefix("products-")
            .tempdir_in(parent)?;
        let mut frozen = BTreeMap::new();
        let products = self.products(compiled.observation().id())?;
        for (path, state) in files {
            let relative = product_relative(&path, target)?;
            let copied = staging.path().join(relative);
            let destination = copied
                .parent()
                .ok_or_else(|| io::Error::other("product parent"))?;
            std::fs::create_dir_all(destination)?;
            std::fs::copy(&path, &copied)?;
            if file(&path)? != state || file(&copied)? != state {
                return Err(io::Error::other(
                    "compiler products changed during publication",
                ));
            }
            frozen.insert(self.product(&path, &products)?, state);
        }
        if held(&products)? {
            if !frozen.is_empty() {
                verified_files(&frozen, &products)?;
            }
        } else {
            std::fs::rename(staging.path(), &products)?;
        }
        Ok(frozen)
    }

    pub(super) fn unchanged(
        &self,
        driver: &Driver<'_>,
        options: &CompileOptions,
    ) -> io::Result<()> {
        let mut env = self.environment.clone();
        let mut current = Self::of(driver, options, &mut env)?;
        if let Some(augmentation) = &self.augmentation {
            current = current.augment(
                &augmentation.role,
                &augmentation.arguments,
                &augmentation.paths,
            )?;
        }
        match folding::InputsChangedError::between(&self.folded, &current.folded) {
            Some(changed) => Err(io::Error::other(changed)),
            None => Ok(()),
        }
    }

    pub(super) fn augment(
        mut self,
        role: &str,
        arguments: &[std::ffi::OsString],
        paths: &[PathBuf],
    ) -> io::Result<Self> {
        let mut folding = Folding::new();
        folding.nest(&self.folded);
        folding.field(&Input::Augmentation, role.as_bytes());
        for argument in arguments {
            folding.field(&Input::Augmentation, argument.as_encoded_bytes());
        }
        for path in paths {
            let state = file(path)?;
            let input = Input::File(path.clone());
            folding.field(&input, path.as_os_str().as_encoded_bytes());
            folding.field(&input, state.digest.as_bytes());
            folding.field(&input, &state.mode.to_be_bytes());
            self.inputs.insert(path, state)?;
        }
        for (name, value) in self.environment.canonical() {
            let input = Input::Variable(name.clone());
            folding.field(&input, name.as_encoded_bytes());
            folding.field(&input, value.as_encoded_bytes());
        }
        self.folded = folding.finish();
        self.key = self.folded.digest().to_owned();
        self.record
            .set_file_name(format!("{}.json", crate::keyed::name(&self.key)?));
        self.augmentation = Some(Augmentation {
            role: role.to_owned(),
            arguments: arguments.to_vec(),
            paths: paths.to_vec(),
        });
        Ok(self)
    }

    pub(super) fn covers(&self, units: &[super::Unit]) -> io::Result<bool> {
        self.inputs.covers(units)
    }

    pub(super) fn capture_files(
        &self,
        messages: &mut [Message],
        directory: &Path,
    ) -> io::Result<()> {
        if !plain_messages(messages) {
            return Err(io::Error::other("opaque doctest compiler messages"));
        }
        for message in messages {
            if let Message::CompilerArtifact(artifact) = message {
                for path in artifact
                    .filenames
                    .iter_mut()
                    .chain(&mut artifact.executable)
                {
                    let copied = directory
                        .join(CAPTURED_COMPILER)
                        .join(product_relative(path, &self.target)?);
                    let parent = copied
                        .parent()
                        .ok_or_else(|| io::Error::other("compiler inventory parent"))?;
                    std::fs::create_dir_all(parent)?;
                    let original = file(path)?;
                    std::fs::copy(&*path, &copied)?;
                    if file(path)? != original || file(&copied)? != original {
                        return Err(io::Error::other("doctest compiler products changed"));
                    }
                    if let Some(depinfo) = super::dep_info_path(path, &artifact.target.name)
                        && held(&depinfo)?
                    {
                        let kept = directory
                            .join(CAPTURED_COMPILER)
                            .join(product_relative(&depinfo, &self.target)?);
                        std::fs::copy(&depinfo, &kept)?;
                        if file(&depinfo)? != file(&kept)? {
                            return Err(io::Error::other("doctest dependency products changed"));
                        }
                    }
                    *path = copied;
                }
            }
        }
        Ok(())
    }

    pub(super) fn capture_messages(
        &self,
        messages: &mut [Message],
        directory: &Path,
    ) -> io::Result<()> {
        for message in messages {
            if let Message::CompilerArtifact(artifact) = message {
                for path in artifact
                    .filenames
                    .iter_mut()
                    .chain(&mut artifact.executable)
                {
                    *path = directory
                        .join(CAPTURED_COMPILER)
                        .join(product_relative(path, &self.target)?);
                }
            }
        }
        Ok(())
    }

    pub(super) fn sources(&self) -> impl Iterator<Item = &Path> {
        self.inputs
            .0
            .keys()
            .map(|path| path.0.as_path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
    }

    pub(super) fn independent(&mut self, observation: &str) -> io::Result<()> {
        self.record.set_file_name(format!(
            "{}.witness-{}.json",
            crate::keyed::name(&self.key)?,
            crate::keyed::name(observation)?
        ));
        Ok(())
    }

    fn products(&self, observation: &str) -> io::Result<PathBuf> {
        Ok(self
            .record
            .with_file_name(products_name(&self.key, observation)?))
    }

    fn product(&self, original: &Path, products: &Path) -> io::Result<PathBuf> {
        if !bound_artifact(original, &self.target) {
            return Err(io::Error::other("unbound compiler product"));
        }
        Ok(products.join(product_relative(original, &self.target)?))
    }
}

fn source_root<'a>(driver: &Driver<'_>, options: &'a CompileOptions) -> io::Result<&'a Path> {
    let root = options
        .target_dir
        .root()
        .ok_or_else(|| io::Error::other("unbound source tree"))?;
    if root != driver.dir
        || !options.locked
        || !std::fs::metadata(root.join("Cargo.lock"))?.is_file()
    {
        return Err(io::Error::other("unlocked or unbound build inputs"));
    }
    Ok(root)
}

fn complete_environment(env: &Variables) -> io::Result<BTreeMap<String, Option<String>>> {
    env.canonical()
        .into_iter()
        .map(|(name, value)| {
            let name = name
                .to_str()
                .ok_or_else(|| io::Error::other("non-textual compiler environment identity"))?;
            Ok((
                name.to_owned(),
                Some(
                    crate::id::HexDigest::of(value.as_encoded_bytes())
                        .as_str()
                        .to_owned(),
                ),
            ))
        })
        .collect()
}

pub(super) struct Preparation {
    _lease: std::fs::File,
    publication: PathBuf,
    prior: Option<Vec<u8>>,
    current: Option<Vec<u8>>,
}

impl Preparation {
    pub(super) fn own(target: &Path, trace: &crate::trace::Recorder) -> io::Result<Self> {
        std::fs::create_dir_all(target)?;
        let publication = target.join("rust-mutants-preparation-publication");
        let read = || match std::fs::read(&publication) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(source),
        };
        let prior = read()?;
        let lease = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(target.join("rust-mutants-preparation.lock"))?;
        let started = std::time::Instant::now();
        let result = lease.lock();
        let elapsed_ns = u64::try_from(started.elapsed().as_nanos()).map_err(io::Error::other)?;
        let cpus = std::thread::available_parallelism()?.get();
        trace.note(
            "host-wait",
            &serde_json::json!({
                "owner": target.display().to_string(), "cause": "compiler preparation lease",
                "elapsed_ns": elapsed_ns,
                "machine": {"os": std::env::consts::OS, "cpus": cpus}
            })
            .to_string(),
        );
        result?;
        let current = read()?;
        Ok(Self {
            _lease: lease,
            publication,
            prior,
            current,
        })
    }

    fn shares(&self, generation: &str) -> bool {
        self.prior != self.current && self.current.as_deref() == Some(generation.as_bytes())
    }

    pub(super) fn publish(&self, generation: &str) -> io::Result<()> {
        let parent = self
            .publication
            .parent()
            .ok_or_else(|| io::Error::other("publication parent"))?;
        let mut staged = tempfile::NamedTempFile::new_in(parent)?;
        staged.write_all(generation.as_bytes())?;
        staged
            .persist(&self.publication)
            .map_err(io::Error::other)?;
        Ok(())
    }
}

pub(super) fn file(path: &Path) -> io::Result<File> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() {
        return Err(io::Error::other(
            "a build input or artifact is not a regular file",
        ));
    }
    let mut digest = Sha256::new();
    let mut input = std::fs::File::open(path)?;
    let mut buffer = vec![0_u8; 65536];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(
            buffer
                .get(..count)
                .ok_or_else(|| io::Error::other("input buffer count"))?,
        );
    }
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt as _;
        metadata.permissions().mode()
    };
    #[cfg(windows)]
    let mode = u32::from(metadata.permissions().readonly());
    Ok(File {
        size: metadata.len(),
        digest: hex::encode(digest.finalize()),
        mode,
    })
}

pub(super) fn tree(
    root: &Path,
    target: &Path,
    files: &mut BTreeMap<PathBuf, File>,
) -> io::Result<()> {
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if !target.as_os_str().is_empty() && path.starts_with(target) {
            continue;
        }
        if entry.file_type()?.is_dir() {
            tree(&path, target, files)?;
        } else {
            files.insert(path.clone(), file(&path)?);
        }
    }
    Ok(())
}

fn product_relative(path: &Path, target: &Path) -> io::Result<PathBuf> {
    let path = njutest_fixture_tree::filesystem_spelling(path);
    let target = njutest_fixture_tree::filesystem_spelling(target);
    let relative = path.strip_prefix(target).map_err(io::Error::other)?;
    if relative
        .components()
        .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(io::Error::other(
            "a compiler product has an unbound relative path",
        ));
    }
    Ok(relative.to_path_buf())
}

pub(super) fn plain_manifest(path: &Path) -> bool {
    let Ok(source) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(document) = source.parse::<toml::Table>() else {
        return false;
    };
    matches!(std::fs::symlink_metadata(path.with_file_name("build.rs")), Err(source) if source.kind() == io::ErrorKind::NotFound)
        && document
            .get("package")
            .and_then(|package| package.get("build"))
            .is_none()
        && document
            .get("lib")
            .and_then(|lib| lib.get("proc-macro"))
            .and_then(toml::Value::as_bool)
            != Some(true)
        && !document.iter().any(|(name, value)| {
            (name.ends_with("dependencies")
                && value.as_table().is_some_and(|table| !table.is_empty()))
                || dependencies(value)
        })
}

fn dependencies(value: &toml::Value) -> bool {
    match value {
        toml::Value::Table(table) => table.iter().any(|(name, value)| {
            (name.ends_with("dependencies")
                && value.as_table().is_some_and(|table| !table.is_empty()))
                || dependencies(value)
        }),
        toml::Value::Array(values) => values.iter().any(dependencies),
        toml::Value::String(_)
        | toml::Value::Integer(_)
        | toml::Value::Float(_)
        | toml::Value::Boolean(_)
        | toml::Value::Datetime(_) => false,
    }
}

pub(super) fn configurations(
    root: &Path,
    env: &Variables,
    files: &mut BTreeMap<PathBuf, File>,
) -> io::Result<()> {
    let home = super::config::home(env).ok_or_else(|| io::Error::other("unknown Cargo home"))?;
    for directory in root
        .ancestors()
        .map(|directory| directory.join(".cargo"))
        .chain(std::iter::once(home))
    {
        for name in ["config", "config.toml"] {
            let path = directory.join(name);
            match std::fs::read_to_string(&path) {
                Ok(source) => {
                    let document = source.parse::<toml::Table>().map_err(io::Error::other)?;
                    if opaque_configuration(&document) {
                        return Err(io::Error::other("opaque Cargo configuration"));
                    }
                    files.insert(path.clone(), file(&path)?);
                }
                Err(source) if source.kind() == io::ErrorKind::NotFound => {}
                Err(source) => return Err(source),
            }
        }
    }
    Ok(())
}

/// The flag variables the compiler reads, each with whether cargo splits its value into arguments on the unit separator rather than on whitespace.
const FLAG_VARIABLES: [(&str, bool); 4] = [
    ("RUSTFLAGS", false),
    ("RUSTDOCFLAGS", false),
    ("CARGO_ENCODED_RUSTFLAGS", true),
    ("CARGO_ENCODED_RUSTDOCFLAGS", true),
];

/// Reads every flag variable under the argument protocol cargo actually splits it by, and binds what the sealed target's own build links with: an engine-owned switch is admitted, a file is admitted by its content, and every other external compiler or linker input is refused for a real Cargo to answer.
fn flag_files(
    dir: &Path,
    options: &CompileOptions,
    env: &Variables,
    files: &mut BTreeMap<PathBuf, File>,
) -> io::Result<()> {
    if let Some(target) = options.build.target.as_ref().filter(|target| {
        Path::new(target)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
    }) {
        let path = PathBuf::from(target);
        files.insert(path.clone(), file(&path)?);
    }
    let sealed = options.build.target.as_deref() == Some(crate::sealed::TARGET);
    for (name, encoded) in FLAG_VARIABLES {
        let Some(flags) = env.var(name) else {
            continue;
        };
        let flags = flags
            .to_str()
            .ok_or_else(|| io::Error::other("non-textual compiler flags"))?;
        let tokens: Vec<&str> = if encoded {
            flags.split(super::config::SEPARATOR).collect()
        } else {
            flags.split_whitespace().collect()
        };
        let mut tokens = tokens.into_iter();
        while let Some(token) = tokens.next() {
            if token.is_empty() {
                continue;
            }
            let option = if token == "-C" {
                Some(
                    tokens
                        .next()
                        .ok_or_else(|| io::Error::other("an incomplete -C compiler flag"))?,
                )
            } else {
                token.strip_prefix("-C")
            };
            if let Some(option) = option {
                if option.is_empty() {
                    return Err(io::Error::other("an incomplete -C compiler flag"));
                }
                if option == "link-arg" {
                    return Err(io::Error::other(
                        "a link-arg compiler flag the compiler accepts no separate value for",
                    ));
                }
                if let Some(value) = option.strip_prefix("link-arg=") {
                    linked(dir, sealed, value, files)?;
                } else if !scalar_codegen(option) {
                    return Err(io::Error::other(format!(
                        "an unsupported or external compiler codegen option: {option}"
                    )));
                }
                continue;
            }
            if external(token) {
                return Err(io::Error::other("external compiler or linker inputs"));
            }
            if token == "--cfg" {
                let value = tokens
                    .next()
                    .filter(|value| !value.is_empty() && !value.starts_with('-'))
                    .ok_or_else(|| io::Error::other("an incomplete --cfg compiler flag"))?;
                if value.starts_with('@') {
                    return Err(io::Error::other("an unsupported --cfg compiler value"));
                }
            } else if !token.starts_with("--cfg=") && token != "--cap-lints=warn" {
                return Err(io::Error::other(format!(
                    "an unsupported compiler option: {token}"
                )));
            }
        }
    }
    Ok(())
}

/// The established scalar codegen options, whose arguments read no external file or program.
fn scalar_codegen(option: &str) -> bool {
    matches!(
        option,
        "opt-level=0"
            | "opt-level=1"
            | "opt-level=2"
            | "opt-level=3"
            | "opt-level=s"
            | "opt-level=z"
            | "strip=none"
            | "strip=debuginfo"
            | "strip=symbols"
            | "debuginfo=0"
            | "debuginfo=1"
            | "debuginfo=2"
            | "instrument-coverage"
            | "link-dead-code"
    )
}

/// Makes Cargo's own freshness depend on the bound inputs even when a foreign object's path stays.
fn fingerprint_link_inputs(
    inputs: &BTreeMap<PathBuf, File>,
    env: &mut Variables,
) -> io::Result<()> {
    let Some(mut flags) = super::config::inherited_as(
        env,
        (super::config::ENCODED_RUSTFLAGS, super::config::RUSTFLAGS),
    )
    .map_err(io::Error::other)?
    else {
        return Ok(());
    };
    let mut digest = Sha256::new();
    let mut states: Vec<_> = inputs.values().collect();
    states.sort_by_key(|state| (&state.digest, state.mode));
    for state in states {
        field(&mut digest, state.digest.as_bytes());
        field(&mut digest, &state.mode.to_be_bytes());
    }
    flags.push(format!(
        "--cfg=rust_mutants_link_inputs=\"{}\"",
        hex::encode(digest.finalize())
    ));
    env.set(
        super::config::ENCODED_RUSTFLAGS,
        flags.join(&super::config::SEPARATOR.to_string()),
    );
    Ok(())
}

/// Whether a file is there to be read, with an unreadable one refused rather than answered as absent.
fn held(path: &Path) -> io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Whether one argument names an input outside what this record binds.
fn external(token: &str) -> bool {
    token.starts_with('@')
        || token.starts_with("--extern")
        || token.starts_with("--sysroot")
        || token.starts_with("-L")
        || token.starts_with("-l")
        || token.contains("link-arg")
        || token.contains("linker=")
        || token.contains("codegen-backend")
}

/// Binds one linker argument: the engine's own sealed switch is admitted, a self-contained file the sealed target links with is admitted by its content, and every other case is refused, because a native platform's linker is not a toolchain input the record binds.
fn linked(
    dir: &Path,
    sealed: bool,
    value: &str,
    files: &mut BTreeMap<PathBuf, File>,
) -> io::Result<()> {
    if !sealed {
        return Err(io::Error::other(
            "a link argument to a native linker the record does not bind",
        ));
    }
    if value.starts_with('@') {
        return Err(io::Error::other(
            "a linker response file names arguments the record would not bind",
        ));
    }
    if engine_linked(value) {
        return Ok(());
    }
    if value.starts_with('-') {
        return Err(io::Error::other("an unsupported linker switch"));
    }
    let path = dir.join(value);
    let bound = file(&path).map_err(|source| link_input_refused(&path, source))?;
    files.insert(path.clone(), bound);
    indirect(&path)?;
    Ok(())
}

/// A refused compiler input with its exact identity and underlying typed cause.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
#[error("{code}: the link input {} could not be bound ({:?}, {})", path.display(), source.kind(), os_code(source), code = LinkInputError::code().code)]
struct LinkInputError {
    path: PathBuf,
    source: io::Error,
}

/// Preserves the I/O kind and attaches the input's identity without using localized prose.
impl LinkInputError {
    const fn code() -> crate::error::ErrorCode {
        crate::error::COMPILER_INPUT_UNREADABLE
    }
}

fn link_input_refused(path: &Path, source: io::Error) -> io::Error {
    io::Error::new(
        source.kind(),
        LinkInputError {
            path: path.to_path_buf(),
            source,
        },
    )
}

/// What a refusal says of the operating system's answer: its code, which unlike its message is never translated, or that it gave none.
pub(super) fn os_code(source: &io::Error) -> String {
    match source.raw_os_error() {
        Some(code) => format!("os error {code}"),
        None => "no operating system code".to_owned(),
    }
}

/// Refuses every format and metadata variant outside the engine's self-contained WebAssembly object.
fn indirect(path: &Path) -> io::Result<()> {
    let bytes = std::fs::read(path).map_err(|source| link_input_refused(path, source))?;
    if bytes.starts_with(b"!<") {
        return Err(io::Error::other(
            "an archive link input whose members the record does not bind",
        ));
    }
    if !bytes.starts_with(b"\0asm\x01\0\0\0")
        || path.file_name() != Some(std::ffi::OsStr::new("platform.o"))
    {
        return Err(io::Error::other(
            "a link input is not the self-contained WebAssembly platform object",
        ));
    }
    wasmparser::Validator::new()
        .validate_all(&bytes)
        .map_err(io::Error::other)?;
    let mut linking = false;
    for payload in wasmparser::Parser::new(0).parse_all(&bytes) {
        if let wasmparser::Payload::CustomSection(section) = payload.map_err(io::Error::other)? {
            match section.name() {
                "linking" if !linking => {
                    let wasmparser::KnownCustom::Linking(metadata) = section.as_known() else {
                        return Err(io::Error::other("unsupported WebAssembly linking metadata"));
                    };
                    for subsection in metadata {
                        if let wasmparser::Linking::Unknown { .. } =
                            subsection.map_err(io::Error::other)?
                        {
                            return Err(io::Error::other(
                                "unsupported WebAssembly linking metadata",
                            ));
                        }
                    }
                    linking = true;
                }
                "reloc.CODE" | "reloc.DATA" | "producers" | "target_features" => {}
                name => {
                    return Err(io::Error::other(format!(
                        "unsupported WebAssembly link metadata: {name}"
                    )));
                }
            }
        }
    }
    if !linking {
        return Err(io::Error::other(
            "missing WebAssembly object linking metadata",
        ));
    }
    Ok(())
}

/// Whether `value` is one of the switches the engine itself passes the sealed target's linker.
fn engine_linked(value: &str) -> bool {
    value == format!("--export={}", crate::sealed::platform::TEMP_DIR)
        || value == format!("--export={}", crate::sealed::platform::HOME_DIR)
        || rust_mutants_sealed::START_LINK_ARGS.contains(&value)
}

/// Whether a variable can be an input of a build: every one but the labels this product and its test harness put on their own work, and the scratch and cache places a build's identity leaves out.
pub(super) fn compilation_input(spelling: crate::vars::Spelling, name: &std::ffi::OsStr) -> bool {
    !["NEXTEST_", "NJUTEST_"]
        .iter()
        .any(|prefix| spelling.begins(name, prefix))
        && !["TMPDIR", "TMP", "TEMP", "XDG_CACHE_HOME"]
            .iter()
            .any(|variable| spelling.same(name, std::ffi::OsStr::new(variable)))
}

fn environment_value(env: &Variables, name: &str) -> Option<String> {
    env.var(name).map(|value| {
        crate::id::HexDigest::of(value.as_encoded_bytes())
            .as_str()
            .to_owned()
    })
}

fn plain_messages(messages: &[Message]) -> bool {
    messages.iter().all(|message| match message {
        Message::CompilerArtifact(artifact) => !artifact
            .target
            .kind
            .iter()
            .any(|kind| kind == "custom-build" || kind == "proc-macro"),
        Message::CompilerMessage(_) | Message::BuildFinished(_) => true,
        Message::BuildScriptExecuted(_) | Message::Other { .. } => false,
    })
}

fn field(digest: &mut Sha256, bytes: &[u8]) {
    digest.update(bytes.len().to_string().as_bytes());
    digest.update(b"\0");
    digest.update(bytes);
}

fn opaque_configuration(table: &toml::Table) -> bool {
    table.iter().any(|(name, value)| {
        matches!(
            name.as_str(),
            "include"
                | "env"
                | "rustc"
                | "rustc-wrapper"
                | "rustc-workspace-wrapper"
                | "linker"
                | "runner"
                | "rustflags"
                | "rustdocflags"
        ) || match value {
            toml::Value::Table(nested) => opaque_configuration(nested),
            toml::Value::Array(_)
            | toml::Value::String(_)
            | toml::Value::Integer(_)
            | toml::Value::Float(_)
            | toml::Value::Boolean(_)
            | toml::Value::Datetime(_) => false,
        }
    })
}

fn bound_artifact(path: &Path, target: &Path) -> bool {
    if !path.starts_with(target)
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return false;
    }
    match (
        path.parent().map(std::fs::canonicalize),
        std::fs::canonicalize(target),
    ) {
        (Some(Ok(parent)), Ok(target)) => parent.starts_with(target),
        (Some(Err(_)) | None, _) | (_, Err(_)) => false,
    }
}

/// Holds the record's products to what the tree holds now, refusing the reuse without touching anything the tree or the target directory keeps: a file the record verified has changed is a state a later process made, and removing it would hand a later reader bytes no compiler and no alteration produced, so cargo itself is asked and judges what it built by its own record of what it read.
fn verified_files(files: &BTreeMap<PathBuf, File>, target: &Path) -> io::Result<()> {
    if files.is_empty() || files.keys().any(|path| !bound_artifact(path, target)) {
        return Err(io::Error::other("unbound compilation artifacts"));
    }
    for (path, recorded) in files {
        match file(path) {
            Ok(current) if current == *recorded => {}
            Ok(_) | Err(_) => {
                return Err(io::Error::other("compilation artifact digest changed"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CompileOptions, flag_files};
    use std::collections::BTreeMap;
    use std::io;
    use std::path::{Path, PathBuf};

    /// The files `name`'s flags bind, built for `target` from a root `dir` holds.
    fn bound(
        dir: &Path,
        target: Option<&str>,
        name: &str,
        flags: &str,
    ) -> Result<BTreeMap<PathBuf, super::File>, io::Error> {
        let mut options =
            CompileOptions::new(super::super::BuildDir::new(dir.join("target"), Vec::new()));
        options.build.target = target.map(str::to_owned);
        options.env.set(name, flags);
        let mut files = BTreeMap::new();
        flag_files(dir, &options, &options.env, &mut files).map(|()| files)
    }

    #[test]
    fn encoded_link_arguments_keep_one_whole_file_even_with_spaces() {
        let directory = tempfile::tempdir().expect("an owned root");
        let object = directory.path().join("plat form.o");
        std::fs::write(&object, b"object bytes").expect("a linker input");
        let flags = format!(
            "-Copt-level=0\u{1f}-Cstrip=none\u{1f}-Clink-arg={}",
            object.display()
        );
        let refused = bound(
            directory.path(),
            Some(crate::sealed::TARGET),
            "CARGO_ENCODED_RUSTFLAGS",
            &flags,
        )
        .expect_err("the existing file is found whole, then refused for its unproven format");
        assert!(
            refused.to_string().contains("self-contained"),
            "encoded whitespace stays inside the found filename: {refused}"
        );
    }

    #[test]
    fn plain_link_arguments_split_on_whitespace_so_a_spaced_path_is_refused() {
        let directory = tempfile::tempdir().expect("an owned root");
        std::fs::create_dir_all(directory.path().join("with space"))
            .expect("a directory whose name holds one");
        let object = directory.path().join("with space").join("platform.o");
        std::fs::write(&object, b"object bytes").expect("a linker input");
        let flags = format!("-Clink-arg={}", object.display());
        let refused = bound(
            directory.path(),
            Some(crate::sealed::TARGET),
            "RUSTFLAGS",
            &flags,
        )
        .expect_err("the plain protocol splits a path with spaces into arguments rustc refuses");
        assert_refused_input(
            &refused,
            &directory.path().join("with"),
            io::ErrorKind::NotFound,
        );
    }

    #[test]
    fn a_self_contained_webassembly_platform_object_is_bound_by_its_content() {
        let directory = tempfile::tempdir().expect("an owned input root");
        let path = directory.path().join("platform.o");
        let module = [
            b"\0asm\x01\0\0\0".as_slice(),
            b"\x00\x09\x07linking\x02".as_slice(),
        ]
        .concat();
        std::fs::write(&path, module).expect("a valid core module with linking metadata");
        let files = bound(
            directory.path(),
            Some(crate::sealed::TARGET),
            "CARGO_ENCODED_RUSTFLAGS",
            "-Clink-arg=platform.o",
        )
        .unwrap_or_else(|error| panic!("the engine's own platform object is admitted: {error}"));
        assert_eq!(
            files.keys().collect::<Vec<_>>(),
            [&path],
            "the object is bound by its content, which is the positive control for every \
             refusal the format checks make"
        );
    }

    #[test]
    fn every_form_the_compiler_accepts_of_the_engine_owned_switches_is_admitted() {
        let directory = tempfile::tempdir().expect("an owned root");
        for flags in [
            "-Copt-level=0",
            "-C\u{1f}strip=none",
            "--cap-lints=warn",
            "-Clink-arg=--export=chdir",
            "-C\u{1f}link-arg=--export=malloc",
            "-Clink-arg=--undefined=chdir",
            "-Clink-arg=--export=rust_mutants_sealed_temp_dir",
            "-Clink-arg=--export=rust_mutants_sealed_home_dir",
        ] {
            let files = bound(
                directory.path(),
                Some(crate::sealed::TARGET),
                "CARGO_ENCODED_RUSTFLAGS",
                flags,
            )
            .unwrap_or_else(|error| panic!("{flags} is the engine's own switch: {error}"));
            assert!(files.is_empty(), "a switch binds no file: {files:?}");
        }
    }

    #[test]
    fn a_separate_link_arg_value_is_refused_because_the_compiler_accepts_no_such_form() {
        let directory = tempfile::tempdir().expect("an owned root");
        for flags in [
            "-Clink-arg\u{1f}--export=malloc",
            "-C\u{1f}link-arg\u{1f}--export=malloc",
            "-Clink-arg",
            "-C\u{1f}link-arg",
        ] {
            let refused = bound(
                directory.path(),
                Some(crate::sealed::TARGET),
                "CARGO_ENCODED_RUSTFLAGS",
                flags,
            )
            .expect_err(&format!(
                "{flags:?} the pinned compiler rejects a separate value"
            ));
            assert!(
                refused.to_string().contains("no separate value"),
                "the refusal names the form the compiler rejects: {flags:?} -> {refused}"
            );
        }
    }

    #[test]
    fn c_options_that_carry_link_inputs_under_unknown_names_are_refused() {
        let directory = tempfile::tempdir().expect("an owned root");
        for flags in [
            "-Clink-args=--export=chdir",
            "-C\u{1f}link-args=--export=malloc",
            "-Clinker-plugin=/lib/lld.so",
            "-C\u{1f}linker-flavor=lld",
            "-Cprofile-use=/tmp/profile.profdata",
            "-C\u{1f}profile-use=/tmp/profile.profdata",
            "-Cllvm-args=-load=/tmp/plugin.so",
            "-C\u{1f}llvm-args=-load=/tmp/plugin.so",
        ] {
            let refused = bound(
                directory.path(),
                Some(crate::sealed::TARGET),
                "CARGO_ENCODED_RUSTFLAGS",
                flags,
            )
            .expect_err(&format!(
                "{flags} names a link input under a name the record does not bind"
            ));
            assert!(
                refused.to_string().contains("external"),
                "the refusal names its class: {flags} -> {refused}"
            );
        }
    }

    #[test]
    fn a_regular_file_is_not_proof_of_a_self_contained_wasm_link_input() {
        let directory = tempfile::tempdir().expect("an owned input root");
        let path = directory.path().join("platform.o");
        std::fs::write(&path, b"INPUT(other.o)\n").expect("a file-bearing linker script");
        let refused = bound(
            directory.path(),
            Some(crate::sealed::TARGET),
            "CARGO_ENCODED_RUSTFLAGS",
            "-Clink-arg=platform.o",
        )
        .expect_err("regular file bytes do not prove the linker's full input closure");
        assert!(
            refused.to_string().contains("self-contained"),
            "the refusal names the unsupported format: {refused}"
        );
    }

    #[test]
    fn wasm_headers_do_not_admit_dynamic_or_unknown_link_metadata() {
        let directory = tempfile::tempdir().expect("an owned input root");
        let path = directory.path().join("platform.o");
        for bytes in [
            b"\0asm\x01\0\0\0\0\x17\x08dylink.0\x02\x0c\x01\x0aoutside.so".as_slice(),
            b"\0asm\x01\0\0\0\0\x0b\x07linking\x02\x09\0".as_slice(),
        ] {
            std::fs::write(&path, bytes).expect("a core module with valid custom-section framing");
            let refused = bound(
                directory.path(),
                Some(crate::sealed::TARGET),
                "CARGO_ENCODED_RUSTFLAGS",
                "-Clink-arg=platform.o",
            )
            .expect_err("a wasm header cannot certify dynamic dependencies or unknown metadata");
            assert!(
                refused.to_string().contains("metadata"),
                "the boundary cause is explicit: {refused}"
            );
        }
    }

    #[test]
    fn a_response_file_link_argument_is_refused_with_both_files_present() {
        let directory = tempfile::tempdir().expect("an owned root");
        std::fs::write(directory.path().join("response"), "--export=malloc\n")
            .expect("the file the linker would read");
        std::fs::write(directory.path().join("@response"), "--export=malloc\n")
            .expect("a file named exactly as the argument");
        let refused = bound(
            directory.path(),
            Some(crate::sealed::TARGET),
            "CARGO_ENCODED_RUSTFLAGS",
            "-Clink-arg=@response",
        )
        .expect_err("a response file's arguments are inputs content alone does not bind");
        assert!(
            refused.to_string().contains("response file"),
            "the refusal names the indirection, not a missing file: {refused}"
        );
    }

    #[test]
    fn an_archive_link_argument_is_refused_because_its_members_are_not_its_bytes() {
        let directory = tempfile::tempdir().expect("an owned root");
        std::fs::write(
            directory.path().join("thin.a"),
            b"!<thin>\nmember.o/           0           0     0     644     4         `\n<><>\n",
        )
        .expect("a thin archive naming a member beside it");
        std::fs::write(directory.path().join("member.o"), b"member bytes")
            .expect("the file the thin archive names");
        std::fs::write(
            directory.path().join("fat.a"),
            b"!<arch>\nmember.o/           0           0     0     644     12        `\nmember bytes\n",
        )
        .expect("an archive");
        for name in ["thin.a", "fat.a"] {
            let refused = bound(
                directory.path(),
                Some(crate::sealed::TARGET),
                "CARGO_ENCODED_RUSTFLAGS",
                &format!("-Clink-arg={name}"),
            )
            .expect_err("an archive's link effect is not established by its own bytes alone");
            assert!(
                refused.to_string().contains("archive"),
                "the refusal names the archive class: {name} -> {refused}"
            );
        }
    }

    #[test]
    fn the_engine_owned_link_switches_stay_refused_for_a_native_target() {
        let directory = tempfile::tempdir().expect("an owned root");
        for target in [None, Some("x86_64-apple-darwin")] {
            let refused = bound(
                directory.path(),
                target,
                "CARGO_ENCODED_RUSTFLAGS",
                "-Clink-arg=--export=chdir",
            )
            .expect_err("the native platform's linker is not a toolchain input");
            assert!(
                refused.to_string().contains("native linker"),
                "the refusal names the native linker class: {refused}"
            );
        }
    }

    #[test]
    fn unproven_link_file_formats_stay_refused_for_every_target() {
        let directory = tempfile::tempdir().expect("an owned root");
        let object = directory.path().join("platform.o");
        std::fs::write(&object, b"object bytes").expect("a linker input");
        let flags = format!("-Clink-arg={}", object.display());
        let refused = bound(
            directory.path(),
            Some(crate::sealed::TARGET),
            "CARGO_ENCODED_RUSTFLAGS",
            &flags,
        )
        .expect_err("the file is not a self-contained WebAssembly object");
        assert!(refused.to_string().contains("self-contained"), "{refused}");
        let refused = bound(
            directory.path(),
            Some("x86_64-apple-darwin"),
            "CARGO_ENCODED_RUSTFLAGS",
            &flags,
        )
        .expect_err("a native link argument reaches a linker the record does not bind");
        assert!(
            refused.to_string().contains("native linker"),
            "the refusal names the native linker class: {refused}"
        );
        let refused = bound(
            directory.path(),
            Some(crate::sealed::TARGET),
            "CARGO_ENCODED_RUSTFLAGS",
            "-Clink-arg=/no/such/object.o",
        )
        .expect_err("a file that is not there cannot be bound");
        let rooted: PathBuf = directory
            .path()
            .components()
            .filter(|part| matches!(part, std::path::Component::Prefix(_)))
            .chain(Path::new("/no/such/object.o").components())
            .collect();
        assert_refused_input(&refused, &rooted, io::ErrorKind::NotFound);
    }

    fn assert_refused_input(refused: &io::Error, path: &Path, kind: io::ErrorKind) {
        assert_eq!(refused.kind(), kind);
        let input = refused
            .get_ref()
            .and_then(|source| source.downcast_ref::<super::LinkInputError>())
            .expect("a refusal exposes the named input and typed cause");
        assert_eq!(input.path, path);
        assert_eq!(input.source.kind(), kind);
        assert!(
            refused
                .to_string()
                .contains(&input.path.display().to_string())
        );
    }

    #[test]
    fn localized_input_failures_keep_their_kind_identity_and_original_source() {
        for kind in [
            io::ErrorKind::NotFound,
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::Other,
        ] {
            let path = Path::new("/compiler/input.o");
            let source = io::Error::new(kind, "指定されたファイルを読み取れません。");
            let refused = super::link_input_refused(path, source);
            assert_refused_input(&refused, path, kind);
            let input = refused.get_ref().expect("the source");
            assert_eq!(
                input
                    .source()
                    .expect("the original I/O failure")
                    .to_string(),
                "指定されたファイルを読み取れません。"
            );
            assert!(!refused.to_string().contains("指定された"));
        }
    }

    #[test]
    fn external_compiler_and_linker_inputs_are_refused_in_every_form() {
        let directory = tempfile::tempdir().expect("an owned root");
        for flags in [
            "@/deps/flags",
            "--extern=core",
            "--extern",
            "-L/deps",
            "-L",
            "-lmock",
            "-l",
            "-Clinker=cc",
            "-C\u{1f}linker=cc",
            "-Clinker\u{1f}cc",
            "-Ccodegen-backend=cranelift",
            "--sysroot=/elsewhere",
            "link-arg=/not/even-a-flag.o",
        ] {
            let refused = bound(
                directory.path(),
                Some(crate::sealed::TARGET),
                "CARGO_ENCODED_RUSTFLAGS",
                flags,
            )
            .expect_err(&format!("{flags} names an input the record does not bind"));
            assert!(
                refused.to_string().contains("external")
                    || refused.to_string().contains("unsupported"),
                "the refusal names its class: {flags} -> {refused}"
            );
        }
    }

    #[test]
    fn incomplete_c_forms_are_refused() {
        let directory = tempfile::tempdir().expect("an owned root");
        for flags in ["-C", "-C\u{1f}"] {
            let refused = bound(
                directory.path(),
                Some(crate::sealed::TARGET),
                "CARGO_ENCODED_RUSTFLAGS",
                flags,
            )
            .expect_err(&format!("{flags:?} ends half a flag"));
            assert!(
                refused.to_string().contains("incomplete"),
                "the refusal says the flag is incomplete: {flags:?} -> {refused}"
            );
        }
    }

    #[test]
    fn a_separate_c_option_prefix_is_read_as_attached_forms_are() {
        let directory = tempfile::tempdir().expect("an owned root");
        for flags in [
            "-C\u{1f}opt-level=0",
            "-C\u{1f}debuginfo=0",
            "--cfg\u{1f}built",
        ] {
            bound(
                directory.path(),
                Some(crate::sealed::TARGET),
                "CARGO_ENCODED_RUSTFLAGS",
                flags,
            )
            .unwrap_or_else(|error| panic!("{flags:?} binds no external input: {error}"));
            bound(
                directory.path(),
                None,
                "RUSTFLAGS",
                &flags.replace('\u{1f}', " "),
            )
            .unwrap_or_else(|error| panic!("{flags:?} binds no external input: {error}"));
        }
    }

    /// A tree with no dependency under an owned parent spelled as the filesystem spells it, and a loader search directory beside it.
    fn bound_tree() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let directory = tempfile::tempdir().expect("an owned parent");
        let parent = std::fs::canonicalize(directory.path()).expect("the parent's own spelling");
        let tree = parent.join("tree");
        std::fs::create_dir_all(tree.join("src")).expect("a source directory");
        std::fs::write(
            tree.join("Cargo.toml"),
            "[package]\nname = \"bound\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n",
        )
        .expect("a manifest with no dependency");
        std::fs::write(tree.join("Cargo.lock"), "version = 4\n").expect("a lock file");
        std::fs::write(tree.join("src").join("lib.rs"), "pub fn one() {}\n").expect("a library");
        let search = parent.join("search");
        std::fs::create_dir_all(&search).expect("a loader search directory");
        (directory, tree, search)
    }

    /// The toolchain this suite was built with, located from `tree`, and the environment of a run with every compiler wrapper cleared.
    fn located(
        tree: &Path,
        cancel: &crate::runner::Cancel,
        trace: &crate::trace::Recorder,
    ) -> (crate::cargo::Toolchain, crate::vars::Variables) {
        let mut env: crate::vars::Variables = njutest_devkit::paths::environment_for_a_run()
            .into_iter()
            .collect();
        env.set("RUSTC_WRAPPER", "");
        env.set("RUSTC_WORKSPACE_WRAPPER", "");
        let toolchain = crate::cargo::Toolchain::locate(
            &crate::cargo::LocateOptions {
                cargo: Some(njutest_devkit::paths::cargo_binary()),
                env: Some(env.clone()),
                ..crate::cargo::LocateOptions::default()
            },
            tree,
            &crate::runner::Watched::new(cancel, trace),
        )
        .expect("the toolchain this suite was built with");
        (toolchain, env)
    }

    /// A locked build of the tree at `tree` into a target directory beside it.
    fn locked(tree: &Path) -> CompileOptions {
        let target = tree.with_file_name("target");
        let mut options = CompileOptions::new(
            super::super::BuildDir::new(target, Vec::new()).rooted(tree.to_path_buf()),
        );
        options.locked = true;
        options
    }

    /// A recorder that keeps what it is told in memory.
    fn recorder() -> crate::trace::Recorder {
        crate::trace::Recorder::wall(
            crate::trace::Sink::Memory(crate::trace::MemorySink::unbounded()),
            crate::testkit::trace::standalone_context(),
        )
    }

    #[test]
    fn a_key_computed_again_names_every_input_that_changed_since_the_first() {
        let (owned, tree, search) = bound_tree();
        let (cancel, trace) = (crate::runner::Cancel::new(), recorder());
        let (toolchain, mut env) = located(&tree, &cancel, &trace);
        let driver = super::Driver {
            toolchain: &toolchain,
            dir: &tree,
            cancel: &cancel,
            trace: &trace,
        };
        let options = locked(&tree);
        let variable = *super::loaders::search_variables()
            .first()
            .expect("a platform whose loader searches a variable");
        env.set(variable, &search);
        let request = super::Request::of(&driver, &options, &mut env).expect("a bound request");
        request
            .unchanged(&driver, &options)
            .expect("a key computed again over the same inputs is the same key");
        std::fs::write(tree.join("src").join("lib.rs"), "pub fn two() {}\n")
            .expect("the library changes");
        std::fs::write(
            search.join("appeared"),
            super::loaders::tests::test_library(b"a library the compile left"),
        )
        .expect("a library appears where the loader searches");
        let refused = request
            .unchanged(&driver, &options)
            .expect_err("a key computed again over changed inputs is another key");
        let message = refused.to_string();
        for named in [tree.join("src").join("lib.rs"), search.join("appeared")] {
            assert!(
                message.contains(&named.display().to_string()),
                "the refusal names {}, the input that changed, so a reader learns the cause \
                 from it: {message}",
                named.display()
            );
        }
        let changed = refused
            .get_ref()
            .and_then(|source| source.downcast_ref::<super::folding::InputsChangedError>())
            .expect("the refusal carries the typed changes");
        assert_eq!(
            changed.changes(),
            [
                super::folding::Change::Changed(super::folding::Input::File(
                    tree.join("src").join("lib.rs")
                )),
                super::folding::Change::Appeared(super::folding::Input::Searched(
                    search.join("appeared")
                )),
            ],
            "exactly the two inputs that changed are named: {message}"
        );
        drop(owned);
    }
}
