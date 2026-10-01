// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Verified content-addressed compilations that answer without starting Cargo.

use std::collections::BTreeMap;
use std::io::{self, Read as _, Write as _};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::{CompileOptions, Compiled, Completion, Driver, Exited, Message};
use crate::vars::Variables;

pub(super) mod toolchain;

const SCHEMA: &str = "rust-mutants-compilation-v1";
const DIRECTORY: &str = "rust-mutants-compilations";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
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
}

pub(super) struct Request {
    pub(super) key: String,
    record: PathBuf,
    inputs: BTreeMap<PathBuf, File>,
}

impl Request {
    pub(super) fn of(
        driver: &Driver<'_>,
        options: &CompileOptions,
        env: &Variables,
    ) -> io::Result<Self> {
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
        let mut inputs = BTreeMap::new();
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
        toolchain::inputs(driver.toolchain, options, env, &mut inputs)?;
        flag_files(options, env, &mut inputs)?;
        let mut digest = Sha256::new();
        field(&mut digest, SCHEMA.as_bytes());
        field(
            &mut digest,
            format!(
                "{:?}/{:?}",
                driver.toolchain.cargo_version(),
                driver.toolchain.rustc_version()
            )
            .as_bytes(),
        );
        for argument in super::compile_arguments(options) {
            field(&mut digest, argument.as_encoded_bytes());
        }
        field(&mut digest, root.as_os_str().as_encoded_bytes());
        for (path, state) in &inputs {
            field(&mut digest, path.as_os_str().as_encoded_bytes());
            field(&mut digest, state.digest.as_bytes());
            field(&mut digest, &state.mode.to_be_bytes());
        }
        for (name, value) in env
            .canonical()
            .into_iter()
            .filter(|(name, _value)| cargo_variable(env.spelling(), name))
        {
            field(&mut digest, name.as_encoded_bytes());
            field(&mut digest, value.as_encoded_bytes());
        }
        let key = hex::encode(digest.finalize());
        Ok(Self {
            record: options
                .target_dir
                .path()
                .join(DIRECTORY)
                .join(format!("{key}.json")),
            key,
            inputs,
        })
    }

    pub(super) fn read(
        &self,
        driver: &Driver<'_>,
        options: &CompileOptions,
        env: &Variables,
    ) -> io::Result<Compiled> {
        let record: Record = crate::strictjson::decode_slice(&std::fs::read(&self.record)?)
            .map_err(io::Error::other)?;
        if record.schema != SCHEMA
            || record.key != self.key
            || record.stdout_digest != crate::id::HexDigest::of(&record.stdout).as_str()
        {
            return Err(io::Error::other("unverified compilation record"));
        }
        for (name, value) in &record.environment {
            if &environment_value(env, name) != value {
                return Err(io::Error::other("compiler environment changed"));
            }
        }
        verified_files(&record.files, options.target_dir.path())?;
        let mut messages = super::parse_messages(&record.stdout).map_err(io::Error::other)?;
        let completion = Completion::of(
            &messages,
            Exited::of(&crate::runner::Termination::Exited(
                crate::runner::ProcessExit::Code(0),
            ))
            .ok_or_else(|| io::Error::other("successful exit"))?,
        )
        .map_err(io::Error::other)?;
        if completion != Completion::Built || !plain_messages(&messages) {
            return Err(io::Error::other("incomplete or opaque compilation"));
        }
        for message in &mut messages {
            if let Message::CompilerArtifact(artifact) = message {
                artifact.fresh = true;
                if artifact
                    .filenames
                    .iter()
                    .chain(&artifact.executable)
                    .any(|path| !record.files.contains_key(path))
                {
                    return Err(io::Error::other("unverified artifact in Cargo messages"));
                }
                for path in artifact.filenames.iter().chain(&artifact.executable) {
                    if let Some(depinfo) = super::dep_info_path(path, &artifact.target.name)
                        && !record.files.contains_key(&depinfo)
                    {
                        return Err(io::Error::other("unverified compiler dependency record"));
                    }
                }
            }
        }
        let units = super::units_of(&messages, driver.dir).map_err(io::Error::other)?;
        if units
            .iter()
            .flat_map(|unit| &unit.inputs)
            .any(|path| !self.inputs.contains_key(path))
        {
            return Err(io::Error::other("compiler read outside the bound inputs"));
        }
        let names: std::collections::BTreeSet<&String> =
            units.iter().flat_map(|unit| unit.env.keys()).collect();
        if names != record.environment.keys().collect()
            || names
                .iter()
                .any(|name| !cargo_variable(env.spelling(), std::ffi::OsStr::new(name)))
        {
            return Err(io::Error::other(
                "unverified compiler environment dependencies",
            ));
        }
        Ok(Compiled {
            completion,
            messages,
            units,
        })
    }

