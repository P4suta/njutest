// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The release archive of one target: exactly the binaries the shipped manifests declare, at exactly the paths `cargo binstall` reads them from.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fmt;
use std::io::{Read as _, Seek as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use thiserror::Error;

/// The documents every archive carries beside the binaries, named as they are at the workspace root.
pub const DOCUMENTS: [&str; 3] = ["LICENSE-MIT", "LICENSE-APACHE", "README.md"];

/// The time every entry is stamped with, the `tar` crate's own deterministic one, so the same files make the same bytes.
const STAMP: u64 = 1_153_704_088;

/// The mode of a binary and of a directory in the archive.
const EXECUTABLE: u32 = 0o755;

/// The mode of a document in the archive.
const READABLE: u32 = 0o644;

/// Why a template cannot be filled the way binstall fills it.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum TemplateError {
    /// A brace that nothing matches.
    #[error("{template:?} has a brace that nothing matches")]
    Unbalanced {
        /// The template.
        template: String,
    },
    /// A backslash, which binstall reads as an escape this command does not follow.
    #[error("{template:?} has a backslash, which binstall reads as an escape")]
    Escaped {
        /// The template.
        template: String,
    },
    /// A variable this command does not fill, so it cannot say what binstall reads.
    #[error(
        "{template:?} names {{ {variable} }}, which this command does not fill, so it cannot say \
         what binstall reads"
    )]
    Unknown {
        /// The template.
        template: String,
        /// The variable.
        variable: String,
    },
    /// A variable that has no value where the template uses it.
    #[error("{template:?} names {{ {variable} }}, which has no value there")]
    Unfilled {
        /// The template.
        template: String,
        /// The variable.
        variable: &'static str,
    },
}

impl crate::error::Coded for TemplateError {
    fn code(&self) -> crate::error::XtCode {
        crate::error::XtCode::BundleManifest
    }
}

