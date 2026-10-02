// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A reader's complete source/configuration and genuine execution evidence, held before use.

use std::collections::BTreeMap;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use sha2::Digest as _;

mod tree;
pub use tree::OriginalTree;

const SCHEMA: &str = "njutest-reader-recording-v1";

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Subject {
    report: String,
    trace: String,
}

#[derive(Debug, Clone, Copy)]
enum Product {
    Engine,
    Assurance,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Execution {
    schema: String,
    command: Vec<String>,
    cwd: String,
    environment: BTreeMap<String, String>,
    producer_revision: String,
    producer_patch_sha256: String,
    binary_sha256: String,
    exit_code: i32,
    started: String,
    duration_seconds: f64,
    stdout_sha256: String,
    stderr_sha256: String,
}

impl Binding {
    fn product(&self, arguments: &[&str]) -> io::Result<Product> {
        if self.schema != SCHEMA
            || self.arguments != arguments
            || self.command.is_empty()
            || self.producer_revision.len() != 40
            || !self
                .producer_revision
                .bytes()
                .all(|one| one.is_ascii_hexdigit())
            || self.subjects.is_empty()
            || ![0, 1, 2].contains(&self.exit_code)
        {
            return Err(io::Error::other(
                "the original recording's schema, command or provenance is incomplete or mismatched",
            ));
        }
        safe(&self.fixture)?;
        let product = match self.program.rsplit(['/', '\\']).next() {
            Some("rust-mutants" | "rust-mutants.exe") => Product::Engine,
            Some("njutest" | "njutest.exe") => Product::Assurance,
            Some(_) | None => {
                return Err(io::Error::other(
                    "the recording names an unknown actual producer",
                ));
            }
        };
        let mut command = vec![self.program.clone()];
        command.extend(self.arguments.iter().cloned());
        command.extend(self.recording_arguments.iter().cloned());
        match product {
            Product::Engine => command.extend(["--root".to_owned(), self.source_root.clone()]),
            Product::Assurance => {}
        }
        if self.command != command
            || !absolute(&self.source_root)
            || !absolute(&self.program)
            || !(self.recording_arguments.is_empty() || self.recording_arguments == ["--trace"])
        {
            return Err(io::Error::other(
                "the actual producer command does not match the complete request",
            ));
        }
        Ok(product)
    }

    fn captured(&self, directory: &Path) -> io::Result<()> {
        let producer: Execution = crate::strictjson::decode_slice(&std::fs::read(
            directory.join("provenance/producer.json"),
        )?)
        .map_err(io::Error::other)?;
        let patch = tree::producer_patch(&std::fs::read(
            directory.join("provenance/producer-source.tar"),
        )?)?;
        if producer.schema != "njutest-reader-producer-v1"
            || producer.command != self.command
            || producer.cwd != self.source_root
            || producer.producer_revision != self.producer_revision
            || producer.exit_code != self.exit_code
            || !producer.duration_seconds.is_finite()
            || producer.duration_seconds < 0.0
            || producer.started.is_empty()
            || producer.environment.is_empty()
            || !sha256(&producer.binary_sha256)
            || digest(&patch) != producer.producer_patch_sha256
            || self.provenance.get("stdout") != Some(&producer.stdout_sha256)
            || self.provenance.get("stderr") != Some(&producer.stderr_sha256)
        {
            return Err(io::Error::other(
                "the actual producer command, input, output or source disagrees with the recording binding",
            ));
        }
        Ok(())
    }
}

/// An original recording whose complete input and artifact inventories have been verified.
#[derive(Debug)]
pub struct Original {
    directory: PathBuf,
    binding: Binding,
}

impl Original {
    /// Opens `name` only when its complete original bytes and command match the requested configuration.
    ///
    /// # Errors
    /// The binding, original tree, artifact inventory, report or trace is missing, mismatched or unsafe.
    pub fn open(name: &str, arguments: &[&str]) -> io::Result<Self> {
        let relative = safe(name)?;
        let directory = crate::paths::workspace_root()
            .join("xtask/tests/testdata/reader-runs")
            .join(relative);
        Self::read(&directory, arguments)
    }

