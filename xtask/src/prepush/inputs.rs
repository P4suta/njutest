// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Actual external inputs to a complete cold and warm proof.

use std::collections::BTreeSet;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use super::{PrePushError, Surroundings, io_error};

pub(super) fn fingerprint(
    surroundings: &Surroundings<'_>,
    target: &Path,
) -> Result<String, PrePushError> {
    let mut inputs = Inputs {
        digest: Sha256::new(),
        seen: BTreeSet::new(),
    };
    inputs.digest.update(b"njutest-complete-proof-inputs-v1\0");
    inputs.file(surroundings.executable)?;
    let environment = surroundings.environment;
    let spelling = environment.spelling();
    for build in [true, false] {
        inputs
            .digest
            .update(if build { b"build\0" } else { b"other\0" });
        for (name, value) in environment.canonical(|name| {
            super::shapes_the_build(spelling, name) == build && !spelling.begins(name, "GIT_")
        }) {
            inputs.digest.update(name.as_encoded_bytes());
            inputs.digest.update(b"=");
            inputs.digest.update(value.as_encoded_bytes());
            inputs.digest.update(b"\0");
        }
    }
    for tool in ["mise", "cargo", "git"] {
        inputs.file(&selected(tool, surroundings)?)?;
    }
    let cargo = crate::tools::cargo(surroundings.directory, environment)
        .map_err(|source| io_error(surroundings.directory, io::Error::other(source)))?;
    inputs.file(&cargo)?;
    let home = environment
        .value("HOME")
        .or_else(|| environment.value("USERPROFILE"))
        .map(PathBuf::from);
    let cargo_home = environment
        .value("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|home| home.join(".cargo")));
    if let Some(cargo_home) = &cargo_home {
        for path in ["config", "config.toml", "advisory-db", "advisory-dbs"] {
            inputs.optional(&cargo_home.join(path), true)?;
        }
    }
    let configured_target = environment
        .value("CARGO_TARGET_DIR")
        .map_or_else(|| target.to_path_buf(), PathBuf::from);
    inputs.optional(&configured_target.join("advisory-db"), true)?;
    inputs.optional(&target.join("advisory-db"), true)?;
    let configuration = surroundings.directory.join("rust-toolchain.toml");
    match std::fs::read_to_string(&configuration) {
        Ok(text) => {
            inputs.file(&configuration)?;
            workspace(&mut inputs, surroundings, &text)?;
            if let Some(home) = &home {
                inputs.optional(&home.join(".kani"), false)?;
            }
            let codeql = environment
                .value("NJUTEST_CODEQL_CACHE")
                .map(PathBuf::from)
                .or_else(|| {
                    super::platform_caches(environment).map(|cache| cache.join("njutest/codeql"))
                })
                .ok_or(PrePushError::Nowhere)?;
            inputs.optional(&codeql, false)?;
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            inputs.digest.update(b"no-pinned-rust-toolchain\0");
        }
        Err(source) => return Err(io_error(configuration, source)),
    }
    Ok(hex::encode(inputs.digest.finalize()))
}

fn workspace(
    inputs: &mut Inputs,
    surroundings: &Surroundings<'_>,
    configuration: &str,
) -> Result<(), PrePushError> {
    let root = surroundings.directory;
    let environment = surroundings.environment;
    let table: toml::Value =
        toml::from_str(configuration).map_err(|source| io_error(root, io::Error::other(source)))?;
    let channel = table
        .get("toolchain")
        .and_then(|toolchain| toolchain.get("channel"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| io_error(root, io::Error::other("the pinned channel is absent")))?;
    let nightly = table
        .get("njutest")
        .and_then(|settings| settings.get("nightly"))
        .and_then(toml::Value::as_str);
    let rustup = selected("rustup", surroundings)?;
    inputs.file(&rustup)?;
    for toolchain in std::iter::once(channel).chain(nightly) {
        let mut command = super::Tools { environment }.command("rustup");
        command
            .args(["which", "--toolchain", toolchain, "rustc"])
            .current_dir(root);
        let output = crate::tools::capture(&mut command, environment)
            .map_err(|source| io_error(&rustup, source))?;
        if !output.status.success() {
            return Err(io_error(
                &rustup,
                io::Error::other(format!(
                    "the actual toolchain input is unresolved: {}",
                    output.stderr.escape_ascii()
                )),
            ));
        }
        let executable = String::from_utf8(output.stdout)
            .map_err(|source| io_error(&rustup, io::Error::other(source)))?;
        let executable = Path::new(executable.trim());
        let sysroot = executable
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| io_error(executable, io::Error::other("rustc has no sysroot")))?;
        inputs.tree(&sysroot.join("bin"), false)?;
        inputs.tree(&sysroot.join("lib"), false)?;
    }
    for path in crate::tools::proof_paths(root, environment)
        .map_err(|source| io_error(root, io::Error::other(source)))?
    {
        inputs.file(&path)?;
    }
    let cargo = crate::tools::cargo(root, environment)
        .map_err(|source| io_error(root, io::Error::other(source)))?;
    let mut command = std::process::Command::new(cargo);
    command.envs(environment.pairs());
    command
        .args(["metadata", "--locked", "--offline", "--format-version", "1"])
        .current_dir(root);
    let output = crate::tools::capture(&mut command, environment)
        .map_err(|source| io_error(root, source))?;
    if !output.status.success() {
        return Err(io_error(
            root,
            io::Error::other(format!(
                "the actual dependency input is unresolved: {}",
                output.stderr.escape_ascii()
            )),
        ));
    }
    let metadata: cargo_metadata::Metadata = crate::strictjson::from_slice(&output.stdout)
        .and_then(serde_json::from_value)
        .map_err(|source| io_error(root, io::Error::other(source)))?;
    let workspace = std::fs::canonicalize(root).map_err(|source| io_error(root, source))?;
    for package in metadata.packages {
        let manifest = package.manifest_path.as_std_path();
        let directory = manifest
            .parent()
            .ok_or_else(|| io_error(manifest, io::Error::other("a dependency has no directory")))?;
        let directory =
            std::fs::canonicalize(directory).map_err(|source| io_error(directory, source))?;
        if !directory.starts_with(&workspace) {
            inputs.tree(&directory, false)?;
        }
    }
    Ok(())
}

fn selected(tool: &str, surroundings: &Surroundings<'_>) -> Result<PathBuf, PrePushError> {
    let environment = surroundings.environment;
    let path = environment.value("PATH").ok_or_else(|| {
        io_error(
            tool,
            io::Error::other("the actual tool search path is absent"),
        )
    })?;
    let program = if cfg!(windows) {
        format!("{tool}.exe")
    } else {
        tool.to_owned()
    };
    for directory in std::env::split_paths(path) {
        let directory = if directory.is_absolute() {
            directory
        } else {
            surroundings.directory.join(directory)
        };
        let candidate = directory.join(&program);
        match std::fs::metadata(&candidate) {
            Ok(metadata) if metadata.is_file() => return Ok(candidate),
            Ok(_other) => {}
            Err(source) if source.kind() == io::ErrorKind::NotFound => {}
            Err(source) => return Err(io_error(candidate, source)),
        }
    }
    Err(io_error(
        tool,
        io::Error::other("the actual executable is absent"),
    ))
}

struct Inputs {
    digest: Sha256,
    seen: BTreeSet<PathBuf>,
}

impl Inputs {
    fn file(&mut self, path: &Path) -> Result<(), PrePushError> {
        self.digest.update(path.as_os_str().as_encoded_bytes());
        self.digest.update(b"\0");
        let actual = std::fs::canonicalize(path).map_err(|source| io_error(path, source))?;
        self.digest.update(actual.as_os_str().as_encoded_bytes());
        if !self.seen.insert(actual.clone()) {
            self.digest.update(b"already-bound\0");
            return Ok(());
        }
        let mut file = std::fs::File::open(&actual).map_err(|source| io_error(path, source))?;
        let mut bytes = [0; 16384];
        loop {
            let count = file
                .read(&mut bytes)
                .map_err(|source| io_error(path, source))?;
            if count == 0 {
                self.digest.update(b"\0end-of-input\0");
                return Ok(());
            }
            let read = bytes.get(..count).ok_or_else(|| {
                io_error(path, io::Error::other("the input read exceeded its buffer"))
            })?;
            self.digest.update(read);
        }
    }

    fn optional(&mut self, path: &Path, security: bool) -> Result<(), PrePushError> {
        match std::fs::metadata(path) {
            Ok(metadata) if metadata.is_dir() => self.tree(path, security),
            Ok(_other) => self.file(path),
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                self.digest.update(path.as_os_str().as_encoded_bytes());
                self.digest.update(b"\0absent-input\0");
                Ok(())
            }
            Err(source) => Err(io_error(path, source)),
        }
    }

    fn tree(&mut self, path: &Path, security: bool) -> Result<(), PrePushError> {
        let actual = std::fs::canonicalize(path).map_err(|source| io_error(path, source))?;
        self.digest.update(path.as_os_str().as_encoded_bytes());
        self.digest.update(actual.as_os_str().as_encoded_bytes());
        if !self.seen.insert(actual) {
            self.digest.update(b"already-bound-directory\0");
            return Ok(());
        }
        let entries = crate::repository::entries(path).map_err(|source| io_error(path, source))?;
        self.digest.update(path.as_os_str().as_encoded_bytes());
        self.digest.update(b"\0directory\0");
        for path in entries {
            if security && path.file_name().is_some_and(|name| name == ".git") {
                continue;
            }
            let metadata = std::fs::metadata(&path).map_err(|source| io_error(&path, source))?;
            if metadata.is_dir() {
                self.tree(&path, security)?;
            } else {
                self.file(&path)?;
            }
        }
        Ok(())
    }
}