/// Why the archive of a target could not be made.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BundleError {
    /// No package cargo would publish declares a binary.
    #[error(
        "no package cargo would publish declares a binary, and an archive of nothing is not a release"
    )]
    NothingShipped,
    /// A shipped package does not say where binstall reads its binaries.
    #[error(
        "{package} publishes binaries and has no [package.metadata.binstall], so binstall \
         compiles it rather than taking the archive"
    )]
    Unannounced {
        /// The package.
        package: String,
    },
    /// A shipped package's binstall table is not exactly the three keys this command reads.
    #[error(
        "{package}: [package.metadata.binstall] is not exactly pkg-url, pkg-fmt and bin-dir, and a \
         key this command does not read can change what binstall reads: {source}"
    )]
    Unreadable {
        /// The package.
        package: String,
        /// Why the table was refused.
        source: serde_json::Error,
    },
    /// A shipped package names a format other than the one this command writes.
    #[error("{package}: pkg-fmt is {format:?}, and the archive this command writes is \"tgz\"")]
    Format {
        /// The package.
        package: String,
        /// The format it names.
        format: String,
    },
    /// A template of a shipped package cannot be filled as binstall fills it.
    #[error("{package}: {source}")]
    Template {
        /// The package.
        package: String,
        /// Why.
        source: TemplateError,
    },
    /// A `pkg-url` that does not end in the name of a gzipped tar.
    #[error("{package}: {url} does not end in the name of a gzipped tar")]
    ArchiveName {
        /// The package.
        package: String,
        /// What its `pkg-url` fills to.
        url: String,
    },
    /// Two shipped packages name different archives for one target.
    #[error(
        "{first} names {first_archive} and {second} names {second_archive}, and a target has one archive"
    )]
    Archives {
        /// The package read first.
        first: String,
        /// The archive it names.
        first_archive: String,
        /// The package that disagrees.
        second: String,
        /// The archive it names.
        second_archive: String,
    },
    /// A binary placed on a path that is not inside the archive.
    #[error("{package}: bin-dir puts {binary} at {path:?}, which is not a path inside the archive")]
    Path {
        /// The package.
        package: String,
        /// The binary.
        binary: String,
        /// Where its `bin-dir` puts it.
        path: String,
    },
    /// Two entries at one path, which is one of them lost.
    #[error("{path} is where both {first} and {second} go")]
    Twice {
        /// The path.
        path: String,
        /// What went there first.
        first: String,
        /// What went there next.
        second: String,
    },
    /// Binaries in more than one directory, which leaves the documents no one place to go.
    #[error(
        "the binaries go to {directories:?}, and the licences and the README go beside them, \
         which needs one directory"
    )]
    Directories {
        /// Every directory a binary goes to.
        directories: Vec<String>,
    },
    /// A program could not be started.
    #[error("{program} could not be started: {source}")]
    Start {
        /// The program.
        program: String,
        /// The operating system's refusal.
        source: std::io::Error,
    },
    /// A program answered in bytes that are not UTF-8.
    #[error("{program} answered in bytes that are not UTF-8: {source}")]
    NotText {
        /// The program.
        program: String,
        /// Where the bytes stopped being text.
        source: std::str::Utf8Error,
    },
    /// cargo refused to describe the workspace.
    #[error("cargo metadata failed with {status}: {said}")]
    Metadata {
        /// How it ended.
        status: ExitStatus,
        /// What it said.
        said: String,
    },
    /// cargo answered with something that is not its metadata.
    #[error("cargo metadata answered with something that is not its metadata: {source}")]
    MetadataUnread {
        /// Why it was refused.
        source: serde_json::Error,
    },
    /// cargo could not build a package's binaries for the target.
    #[error("cargo could not build {package} for {target}, and said why above: {status}")]
    Build {
        /// The package.
        package: String,
        /// The target triple.
        target: String,
        /// How cargo ended.
        status: ExitStatus,
    },
    /// A line of cargo's build messages that is not one.
    #[error("cargo said something about {package} that is not a build message: {source}")]
    Messages {
        /// The package.
        package: String,
        /// Why the line was refused.
        source: serde_json::Error,
    },
    /// cargo did not report exactly the binaries the manifests declare.
    #[error("cargo reported {built:?} of {package}, where its manifest declares {declared:?}")]
    Unreported {
        /// The package.
        package: String,
        /// The binaries cargo reported an executable for.
        built: Vec<String>,
        /// The binaries the manifest declares.
        declared: Vec<String>,
    },
    /// A built binary did not say the version its archive is named for.
    #[error("{binary} --version ended {status} and said {said:?}, which does not name {version}")]
    Version {
        /// The binary.
        binary: String,
        /// How it ended.
        status: ExitStatus,
        /// What it said.
        said: String,
        /// The version its package has.
        version: String,
    },
    /// A file could not be read.
    #[error("{}: {source}", path.display())]
    Read {
        /// The file.
        path: PathBuf,
        /// The operating system's refusal.
        source: std::io::Error,
    },
    /// A file could not be written.
    #[error("{}: {source}", path.display())]
    Write {
        /// The file.
        path: PathBuf,
        /// The operating system's refusal.
        source: std::io::Error,
    },
    /// The archive read back is not what was planned.
    #[error("the archive read back is not what was planned:\n  {}", differences.join("\n  "))]
    ReadBack {
        /// Every way it differs.
        differences: Vec<String>,
    },
}

impl crate::error::Coded for BundleError {
    fn code(&self) -> crate::error::XtCode {
        match self {
            Self::NothingShipped
            | Self::Unannounced { .. }
            | Self::Unreadable { .. }
            | Self::Format { .. }
            | Self::Template { .. }
            | Self::ArchiveName { .. }
            | Self::Archives { .. }
            | Self::Path { .. }
            | Self::Twice { .. }
            | Self::Directories { .. } => crate::error::XtCode::BundleManifest,
            Self::Start { .. }
            | Self::NotText { .. }
            | Self::Metadata { .. }
            | Self::MetadataUnread { .. }
            | Self::Build { .. }
            | Self::Messages { .. }
            | Self::Unreported { .. } => crate::error::XtCode::BundleUnbuilt,
            Self::Version { .. } => crate::error::XtCode::BundleVersion,
            Self::Read { .. } | Self::Write { .. } | Self::ReadBack { .. } => {
                crate::error::XtCode::BundleUnwritten
            }
        }
    }
}

/// One package of the workspace, as much of it as a bundle reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// The identity cargo gives it, which its build messages carry.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its version.
    pub version: String,
    /// The repository its manifest names, which binstall's `{ repo }` is.
    pub repository: Option<String>,
    /// Whether cargo would publish it anywhere.
    pub published: bool,
    /// The binaries it declares.
    pub binaries: Vec<String>,
    /// Its `[package.metadata]` table, which the binstall one is in.
    pub metadata: serde_json::Value,
}