    /// Reads a retained directory with the same strict binding as [`Self::open`].
    ///
    /// # Errors
    /// An original byte, complete inventory or requested command does not match its binding.
    pub fn read(directory: &Path, arguments: &[&str]) -> io::Result<Self> {
        layout(directory)?;
        let binding: Binding =
            crate::strictjson::decode_slice(&std::fs::read(directory.join("binding.json"))?)
                .map_err(io::Error::other)?;
        let product = binding.product(arguments)?;
        let original = OriginalTree::read(&directory.join("original"))?;
        if original.digests() != binding.sources {
            return Err(io::Error::other(
                "the complete original source differs from the producer binding",
            ));
        }
        verify(&directory.join("artifacts"), &binding.artifacts)?;
        verify(&directory.join("provenance"), &binding.provenance)?;
        binding.captured(directory)?;
        for subject in &binding.subjects {
            for file in [&subject.report, &subject.trace] {
                safe(file)?;
                if !binding.artifacts.contains_key(file) {
                    return Err(io::Error::other(format!(
                        "the original recording does not retain {file}"
                    )));
                }
            }
        }
        subjects(product, &binding)?;
        Ok(Self {
            directory: directory.to_path_buf(),
            binding,
        })
    }

    /// Restores the actual artifacts only after the reader's complete tree matches the original source and configuration.
    ///
    /// # Errors
    /// A file or configuration differs, a source is hidden, or an artifact would replace existing data.
    pub fn restore(&self, root: &Path) -> io::Result<()> {
        layout(&self.directory)?;
        let original = OriginalTree::read(&self.directory.join("original"))?;
        if original.digests() != self.binding.sources {
            return Err(io::Error::other(
                "the held original source changed before restoration",
            ));
        }
        original.check_source(root)?;
        verify(&self.directory.join("artifacts"), &self.binding.artifacts)?;
        verify(&self.directory.join("provenance"), &self.binding.provenance)?;
        self.binding.captured(&self.directory)?;
        for relative in self.binding.artifacts.keys() {
            let relative = safe(relative)?;
            let target = root.join(relative);
            if target.try_exists()? {
                return Err(io::Error::other(format!(
                    "the reader already holds {}",
                    target.display()
                )));
            }
            let parent = target
                .parent()
                .ok_or_else(|| io::Error::other("the retained artifact has no parent"))?;
            std::fs::create_dir_all(parent)?;
            let mut destination = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(target)?;
            let mut original =
                std::fs::File::open(self.directory.join("artifacts").join(relative))?;
            io::copy(&mut original, &mut destination)?;
        }
        Ok(())
    }

    /// Copies the current fixture and binds it to this original before restoring the artifacts.
    ///
    /// # Errors
    /// The current fixture no longer has the complete source and configuration that produced the recording.
    pub fn fixture(&self) -> io::Result<crate::fixture::Fixture> {
        let fixture = crate::fixture::Fixture::copy(&self.binding.fixture);
        self.restore(fixture.root())?;
        Ok(fixture)
    }
}

fn safe(relative: &str) -> io::Result<&Path> {
    let path = Path::new(relative);
    if relative.is_empty()
        || relative.contains('\\')
        || relative.contains(':')
        || relative
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
        || !path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err(io::Error::other(format!(
            "a retained path is not a safe relative path: {relative:?}"
        )));
    }
    Ok(path)
}

fn absolute(path: &str) -> bool {
    Path::new(path).is_absolute()
        || path.starts_with('/')
        || path.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            && path.as_bytes().get(1) == Some(&b':')
            && path
                .as_bytes()
                .get(2)
                .is_some_and(|one| matches!(*one, b'/' | b'\\'))
        || path.starts_with("\\\\") && path.split('\\').filter(|one| !one.is_empty()).count() >= 3
}

fn sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|one| one.is_ascii_hexdigit())
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(bytes))
}

