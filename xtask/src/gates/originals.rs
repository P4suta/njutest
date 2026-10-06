// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The closed reader catalog binds every original source, request, subject and artifact to independently checked bytes.

use serde::Deserialize;
use sha2::Digest as _;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read as _;
use std::path::{Component, Path};

use super::GateError;

const PREFIX: &str = "xtask/tests/testdata/reader-runs/";

const FAMILIES: [&str; 7] = [
    "documents-baseline",
    "explain-simple",
    "projections-unicode",
    "reports-coverage",
    "reports-simple",
    "run-report-alltiers",
    "run-report-default",
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    schema: String,
    fixture: String,
    arguments: Vec<String>,
    recording_arguments: Vec<String>,
    command: Vec<String>,
    source_root: String,
    program: String,
    producer_revision: String,
    exit_code: i32,
    sources: BTreeMap<String, String>,
    artifacts: BTreeMap<String, String>,
    provenance: BTreeMap<String, String>,
    subjects: Vec<Subject>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Subject {
    report: String,
    trace: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    schema: String,
    archive: String,
    files: BTreeMap<String, File>,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct File {
    sha256: String,
    mode: u32,
}

pub(super) fn held(root: &Path) -> Result<String, GateError> {
    let files: BTreeSet<_> = crate::repository::files(root)?.into_iter().collect();
    let bindings: BTreeSet<_> = files
        .iter()
        .filter(|path| path.starts_with(PREFIX) && path.ends_with("/binding.json"))
        .cloned()
        .collect();
    let required: BTreeSet<_> = FAMILIES
        .iter()
        .map(|family| format!("{PREFIX}{family}/binding.json"))
        .collect();
    if bindings != required {
        return Err(GateError(
            "originals: the complete reader catalog differs from the closed repository inventory"
                .to_owned(),
        ));
    }
    let mut complete = BTreeSet::new();
    for path in &bindings {
        let directory = Path::new(path)
            .parent()
            .ok_or_else(|| GateError(format!("originals: {path}: missing parent")))?;
        let binding: Binding = document(root, &files, Path::new(path))?;
        complete_request(path, &binding)?;
        let mut expected = BTreeSet::from([
            path.clone(),
            super::relative_slash(root, &root.join(directory.join("original/source.json")))?,
            super::relative_slash(root, &root.join(directory.join("original/source.tar")))?,
        ]);
        for (field, inventory) in [
            ("artifacts", &binding.artifacts),
            ("provenance", &binding.provenance),
        ] {
            if inventory.is_empty() {
                return Err(GateError(format!(
                    "originals: {path}: empty {field} inventory"
                )));
            }
            for (name, digest) in inventory {
                let relative = directory.join(field).join(safe(name)?);
                let body = member(root, &files, &relative)?;
                check_digest(&relative, &body, digest)?;
                expected.insert(super::relative_slash(root, &root.join(relative))?);
            }
        }
        for subject in &binding.subjects {
            for name in [&subject.report, &subject.trace] {
                safe(name)?;
                if !binding.artifacts.contains_key(name) {
                    return Err(GateError(format!(
                        "originals: {path}: an actual subject is absent from the bound artifacts"
                    )));
                }
            }
        }
        let pair = directory.join("original");
        let source: Source = document(root, &files, &pair.join("source.json"))?;
        let archive = member(root, &files, &pair.join("source.tar"))?;
        check_digest(&pair.join("source.tar"), &archive, &source.archive)?;
        original_source(path, &binding, &source, &archive)?;
        complete.extend(expected);
    }
    let retained: BTreeSet<_> = files
        .iter()
        .filter(|name| name.starts_with(PREFIX))
        .cloned()
        .collect();
    if retained != complete {
        return Err(GateError(
            "originals: a hidden or missing original byte changes the complete inventory"
                .to_owned(),
        ));
    }
    Ok(format!(
        "originals: {} complete reader bindings retain every named original byte",
        bindings.len()
    ))
}

fn complete_request(path: &str, binding: &Binding) -> Result<(), GateError> {
    let mut command = vec![binding.program.clone()];
    command.extend(binding.arguments.iter().cloned());
    command.extend(binding.recording_arguments.iter().cloned());
    match binding.program.rsplit(['/', '\\']).next() {
        Some("rust-mutants" | "rust-mutants.exe") => {
            command.extend(["--root".to_owned(), binding.source_root.clone()]);
        }
        Some("njutest" | "njutest.exe") => {}
        Some(_) | None => {
            return Err(GateError(format!(
                "originals: {path}: an unknown actual producer"
            )));
        }
    }
    safe(&binding.fixture)?;
    if binding.schema != "njutest-reader-recording-v1"
        || binding.arguments.is_empty()
        || binding.command != command
        || !absolute(&binding.source_root)
        || !absolute(&binding.program)
        || binding.producer_revision.len() != 40
        || !binding
            .producer_revision
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || ![0, 1, 2].contains(&binding.exit_code)
        || binding.subjects.is_empty()
        || !(binding.recording_arguments.is_empty() || binding.recording_arguments == ["--trace"])
    {
        return Err(GateError(format!(
            "originals: {path}: an incomplete or mismatched actual request"
        )));
    }
    Ok(())
}

fn absolute(value: &str) -> bool {
    value.starts_with('/')
        || matches!(value.as_bytes(), [drive, b':', b'/' | b'\\', ..] if drive.is_ascii_alphabetic())
        || value.starts_with("\\\\")
            && value.split('\\').filter(|part| !part.is_empty()).count() >= 3
}

fn original_source(
    path: &str,
    binding: &Binding,
    source: &Source,
    body: &[u8],
) -> Result<(), GateError> {
    let mut archive = tar::Archive::new(body);
    let mut actual = BTreeMap::new();
    for entry in archive
        .entries()
        .map_err(|error| GateError(format!("originals: {path}: {error}")))?
    {
        let mut entry = entry.map_err(|error| GateError(format!("originals: {path}: {error}")))?;
        if !entry.header().entry_type().is_file() {
            return Err(GateError(format!(
                "originals: {path}: the original archive contains a non-file input"
            )));
        }
        let name = entry
            .path()
            .map_err(|error| GateError(format!("originals: {path}: {error}")))?
            .to_str()
            .ok_or_else(|| GateError(format!("originals: {path}: a non-text input")))?
            .to_owned();
        safe(&name)?;
        let mode = entry
            .header()
            .mode()
            .map_err(|error| GateError(format!("originals: {path}: {error}")))?;
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|error| GateError(format!("originals: {path}: {error}")))?;
        let file = File {
            sha256: hex::encode(sha2::Sha256::digest(&bytes)),
            mode,
        };
        if actual.insert(name, file).is_some() {
            return Err(GateError(format!(
                "originals: {path}: a repeated original archive input"
            )));
        }
    }
    let digests: BTreeMap<_, _> = actual
        .iter()
        .map(|(name, file)| (name.clone(), file.sha256.clone()))
        .collect();
    if source.schema != "njutest-original-source-v1"
        || actual.is_empty()
        || actual != source.files
        || digests != binding.sources
    {
        return Err(GateError(format!(
            "originals: {path}: the complete original source differs from its producer binding"
        )));
    }
    Ok(())
}

fn safe(name: &str) -> Result<&Path, GateError> {
    let path = Path::new(name);
    if name.is_empty()
        || name.contains(['\\', ':'])
        || path.is_absolute()
        || name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(GateError(format!(
            "originals: unsafe original name {name:?}"
        )));
    }
    Ok(path)
}