impl Package {
    /// The package cargo's metadata describes.
    #[must_use]
    pub fn of(package: &cargo_metadata::Package) -> Self {
        Self {
            id: package.id.repr.clone(),
            name: package.name.to_string(),
            version: package.version.to_string(),
            repository: package.repository.clone(),
            published: package
                .publish
                .as_ref()
                .is_none_or(|registries| !registries.is_empty()),
            binaries: package
                .targets
                .iter()
                .filter(|target| target.is_bin())
                .map(|target| target.name.clone())
                .collect(),
            metadata: package.metadata.clone(),
        }
    }

    /// Whether this package puts binaries in the release.
    const fn ships(&self) -> bool {
        self.published && !self.binaries.is_empty()
    }
}

/// The keys of `[package.metadata.binstall]` this command reads, and no others.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Binstall {
    pkg_url: String,
    pkg_fmt: String,
    bin_dir: String,
}

/// What binstall fills a package's templates with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Values<'a> {
    /// The package's name.
    pub name: &'a str,
    /// Its version.
    pub version: &'a str,
    /// The repository its manifest names.
    pub repo: Option<&'a str>,
    /// The target triple.
    pub target: &'a str,
    /// The binary, which only `bin-dir` has.
    pub bin: Option<&'a str>,
}

/// The variables of binstall's templates this command fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Variable {
    Name,
    Version,
    Repo,
    Target,
    Bin,
    BinaryExt,
}

impl Variable {
    /// The variable a template names by `word`, when it is one this command fills.
    fn named(word: &str) -> Option<Self> {
        match word {
            "name" => Some(Self::Name),
            "version" => Some(Self::Version),
            "repo" => Some(Self::Repo),
            "target" => Some(Self::Target),
            "bin" => Some(Self::Bin),
            "binary-ext" => Some(Self::BinaryExt),
            _ => None,
        }
    }

    /// Its name in a template.
    const fn word(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Version => "version",
            Self::Repo => "repo",
            Self::Target => "target",
            Self::Bin => "bin",
            Self::BinaryExt => "binary-ext",
        }
    }

    /// What binstall fills it with, where it has a value.
    fn value<'a>(self, values: &Values<'a>) -> Option<&'a str> {
        match self {
            Self::Name => Some(values.name),
            Self::Version => Some(values.version),
            Self::Repo => values.repo,
            Self::Target => Some(values.target),
            Self::Bin => values.bin,
            Self::BinaryExt => Some(if values.target.contains("windows") {
                ".exe"
            } else {
                ""
            }),
        }
    }
}

/// `template` filled the way binstall fills it.
///
/// # Errors
/// A brace nothing matches, a backslash, a variable this command does not fill, or one with no value here.
pub fn render(template: &str, values: &Values<'_>) -> Result<String, TemplateError> {
    let unbalanced = || TemplateError::Unbalanced {
        template: template.to_owned(),
    };
    if template.contains('\\') {
        return Err(TemplateError::Escaped {
            template: template.to_owned(),
        });
    }
    let mut filled = String::new();
    let mut rest = template;
    while let Some(brace) = rest.find(['{', '}']) {
        let (before, from) = rest.split_at_checked(brace).ok_or_else(unbalanced)?;
        filled.push_str(before);
        let inside = from.strip_prefix('{').ok_or_else(unbalanced)?;
        let (word, after) = inside.split_once('}').ok_or_else(unbalanced)?;
        if word.contains('{') {
            return Err(unbalanced());
        }
        let word = word.trim();
        let variable = Variable::named(word).ok_or_else(|| TemplateError::Unknown {
            template: template.to_owned(),
            variable: word.to_owned(),
        })?;
        let value = variable
            .value(values)
            .ok_or_else(|| TemplateError::Unfilled {
                template: template.to_owned(),
                variable: variable.word(),
            })?;
        filled.push_str(value);
        rest = after;
    }
    filled.push_str(rest);
    Ok(filled)
}

/// One binary of the archive: what cargo builds, and where binstall reads it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Program {
    /// Where binstall reads it, which its package's `bin-dir` fills to.
    pub path: String,
    /// The package that declares it.
    pub package: String,
    /// Its `[[bin]]` name.
    pub binary: String,
    /// The version it must say it is.
    pub version: String,
}

/// A document of the archive: the file at the workspace root, and where it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// Its name at the workspace root.
    pub name: String,
    /// Its path in the archive.
    pub path: String,
}

/// What the archive of one target holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The target triple.
    pub target: String,
    /// The archive's file name, the last segment of the URL every shipped `pkg-url` fills to.
    pub archive: String,
    /// Every binary, by path.
    pub programs: Vec<Program>,
    /// Every document, in the order of [`DOCUMENTS`].
    pub documents: Vec<Document>,
    /// Every directory an entry sits in, each after its parent.
    pub directories: Vec<String>,
}