fn subjects(product: Product, binding: &Binding) -> io::Result<()> {
    let mut held = std::collections::BTreeSet::new();
    for subject in &binding.subjects {
        if !held.insert((&subject.report, &subject.trace)) {
            return Err(io::Error::other("the original repeats an audited subject"));
        }
    }
    let mut expected = std::collections::BTreeSet::new();
    for relative in binding.artifacts.keys() {
        let report = match product {
            Product::Engine => "run-report-v1.json",
            Product::Assurance => "njutest-assurance-report-v1.json",
        };
        let path = safe(relative)?;
        if path.file_name() != Some(std::ffi::OsStr::new(report)) {
            continue;
        }
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("an audited report has no run directory"))?;
        let trace = match product {
            Product::Engine => format!("{}/trace/trace.jsonl", parent.display()),
            Product::Assurance => format!(
                ".njutest/trace/{}/trace.jsonl",
                parent
                    .file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .ok_or_else(|| io::Error::other("the report has no run identity"))?
            ),
        };
        expected.insert((relative.clone(), trace.replace('\\', "/")));
    }
    let held: std::collections::BTreeSet<_> = held
        .into_iter()
        .map(|(report, trace)| (report.clone(), trace.clone()))
        .collect();
    if held != expected {
        return Err(io::Error::other(
            "the audited subjects are not the complete actual report inventory",
        ));
    }
    Ok(())
}

fn verify(root: &Path, expected: &BTreeMap<String, String>) -> io::Result<()> {
    if expected.is_empty() {
        return Err(io::Error::other(
            "an original recording must retain a complete nonempty inventory",
        ));
    }
    let actual = inventory(root)?;
    if &actual != expected {
        let missing: Vec<&str> = expected
            .keys()
            .filter(|name| !actual.contains_key(*name))
            .map(String::as_str)
            .collect();
        let extra: Vec<&str> = actual
            .keys()
            .filter(|name| !expected.contains_key(*name))
            .map(String::as_str)
            .collect();
        let changed: Vec<&str> = expected
            .iter()
            .filter(|(name, digest)| actual.get(*name).is_some_and(|one| one != *digest))
            .map(|(name, _digest)| name.as_str())
            .collect();
        return Err(io::Error::other(format!(
            "{} differs from the complete recording: missing {missing:?}, hidden {extra:?}, changed {changed:?}",
            root.display()
        )));
    }
    Ok(())
}

fn inventory(root: &Path) -> io::Result<BTreeMap<String, String>> {
    if !std::fs::symlink_metadata(root)?.file_type().is_dir() {
        return Err(io::Error::other(
            "a retained inventory root is not an owned directory",
        ));
    }
    let mut files = BTreeMap::new();
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(directory)? {
            let path = entry?.path();
            let kind = std::fs::symlink_metadata(&path)?.file_type();
            if kind.is_dir() {
                directories.push(path);
            } else if kind.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .map_err(io::Error::other)?
                    .to_str()
                    .ok_or_else(|| io::Error::other("a retained path is not UTF-8"))?
                    .replace('\\', "/");
                safe(&relative)?;
                let digest = hex::encode(sha2::Sha256::digest(std::fs::read(path)?));
                if files.insert(relative, digest).is_some() {
                    return Err(io::Error::other("an original recording repeats a path"));
                }
            } else {
                return Err(io::Error::other(format!(
                    "{} is not a retained regular file or directory",
                    path.display()
                )));
            }
        }
    }
    Ok(files)
}

fn layout(directory: &Path) -> io::Result<()> {
    if !std::fs::symlink_metadata(directory)?.file_type().is_dir() {
        return Err(io::Error::other(
            "an original recording is not an owned directory",
        ));
    }
    let names = std::fs::read_dir(directory)?
        .map(|entry| entry.map(|one| one.file_name()))
        .collect::<io::Result<std::collections::BTreeSet<_>>>()?;
    let expected = ["artifacts", "binding.json", "original", "provenance"]
        .map(std::ffi::OsString::from)
        .into_iter()
        .collect();
    if names != expected {
        return Err(io::Error::other(
            "the original recording has missing or hidden evidence",
        ));
    }
    Ok(())
}