fn member(root: &Path, files: &BTreeSet<String>, relative: &Path) -> Result<Vec<u8>, GateError> {
    let name = super::relative_slash(root, &root.join(relative))?;
    if !files.contains(&name) {
        return Err(GateError(format!(
            "originals: {name}: the bound original is absent from the closed repository inventory"
        )));
    }
    if !std::fs::symlink_metadata(root.join(relative))
        .map_err(|error| GateError(format!("originals: {name}: {error}")))?
        .is_file()
    {
        return Err(GateError(format!(
            "originals: {name}: the original byte is not a regular file"
        )));
    }
    let body = std::fs::read(root.join(relative))
        .map_err(|error| GateError(format!("originals: {name}: {error}")))?;
    Ok(body)
}

fn check_digest(path: &Path, body: &[u8], wanted: &str) -> Result<(), GateError> {
    if hex::encode(sha2::Sha256::digest(body)) != wanted {
        return Err(GateError(format!(
            "originals: {}: the bound original digest changed",
            path.display()
        )));
    }
    Ok(())
}

fn document<T: serde::de::DeserializeOwned>(
    root: &Path,
    files: &BTreeSet<String>,
    path: &Path,
) -> Result<T, GateError> {
    let body = member(root, files, path)?;
    let text = std::str::from_utf8(&body)
        .map_err(|error| GateError(format!("originals: {}: {error}", path.display())))?;
    crate::strictjson::decode_str(text)
        .map_err(|error| GateError(format!("originals: {}: {error}", path.display())))
}