/// What the archive of `packages` for `target` holds: every binary a shipped package declares, where its `bin-dir` puts it, in the one archive every `pkg-url` names.
///
/// # Errors
/// What keeps the manifests from naming one archive whose every path binstall reads.
pub fn plan(packages: &[Package], target: &str) -> Result<Plan, BundleError> {
    let mut archive: Option<(String, String)> = None;
    let mut programs = Vec::new();
    for package in packages.iter().filter(|package| package.ships()) {
        let binstall = binstall_of(package)?;
        let named = archive_of(package, &binstall, target)?;
        match &archive {
            Some((first, first_archive)) if *first_archive != named => {
                return Err(BundleError::Archives {
                    first: first.clone(),
                    first_archive: first_archive.clone(),
                    second: package.name.clone(),
                    second_archive: named,
                });
            }
            Some(_) => {}
            None => archive = Some((package.name.clone(), named)),
        }
        programs.extend(programs_of(package, &binstall, target)?);
    }
    let Some((_, archive)) = archive else {
        return Err(BundleError::NothingShipped);
    };
    programs.sort();
    let documents = documents_beside(&programs)?;
    refuse_twice(&programs, &documents)?;
    let directories = directories_of(&programs, &documents);
    Ok(Plan {
        target: target.to_owned(),
        archive,
        programs,
        documents,
        directories,
    })
}

/// The binstall table of a shipped package, refused unless it is exactly what this command reads.
fn binstall_of(package: &Package) -> Result<Binstall, BundleError> {
    let table = package
        .metadata
        .get("binstall")
        .ok_or_else(|| BundleError::Unannounced {
            package: package.name.clone(),
        })?;
    let binstall: Binstall =
        serde_json::from_value(table.clone()).map_err(|source| BundleError::Unreadable {
            package: package.name.clone(),
            source,
        })?;
    if binstall.pkg_fmt != "tgz" {
        return Err(BundleError::Format {
            package: package.name.clone(),
            format: binstall.pkg_fmt,
        });
    }
    Ok(binstall)
}

/// The values binstall fills `package`'s templates with for `target`, and `bin` in `bin-dir`.
fn values_of<'a>(package: &'a Package, target: &'a str, bin: Option<&'a str>) -> Values<'a> {
    Values {
        name: &package.name,
        version: &package.version,
        repo: package.repository.as_deref(),
        target,
        bin,
    }
}

/// The file name of the archive `package`'s `pkg-url` fetches for `target`.
fn archive_of(package: &Package, binstall: &Binstall, target: &str) -> Result<String, BundleError> {
    let url = render(&binstall.pkg_url, &values_of(package, target, None)).map_err(|source| {
        BundleError::Template {
            package: package.name.clone(),
            source,
        }
    })?;
    let name = match url.rsplit_once('/') {
        Some((_, name)) => name,
        None => url.as_str(),
    };
    let gzipped_tar = [".tar.gz", ".tgz"].into_iter().any(|suffix| {
        name.strip_suffix(suffix)
            .is_some_and(|stem| !stem.is_empty())
    });
    if !gzipped_tar || name.contains(['?', '#']) {
        return Err(BundleError::ArchiveName {
            package: package.name.clone(),
            url,
        });
    }
    Ok(name.to_owned())
}

/// Every binary `package` declares, where its `bin-dir` puts it for `target`.
fn programs_of(
    package: &Package,
    binstall: &Binstall,
    target: &str,
) -> Result<Vec<Program>, BundleError> {
    let mut found = Vec::new();
    for binary in &package.binaries {
        let path = render(&binstall.bin_dir, &values_of(package, target, Some(binary))).map_err(
            |source| BundleError::Template {
                package: package.name.clone(),
                source,
            },
        )?;
        let inside = path
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..");
        if !inside {
            return Err(BundleError::Path {
                package: package.name.clone(),
                binary: binary.clone(),
                path,
            });
        }
        found.push(Program {
            path,
            package: package.name.clone(),
            binary: binary.clone(),
            version: package.version.clone(),
        });
    }
    Ok(found)
}

/// The directory holding `path` in the archive, empty at its top.
fn parent(path: &str) -> &str {
    match path.rsplit_once('/') {
        Some((parent, _)) => parent,
        None => "",
    }
}