    pub(super) fn write(
        &self,
        compiled: &Compiled,
        stdout: &[u8],
        (env, target): (&Variables, &Path),
    ) -> io::Result<()> {
        if compiled.completion() != Completion::Built
            || !plain_messages(&compiled.messages)
            || compiled
                .units
                .iter()
                .flat_map(|unit| &unit.inputs)
                .any(|path| !self.inputs.contains_key(path))
        {
            return Err(io::Error::other("incomplete or opaque compiler inputs"));
        }
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
                    if let Some(depinfo) = super::dep_info_path(path, &artifact.target.name) {
                        files.insert(depinfo.clone(), file(&depinfo)?);
                    }
                }
            }
        }
        let names = compiled.units.iter().flat_map(|unit| unit.env.keys());
        if names
            .clone()
            .any(|name| !cargo_variable(env.spelling(), std::ffi::OsStr::new(name)))
        {
            return Err(io::Error::other(
                "the compiler reads a volatile diagnostic variable",
            ));
        }
        let environment = names
            .map(|name| (name.clone(), environment_value(env, name)))
            .collect();
        let record = Record {
            schema: SCHEMA.to_owned(),
            key: self.key.clone(),
            stdout: stdout.to_vec(),
            stdout_digest: crate::id::HexDigest::of(stdout).as_str().to_owned(),
            files,
            environment,
        };
        let parent = self
            .record
            .parent()
            .ok_or_else(|| io::Error::other("cache record parent"))?;
        std::fs::create_dir_all(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(&serde_json::to_vec(&record).map_err(io::Error::other)?)?;
        temporary.persist(&self.record).map_err(io::Error::other)?;
        Ok(())
    }
}

fn file(path: &Path) -> io::Result<File> {
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

fn tree(root: &Path, target: &Path, files: &mut BTreeMap<PathBuf, File>) -> io::Result<()> {
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.starts_with(target) {
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

fn plain_manifest(path: &Path) -> bool {
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

fn configurations(
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

fn flag_files(
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
    for name in [
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "RUSTDOCFLAGS",
        "CARGO_ENCODED_RUSTDOCFLAGS",
    ] {
        if let Some(flags) = env.var(name) {
            let flags = flags
                .to_str()
                .ok_or_else(|| io::Error::other("non-textual compiler flags"))?;
            for flag in flags.split(['\u{1f}', ' ']) {
                if let Some(path) = flag
                    .strip_prefix("link-arg=")
                    .filter(|path| !path.starts_with('-'))
                {
                    let path = PathBuf::from(path);
                    files.insert(path.clone(), file(&path)?);
                }
                if flag.starts_with('@')
                    || flag.starts_with("--extern")
                    || flag.starts_with("-L")
                    || flag.starts_with("-l")
                    || flag.contains("linker=")
                    || flag.contains("link-arg=")
                    || flag.contains("codegen-backend")
                    || flag.starts_with("--sysroot")
                {
                    return Err(io::Error::other("external compiler or linker inputs"));
                }
            }
        }
    }
    Ok(())
}

fn cargo_variable(spelling: crate::vars::Spelling, name: &std::ffi::OsStr) -> bool {
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

fn verified_files(files: &BTreeMap<PathBuf, File>, target: &Path) -> io::Result<()> {
    if files.is_empty() || files.keys().any(|path| !bound_artifact(path, target)) {
        return Err(io::Error::other("unbound compilation artifacts"));
    }
    let changed: Vec<&PathBuf> = files
        .iter()
        .filter_map(|(path, recorded)| match file(path) {
            Ok(current) if current == *recorded => None,
            Ok(_) | Err(_) => Some(path),
        })
        .collect();
    if changed.is_empty() {
        return Ok(());
    }
    for path in changed {
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(source) if source.kind() == io::ErrorKind::NotFound => {}
            Err(source) => return Err(source),
        }
    }
    Err(io::Error::other("compilation artifact digest changed"))
}
