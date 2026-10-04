// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Original native invocation attestations retained with immutable compiler products.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::cargo::build_cache::File;
use crate::cargo::build_cache::loaders::{RuntimeAttestation, RuntimeInputs};
use crate::cargo::build_cache::toolchain::Identities;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Attestation {
    NotNative,
    #[serde(rename = "original-native")]
    Original(BTreeMap<PathBuf, RuntimeAttestation>),
}

#[derive(Debug)]
pub(super) enum Inputs {
    NotNative,
    Original(BTreeMap<PathBuf, RuntimeInputs>),
}

fn invocations(files: &BTreeMap<PathBuf, File>) -> io::Result<BTreeSet<PathBuf>> {
    let expected: BTreeSet<_> = files
        .keys()
        .filter(|path| path.extension() == Some(OsStr::new("wasm")))
        .map(|path| path.with_extension("invocation"))
        .collect();
    let observed: BTreeSet<_> = files
        .keys()
        .filter(|path| path.extension() == Some(OsStr::new("invocation")))
        .cloned()
        .collect();
    if observed != expected {
        return Err(io::Error::other("incomplete native invocation inventory"));
    }
    Ok(observed)
}

fn original(directory: &Path, path: &Path) -> io::Result<(crate::vars::Variables, PathBuf)> {
    super::super::native::invocation_runtime_inputs(&std::fs::read(directory.join(path))?)
}

impl Inputs {
    pub(super) fn capture(
        directory: &Path,
        files: &BTreeMap<PathBuf, File>,
        identities: &Identities,
    ) -> io::Result<Self> {
        let mut retained = BTreeMap::new();
        for path in invocations(files)? {
            let (environment, cwd) = original(directory, &path)?;
            let inputs = RuntimeInputs::capture(&environment, &cwd, identities)?;
            retained.insert(path, inputs);
        }
        Ok(Self::Original(retained))
    }

    pub(super) fn attestation(&self) -> Attestation {
        match self {
            Self::NotNative => Attestation::NotNative,
            Self::Original(inputs) => Attestation::Original(
                inputs
                    .iter()
                    .map(|(path, inputs)| (path.clone(), inputs.attestation()))
                    .collect(),
            ),
        }
    }

    pub(super) fn verify(&self) -> io::Result<()> {
        match self {
            Self::NotNative => Err(io::Error::other("no original native runtime capability")),
            Self::Original(inputs) => {
                for inputs in inputs.values() {
                    inputs.verify()?;
                }
                Ok(())
            }
        }
    }
}

impl Attestation {
    pub(super) fn restore(
        &self,
        directory: &Path,
        files: &BTreeMap<PathBuf, File>,
        identities: &Identities,
    ) -> io::Result<Inputs> {
        match self {
            Self::NotNative => Ok(Inputs::NotNative),
            Self::Original(originals) => {
                if originals.keys().cloned().collect::<BTreeSet<_>>() != invocations(files)? {
                    return Err(io::Error::other(
                        "original native attestation inventory changed",
                    ));
                }
                let mut retained = BTreeMap::new();
                for (path, attestation) in originals {
                    let (environment, cwd) = original(directory, path)?;
                    retained.insert(
                        path.clone(),
                        RuntimeInputs::restore(attestation, &environment, &cwd, identities)?,
                    );
                }
                Ok(Inputs::Original(retained))
            }
        }
    }
}