/// Every document, in the one directory the binaries go to.
fn documents_beside(programs: &[Program]) -> Result<Vec<Document>, BundleError> {
    let directories: BTreeSet<&str> = programs
        .iter()
        .map(|program| parent(&program.path))
        .collect();
    let mut listed = directories.iter();
    let (Some(directory), None) = (listed.next(), listed.next()) else {
        return Err(BundleError::Directories {
            directories: directories.iter().map(|one| (*one).to_owned()).collect(),
        });
    };
    Ok(DOCUMENTS
        .iter()
        .map(|name| Document {
            name: (*name).to_owned(),
            path: if directory.is_empty() {
                (*name).to_owned()
            } else {
                format!("{directory}/{name}")
            },
        })
        .collect())
}

/// Refuses two entries at one path.
fn refuse_twice(programs: &[Program], documents: &[Document]) -> Result<(), BundleError> {
    let mut seen: BTreeMap<&str, String> = BTreeMap::new();
    let entries = programs
        .iter()
        .map(|program| {
            (
                program.path.as_str(),
                format!("{} of {}", program.binary, program.package),
            )
        })
        .chain(
            documents
                .iter()
                .map(|document| (document.path.as_str(), document.name.clone())),
        );
    for (path, what) in entries {
        if let Some(first) = seen.insert(path, what.clone()) {
            return Err(BundleError::Twice {
                path: path.to_owned(),
                first,
                second: what,
            });
        }
    }
    Ok(())
}

/// Every directory an entry sits in, parents first.
fn directories_of(programs: &[Program], documents: &[Document]) -> Vec<String> {
    let mut found = BTreeSet::new();
    let paths = programs
        .iter()
        .map(|program| program.path.as_str())
        .chain(documents.iter().map(|document| document.path.as_str()));
    for path in paths {
        let mut at = parent(path);
        while !at.is_empty() {
            found.insert(at.to_owned());
            at = parent(at);
        }
    }
    found.into_iter().collect()
}

/// What `cargo xtask bundle` is asked, and what it runs with.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    /// The workspace whose shipped packages are bundled.
    pub root: &'a Path,
    /// The cargo that describes and builds them.
    pub cargo: &'a OsStr,
    /// The environment every program it starts is given.
    pub environment: &'a crate::environment::Environment,
    /// The target triple the archive is for.
    pub target: &'a str,
    /// The directory the archive and its checksum are written to.
    pub out: &'a Path,
}

/// What `bundle` wrote, and from what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// What the archive holds.
    pub plan: Plan,
    /// The archive.
    pub archive: PathBuf,
    /// The checksum beside it, as `shasum -a 256` writes one.
    pub checksum: PathBuf,
    /// The archive's SHA-256, in hex.
    pub digest: String,
}

impl fmt::Display for Written {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            formatter,
            "bundle: {} for {}, every binary asked its version and the archive read back as planned",
            self.plan.archive, self.plan.target
        )?;
        for program in &self.plan.programs {
            writeln!(
                formatter,
                "  {}  {} of {}, saying {}",
                program.path, program.binary, program.package, program.version
            )?;
        }
        for document in &self.plan.documents {
            writeln!(formatter, "  {}", document.path)?;
        }
        write!(
            formatter,
            "written to {}, sha256 {} in {}",
            self.archive.display(),
            self.digest,
            self.checksum.display()
        )
    }
}

/// Builds every shipped binary for the target, asks each its version, and writes the archive binstall reads and its checksum.
///
/// # Errors
/// What the manifests, cargo, a binary or the filesystem refused; nothing is written under the archive's name until it reads back as planned.
pub fn bundle(request: &Request<'_>) -> Result<Written, BundleError> {
    let packages = packages(request.root, request.cargo, request.environment)?;
    let plan = plan(&packages, request.target)?;
    let mut executables = BTreeMap::new();
    for package in packages.iter().filter(|package| package.ships()) {
        let programs: Vec<&Program> = plan
            .programs
            .iter()
            .filter(|program| program.package == package.name)
            .collect();
        let binaries: Vec<&str> = programs
            .iter()
            .map(|program| program.binary.as_str())
            .collect();
        let mut built = build(request, package, &binaries)?;
        for program in programs {
            let executable =
                built
                    .remove(&program.binary)
                    .ok_or_else(|| BundleError::Unreported {
                        package: package.name.clone(),
                        built: Vec::new(),
                        declared: vec![program.binary.clone()],
                    })?;
            says(request, program, &executable)?;
            executables.insert(program.path.clone(), executable);
        }
    }
    write(request, &plan, &executables)
}

/// `program` as a person reads its name.
fn shown(program: &OsStr) -> String {
    Path::new(program).display().to_string()
}

/// `program` started in `directory` with exactly `environment`, reading nothing.
fn command(
    program: &OsStr,
    directory: &Path,
    environment: &crate::environment::Environment,
) -> Command {
    let mut command = Command::new(program);
    command
        .current_dir(directory)
        .env_clear()
        .envs(environment.pairs())
        .stdin(Stdio::null());
    command
}

/// The text of what `program` wrote.
fn text<'a>(program: &OsStr, bytes: &'a [u8]) -> Result<&'a str, BundleError> {
    std::str::from_utf8(bytes).map_err(|source| BundleError::NotText {
        program: shown(program),
        source,
    })
}

/// Every package of the workspace at `root`, as cargo describes it, by name.
///
/// # Errors
/// cargo could not be started, refused, or answered with something that is not its metadata.
pub fn packages(
    root: &Path,
    cargo: &OsStr,
    environment: &crate::environment::Environment,
) -> Result<Vec<Package>, BundleError> {
    let output = command(cargo, root, environment)
        .args(["metadata", "--format-version", "1", "--no-deps", "--locked"])
        .arg("--manifest-path")
        .arg(root.join("Cargo.toml"))
        .output()
        .map_err(|source| BundleError::Start {
            program: shown(cargo),
            source,
        })?;
    if !output.status.success() {
        return Err(BundleError::Metadata {
            status: output.status,
            said: text(cargo, &output.stderr)?.trim().to_owned(),
        });
    }
    let metadata: cargo_metadata::Metadata =
        crate::strictjson::decode_str(text(cargo, &output.stdout)?)
            .map_err(|source| BundleError::MetadataUnread { source })?;
    let mut found: Vec<Package> = metadata
        .workspace_packages()
        .into_iter()
        .map(Package::of)
        .collect();
    found.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(found)
}

/// Builds `binaries` of `package` in release for the target, and answers where cargo put each.
fn build(
    request: &Request<'_>,
    package: &Package,
    binaries: &[&str],
) -> Result<BTreeMap<String, PathBuf>, BundleError> {
    let mut building = command(request.cargo, request.root, request.environment);
    building
        .args(["build", "--locked", "--release"])
        .args(["--message-format", "json-render-diagnostics"])
        .args(["--target", request.target, "--package", &package.name])
        .arg("--manifest-path")
        .arg(request.root.join("Cargo.toml"))
        .stderr(Stdio::inherit());
    for binary in binaries {
        building.args(["--bin", binary]);
    }
    let output = building.output().map_err(|source| BundleError::Start {
        program: shown(request.cargo),
        source,
    })?;
    if !output.status.success() {
        return Err(BundleError::Build {
            package: package.name.clone(),
            target: request.target.to_owned(),
            status: output.status,
        });
    }
    let built = reported(package, text(request.cargo, &output.stdout)?)?;
    let declared: BTreeSet<&str> = binaries.iter().copied().collect();
    if built
        .keys()
        .map(String::as_str)
        .ne(declared.iter().copied())
    {
        return Err(BundleError::Unreported {
            package: package.name.clone(),
            built: built.keys().cloned().collect(),
            declared: declared.iter().map(|one| (*one).to_owned()).collect(),
        });
    }
    Ok(built)
}

/// Every binary of `package` cargo's build `messages` report an executable for.
fn reported(package: &Package, messages: &str) -> Result<BTreeMap<String, PathBuf>, BundleError> {
    let unread = |source| BundleError::Messages {
        package: package.name.clone(),
        source,
    };
    let mut found = BTreeMap::new();
    for line in messages.lines().filter(|line| !line.trim().is_empty()) {
        let message = crate::strictjson::from_str(line).map_err(unread)?;
        if message.get("reason").and_then(serde_json::Value::as_str) != Some("compiler-artifact") {
            continue;
        }
        let artifact: cargo_metadata::Artifact = serde_json::from_value(message).map_err(unread)?;
        if artifact.package_id.repr != package.id || !artifact.target.is_bin() {
            continue;
        }
        if let Some(executable) = artifact.executable {
            found.insert(artifact.target.name, executable.into_std_path_buf());
        }
    }
    Ok(found)
}

/// Asks the built `executable` of `program` its version, and refuses an answer that does not name the version its package has.
fn says(request: &Request<'_>, program: &Program, executable: &Path) -> Result<(), BundleError> {
    let output = command(executable.as_os_str(), request.root, request.environment)
        .arg("--version")
        .output()
        .map_err(|source| BundleError::Start {
            program: executable.display().to_string(),
            source,
        })?;
    let said = text(executable.as_os_str(), &output.stdout)?.trim();
    if output.status.success() && said.split_whitespace().any(|word| word == program.version) {
        return Ok(());
    }
    Err(BundleError::Version {
        binary: program.binary.clone(),
        status: output.status,
        said: said.to_owned(),
        version: program.version.clone(),
    })
}

/// What an entry of the archive is: its tar type, its mode, and the SHA-256 of what it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its tar type flag.
    pub kind: u8,
    /// Its mode.
    pub mode: u32,
    /// The SHA-256 of what it holds, in hex.
    pub digest: String,
}

impl Entry {
    /// An entry of tar type `kind` and `mode`, holding `bytes`.
    #[must_use]
    pub fn of(kind: tar::EntryType, mode: u32, bytes: &[u8]) -> Self {
        Self {
            kind: kind.as_byte(),
            mode,
            digest: hex::encode(Sha256::digest(bytes)),
        }
    }
}

/// The refusal of a write to `path`.
fn unwritten(path: &Path) -> impl FnOnce(std::io::Error) -> BundleError {
    let path = path.to_path_buf();
    move |source| BundleError::Write { path, source }
}

/// The refusal of a read of `path`.
fn unread(path: &Path) -> impl FnOnce(std::io::Error) -> BundleError {
    let path = path.to_path_buf();
    move |source| BundleError::Read { path, source }
}

/// A file in `out` that goes away unless it is persisted, readable by others as a release asset is.
fn staged_in(out: &Path) -> Result<tempfile::NamedTempFile, BundleError> {
    let mut builder = tempfile::Builder::new();
    builder.prefix(".bundle-");
    #[cfg(unix)]
    builder.permissions(std::os::unix::fs::PermissionsExt::from_mode(READABLE));
    builder.tempfile_in(out).map_err(unwritten(out))
}

/// Writes the archive and its checksum into the output directory, the archive last and only once it reads back as planned.
fn write(
    request: &Request<'_>,
    plan: &Plan,
    executables: &BTreeMap<String, PathBuf>,
) -> Result<Written, BundleError> {
    let out = request.out;
    std::fs::create_dir_all(out).map_err(unwritten(out))?;
    let archive = out.join(&plan.archive);
    let checksum = out.join(format!("{}.sha256", plan.archive));
    let mut staged = staged_in(out)?;
    let expected = pack(staged.as_file(), request.root, plan, executables)?;
    let staged_at = staged.path().to_path_buf();
    let mut bytes = Vec::new();
    staged
        .as_file_mut()
        .rewind()
        .and_then(|()| staged.as_file_mut().read_to_end(&mut bytes))
        .map_err(unread(&staged_at))?;
    read_back(&expected, &bytes)?;
    let digest = hex::encode(Sha256::digest(&bytes));
    let mut summed = staged_in(out)?;
    writeln!(summed.as_file_mut(), "{digest}  {}", plan.archive)
        .and_then(|()| summed.as_file_mut().sync_all())
        .map_err(unwritten(&checksum))?;
    summed
        .persist(&checksum)
        .map_err(|refused| unwritten(&checksum)(refused.error))?;
    staged
        .persist(&archive)
        .map_err(|refused| unwritten(&archive)(refused.error))?;
    Ok(Written {
        plan: plan.clone(),
        archive,
        checksum,
        digest,
    })
}

/// Writes the gzipped tar `plan` describes into `file`, and answers what each entry must read back as.
fn pack(
    file: &std::fs::File,
    root: &Path,
    plan: &Plan,
    executables: &BTreeMap<String, PathBuf>,
) -> Result<BTreeMap<String, Entry>, BundleError> {
    let archived = PathBuf::from(&plan.archive);
    let read = |path: &Path| std::fs::read(path).map_err(unread(path));
    let mut expected = BTreeMap::new();
    let mut archive = tar::Builder::new(flate2::write::GzEncoder::new(
        std::io::BufWriter::new(file),
        flate2::Compression::default(),
    ));
    for directory in &plan.directories {
        append(&mut archive, &format!("{directory}/"), EXECUTABLE, &[])
            .map_err(unwritten(&archived))?;
        expected.insert(
            directory.clone(),
            Entry::of(tar::EntryType::Directory, EXECUTABLE, &[]),
        );
    }
    for program in &plan.programs {
        let executable = executables
            .get(&program.path)
            .ok_or_else(|| BundleError::Unreported {
                package: program.package.clone(),
                built: Vec::new(),
                declared: vec![program.binary.clone()],
            })?;
        let bytes = read(executable)?;
        append(&mut archive, &program.path, EXECUTABLE, &bytes).map_err(unwritten(&archived))?;
        expected.insert(
            program.path.clone(),
            Entry::of(tar::EntryType::Regular, EXECUTABLE, &bytes),
        );
    }
    for document in &plan.documents {
        let bytes = read(&root.join(&document.name))?;
        append(&mut archive, &document.path, READABLE, &bytes).map_err(unwritten(&archived))?;
        expected.insert(
            document.path.clone(),
            Entry::of(tar::EntryType::Regular, READABLE, &bytes),
        );
    }
    archive
        .into_inner()
        .and_then(flate2::write::GzEncoder::finish)
        .and_then(|mut buffered| buffered.flush())
        .and_then(|()| file.sync_all())
        .map_err(unwritten(&archived))?;
    Ok(expected)
}

/// Appends one entry at `path`, a directory where it ends in a slash, owned by nobody and stamped with [`STAMP`].
fn append<W: std::io::Write>(
    archive: &mut tar::Builder<W>,
    path: &str,
    mode: u32,
    bytes: &[u8],
) -> std::io::Result<()> {
    let mut header = tar::Header::new_ustar();
    header.set_entry_type(if path.ends_with('/') {
        tar::EntryType::Directory
    } else {
        tar::EntryType::Regular
    });
    header.set_mode(mode);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(STAMP);
    header.set_size(u64::try_from(bytes.len()).map_err(std::io::Error::other)?);
    archive.append_data(&mut header, path, bytes)
}

/// Refuses the gzipped tar in `bytes` unless it holds exactly the `expected` entries, by path, directories without their trailing slash.
///
/// # Errors
/// [`BundleError::ReadBack`], naming every entry that is missing, differs, was never planned, or is there twice.
pub fn read_back(expected: &BTreeMap<String, Entry>, bytes: &[u8]) -> Result<(), BundleError> {
    compare(expected, &unpack(bytes)?)
}

/// Every entry of the gzipped tar in `bytes`, as it reads back, and every path it holds twice.
fn unpack(bytes: &[u8]) -> Result<(BTreeMap<String, Entry>, Vec<String>), BundleError> {
    let unreadable = |source: std::io::Error| BundleError::ReadBack {
        differences: vec![format!("it does not read as a gzipped tar: {source}")],
    };
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
    let mut found = BTreeMap::new();
    let mut twice = Vec::new();
    for entry in archive.entries().map_err(unreadable)? {
        let mut entry = entry.map_err(unreadable)?;
        let named = String::from_utf8(entry.path_bytes().into_owned()).map_err(|source| {
            BundleError::ReadBack {
                differences: vec![format!("an entry is not named in UTF-8: {source}")],
            }
        })?;
        let kind = entry.header().entry_type();
        let mode = entry.header().mode().map_err(unreadable)?;
        let mut content = Vec::new();
        entry.read_to_end(&mut content).map_err(unreadable)?;
        let path = match named.strip_suffix('/') {
            Some(directory) if kind.is_dir() => directory.to_owned(),
            Some(_) | None => named,
        };
        if found
            .insert(path.clone(), Entry::of(kind, mode, &content))
            .is_some()
        {
            twice.push(path);
        }
    }
    Ok((found, twice))
}

/// Refuses an archive that read back as anything but `expected`.
fn compare(
    expected: &BTreeMap<String, Entry>,
    (found, twice): &(BTreeMap<String, Entry>, Vec<String>),
) -> Result<(), BundleError> {
    let mut differences: Vec<String> = twice
        .iter()
        .map(|path| format!("{path} is in it twice"))
        .collect();
    for (path, entry) in expected {
        match found.get(path) {
            None => differences.push(format!("{path} is missing")),
            Some(read) if read != entry => {
                differences.push(format!("{path} reads back as {read:?}, not {entry:?}"));
            }
            Some(_) => {}
        }
    }
    differences.extend(
        found
            .keys()
            .filter(|path| !expected.contains_key(*path))
            .map(|path| format!("{path} was never planned")),
    );
    if differences.is_empty() {
        Ok(())
    } else {
        Err(BundleError::ReadBack { differences })
    }
}
