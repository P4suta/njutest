// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A retained source graph and its distinct actual original, changed and restored compiler owners.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

const SCHEMA: &str = "njutest-independent-compiler-pair-v2";
const CHANGE: &[u8] = b"\npub const A_THING_NOTHING_READS: u8 = 7;\n";
const MANIFEST: &[u8] = include_bytes!("../../../../fixtures/fixture-equivalent/Cargo.toml");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stamp {
    length: u64,
    modified: std::time::SystemTime,
    readonly: bool,
    #[cfg(unix)]
    mode: u32,
    #[cfg(unix)]
    changed: (i64, i64, u64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    stamp: Stamp,
    digest: String,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
struct Content<'a> {
    digest: &'a str,
    length: u64,
    readonly: bool,
    #[cfg(unix)]
    mode: u32,
}

impl File {
    const fn content(&self) -> Content<'_> {
        Content {
            digest: self.digest.as_str(),
            length: self.stamp.length,
            readonly: self.stamp.readonly,
            #[cfg(unix)]
            mode: self.stamp.mode,
        }
    }
}

impl PartialEq for File {
    fn eq(&self, other: &Self) -> bool {
        self.content() == other.content()
    }
}

impl Eq for File {}

fn stamp(path: &Path) -> io::Result<Stamp> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() {
        return Err(io::Error::other(format!(
            "the compiler input {} is not a regular file",
            path.display()
        )));
    }
    Ok(Stamp {
        length: metadata.len(),
        modified: metadata.modified()?,
        readonly: metadata.permissions().readonly(),
        #[cfg(unix)]
        mode: {
            use std::os::unix::fs::PermissionsExt as _;
            metadata.permissions().mode()
        },
        #[cfg(unix)]
        changed: {
            use std::os::unix::fs::MetadataExt as _;
            (metadata.ctime(), metadata.ctime_nsec(), metadata.ino())
        },
    })
}

fn file(path: &Path, previous: Option<&File>) -> io::Result<File> {
    let before = stamp(path)?;
    if let Some(previous) = previous
        && cfg!(unix)
        && previous.stamp == before
    {
        return Ok(previous.clone());
    }
    let digest = hex::encode(Sha256::digest(std::fs::read(path)?));
    if stamp(path)? != before {
        return Err(io::Error::other(format!(
            "the compiler input {} changed during observation",
            path.display()
        )));
    }
    Ok(File {
        stamp: before,
        digest,
    })
}

fn files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() {
                found.push(path);
            } else {
                return Err(io::Error::other(format!(
                    "the input {} has an opaque filesystem identity",
                    path.display()
                )));
            }
        }
    }
    found.sort();
    Ok(found)
}

fn environment() -> io::Result<BTreeMap<String, String>> {
    let mut held = BTreeMap::new();
    for (name, value) in crate::paths::environment_for_a_toolchain_run(&[]) {
        if [
            "NJUTEST_FIXTURE_BUILD_CACHE",
            "NJUTEST_TEST_COST_DIR",
            "NEXTEST_BINARY_ID",
            "NEXTEST_TEST_NAME",
        ]
        .iter()
        .any(|reserved| crate::paths::same_name(&name, OsStr::new(reserved)))
        {
            continue;
        }
        if crate::paths::same_name(&name, OsStr::new("RUSTC_WRAPPER")) && !value.is_empty() {
            return Err(io::Error::other("the compiler wrapper is an opaque input"));
        }
        let name = name.into_string().map_err(|name| {
            io::Error::other(format!(
                "non-textual compiler input encoded as {}",
                hex::encode(name.as_encoded_bytes())
            ))
        })?;
        let name = if cfg!(windows) {
            name.to_ascii_uppercase()
        } else {
            name
        };
        let value = value
            .into_string()
            .map_err(|_value| io::Error::other("a compiler input value is non-textual"))?;
        if held.insert(name, value).is_some() {
            return Err(io::Error::other("duplicate compiler environment identity"));
        }
    }
    held.insert("CARGO_INCREMENTAL".to_owned(), "0".to_owned());
    Ok(held)
}

fn value<'a>(env: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    env.get(name).map(String::as_str)
}

fn home(env: &BTreeMap<String, String>, name: &str, beside: &str) -> io::Result<PathBuf> {
    match value(env, name) {
        Some(path) => Ok(PathBuf::from(path)),
        None => value(env, "HOME")
            .or_else(|| value(env, "USERPROFILE"))
            .map(|path| Path::new(path).join(beside))
            .ok_or_else(|| io::Error::other(format!("the {name} input has no named home"))),
    }
}

fn toolchain(env: &BTreeMap<String, String>) -> io::Result<PathBuf> {
    let cargo = crate::paths::cargo_binary();
    let rustup = home(env, "RUSTUP_HOME", ".rustup")?;
    let installed = rustup.join("toolchains");
    if cargo.is_absolute()
        && let Some(root) = cargo.parent().and_then(Path::parent)
        && root.parent() == Some(installed.as_path())
    {
        return Ok(root.to_path_buf());
    }
    let proxy = home(env, "CARGO_HOME", ".cargo")?
        .join("bin")
        .join(format!("rustup{}", std::env::consts::EXE_SUFFIX));
    if file(&cargo, None)?.digest != file(&proxy, None)?.digest {
        return Err(io::Error::other(
            "the Cargo selector is an opaque executable",
        ));
    }
    let chosen = value(env, "RUSTUP_TOOLCHAIN")
        .ok_or_else(|| io::Error::other("the compiler selection is not explicit"))?;
    let mut selected = Vec::new();
    for entry in std::fs::read_dir(installed)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| io::Error::other("non-textual toolchain identity"))?;
        if name == chosen || name.starts_with(&format!("{chosen}-")) {
            selected.push(entry.path());
        }
    }
    match selected.as_slice() {
        [selected] => Ok(selected.clone()),
        [] => Err(io::Error::other(
            "the declared compiler has not been installed",
        )),
        [_, _, ..] => Err(io::Error::other("the compiler selection is ambiguous")),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Inputs {
    source: Vec<(String, String)>,
    environment: BTreeMap<String, String>,
    compiler: PathBuf,
    files: BTreeMap<PathBuf, File>,
}

impl Inputs {
    fn of(source: &Path, owner: &Path, previous: Option<&Self>) -> io::Result<Self> {
        plain(source)?;
        let mut environment = environment()?;
        let compiler = toolchain(&environment)?;
        for (name, program) in [("RUSTC", "rustc"), ("RUSTDOC", "rustdoc")] {
            environment.insert(
                name.to_owned(),
                text(
                    &compiler
                        .join("bin")
                        .join(format!("{program}{}", std::env::consts::EXE_SUFFIX)),
                )?,
            );
        }
        let mut paths = BTreeSet::new();
        for program in ["cargo", "rustc", "rustdoc"] {
            paths.insert(
                compiler
                    .join("bin")
                    .join(format!("{program}{}", std::env::consts::EXE_SUFFIX)),
            );
        }
        compiler_files(&compiler, &mut paths)?;
        paths.extend(files(source)?);
        #[cfg(windows)]
        for path in files(&compiler.join("bin"))? {
            if path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("dll"))
            {
                paths.insert(path);
            }
        }
        configuration(owner, &environment, &mut paths)?;
        let mut current = BTreeMap::new();
        for path in paths {
            let old = previous.and_then(|previous| previous.files.get(&path));
            current.insert(path.clone(), file(&path, old)?);
        }
        Ok(Self {
            source: crate::fixture::fingerprint(source),
            environment,
            compiler,
            files: current,
        })
    }

    fn key(&self) -> io::Result<String> {
        let files: BTreeMap<_, _> = self
            .files
            .iter()
            .map(|(path, file)| (path, file.content()))
            .collect();
        Ok(hex::encode(Sha256::digest(
            serde_json::to_vec(&(
                SCHEMA,
                &self.source,
                &self.environment,
                &self.compiler,
                files,
            ))
            .map_err(io::Error::other)?,
        )))
    }

    #[expect(
        unused_results,
        reason = "Command's infallible builder API returns self at this explicit configuration boundary"
    )]
    fn command(&self, root: &Path, target: &Path) -> Command {
        let mut command = Command::new(
            self.compiler
                .join("bin")
                .join(format!("cargo{}", std::env::consts::EXE_SUFFIX)),
        );
        command
            .env_clear()
            .envs(&self.environment)
            .args([
                "test",
                "--no-run",
                "--message-format=json",
                "--offline",
                "--locked",
            ])
            .arg("--target-dir")
            .arg(target)
            .current_dir(root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }
}

fn text(path: &Path) -> io::Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| io::Error::other(format!("non-textual compiler path {}", path.display())))
}

fn configuration(
    root: &Path,
    env: &BTreeMap<String, String>,
    paths: &mut BTreeSet<PathBuf>,
) -> io::Result<()> {
    let cargo_home = home(env, "CARGO_HOME", ".cargo")?;
    for directory in root
        .ancestors()
        .map(|path| path.join(".cargo"))
        .chain(std::iter::once(cargo_home))
    {
        for name in ["config", "config.toml"] {
            let path = directory.join(name);
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(source) if source.kind() == io::ErrorKind::NotFound => continue,
                Err(source) => return Err(source),
            };
            let configuration = std::str::from_utf8(&bytes).map_err(io::Error::other)?;
            let mut section = "";
            for line in configuration.lines().map(str::trim) {
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if line.starts_with('[') {
                    section = line;
                } else if !matches!(section, "[alias]" | "[net]" | "[term]") {
                    return Err(io::Error::other(format!(
                        "the {} input has an opaque compiler configuration",
                        path.display()
                    )));
                }
            }
            paths.insert(path);
        }
    }
    Ok(())
}

fn compiler_files(compiler: &Path, paths: &mut BTreeSet<PathBuf>) -> io::Result<()> {
    for entry in std::fs::read_dir(compiler.join("lib"))? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            paths.insert(entry.path());
        }
    }
    let name = compiler
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| io::Error::other("the compiler has no installed host identity"))?;
    let mut hosts = Vec::new();
    for entry in std::fs::read_dir(compiler.join("lib/rustlib"))? {
        let entry = entry?;
        if entry.file_type()?.is_dir()
            && entry
                .file_name()
                .to_str()
                .is_some_and(|host| name.ends_with(host))
        {
            hosts.push(entry.path());
        }
    }
    let [host] = hosts.as_slice() else {
        return Err(io::Error::other(
            "the actual compiler host graph is ambiguous",
        ));
    };
    for directory in [host.join("lib"), host.join("bin")] {
        paths.extend(files(&directory)?);
    }
    Ok(())
}

fn plain(source: &Path) -> io::Result<()> {
    if std::fs::read(source.join("Cargo.toml"))? != MANIFEST {
        return Err(io::Error::other(
            "the reproducibility fixture has an unbound manifest",
        ));
    }
    for path in files(source)? {
        if path.file_name() == Some(OsStr::new("build.rs")) {
            return Err(io::Error::other(
                "an implicit build script has unbound inputs",
            ));
        }
        if path.extension() == Some(OsStr::new("rs")) {
            use syn::visit::Visit as _;
            let source = std::fs::read_to_string(&path)?;
            let parsed = crate::lexed::file(&source).map_err(io::Error::other)?;
            let mut inputs = SourceInputs { external: false };
            inputs.visit_file(&parsed);
            if inputs.external {
                return Err(io::Error::other(format!(
                    "the {} source has an external compiler input",
                    path.display()
                )));
            }
        }
    }
    Ok(())
}

struct SourceInputs {
    external: bool,
}

impl<'ast> syn::visit::Visit<'ast> for SourceInputs {
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let tokens: String = mac
            .tokens
            .to_string()
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();
        self.external |= mac.path.is_ident("macro_rules")
            || [
                "include!",
                "include_str!",
                "include_bytes!",
                "env!",
                "option_env!",
            ]
            .iter()
            .any(|name| tokens.contains(name));
        self.external |= mac.path.segments.last().is_some_and(|segment| {
            [
                "include",
                "include_str",
                "include_bytes",
                "env",
                "option_env",
            ]
            .iter()
            .any(|name| segment.ident == *name)
        });
        syn::visit::visit_macro(self, mac);
    }

    fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
        self.external |= attribute.path().is_ident("path")
            || attribute.path().is_ident("doc")
                && !matches!(
                    &attribute.meta,
                    syn::Meta::NameValue(syn::MetaNameValue {
                        value: syn::Expr::Lit(syn::ExprLit {
                            lit: syn::Lit::Str(_),
                            ..
                        }),
                        ..
                    })
                );
        syn::visit::visit_attribute(self, attribute);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Stage {
    Original,
    Changed,
    Restored,
}

impl Stage {
    const fn name(self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::Changed => "changed",
            Self::Restored => "independent-restored",
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Actual {
    stage: Stage,
    identity: String,
    pid: u32,
    kernel: String,
    argv: Vec<String>,
    environment: String,
    root: PathBuf,
    source: Vec<(String, String)>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_digest: String,
    stderr_digest: String,
    products: PathBuf,
    files: BTreeMap<PathBuf, File>,
}

fn arguments(command: &Command) -> io::Result<Vec<String>> {
    std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|argument| {
            argument
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| io::Error::other("a compiler argument is not textual"))
        })
        .collect()
}

fn fresh(target: &Path) -> io::Result<()> {
    let root = target.join("debug/.fingerprint");
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(source) => return Err(source),
    };
    for entry in entries {
        let entry = entry?;
        if !entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with("fixture-equivalent-"))
        {
            return Err(io::Error::other(
                "the compiler witness has an undeclared package",
            ));
        }
        for path in files(&entry.path())? {
            std::fs::remove_file(path)?;
        }
    }
    Ok(())
}

fn outputs(stdout: &[u8], root: &Path, target: &Path) -> io::Result<BTreeSet<PathBuf>> {
    let mut executable = false;
    let mut products = BTreeSet::new();
    let mut completed = false;
    let mut units = 0_u64;
    for message in cargo_metadata::Message::parse_stream(io::Cursor::new(stdout)) {
        match message? {
            cargo_metadata::Message::CompilerArtifact(artifact) => {
                if artifact.fresh
                    || njutest_fixture_tree::filesystem_spelling(
                        artifact.target.src_path.as_std_path(),
                    ) != njutest_fixture_tree::filesystem_spelling(&root.join("src/lib.rs"))
                {
                    return Err(io::Error::other(
                        "the independent witness did not read its original source",
                    ));
                }
                units = units
                    .checked_add(1)
                    .ok_or_else(|| io::Error::other("compiler unit count overflow"))?;
                for path in artifact.filenames {
                    let path = path.into_std_path_buf();
                    if !path.starts_with(target) {
                        return Err(io::Error::other("the compiler returned an unowned product"));
                    }
                    products.insert(path.clone());
                    let stem = path
                        .file_stem()
                        .and_then(OsStr::to_str)
                        .ok_or_else(|| io::Error::other("an unnamed compiler artifact"))?;
                    let stem = match stem.strip_prefix("libfixture_equivalent") {
                        Some(rest) => format!("fixture_equivalent{rest}"),
                        None => stem.to_owned(),
                    };
                    products.insert(path.with_file_name(format!("{stem}.d")));
                }
                executable |= artifact.executable.is_some();
            }
            cargo_metadata::Message::BuildFinished(finished) => completed = finished.success,
            cargo_metadata::Message::CompilerMessage(_diagnostic) => {}
            cargo_metadata::Message::BuildScriptExecuted(_script) => {
                return Err(io::Error::other(
                    "a build script cannot establish the fixed compiler pair",
                ));
            }
            cargo_metadata::Message::TextLine(_text) => {
                return Err(io::Error::other(
                    "the compiler witness output is not complete Cargo JSON",
                ));
            }
            _ => {
                return Err(io::Error::other(
                    "an unknown Cargo output cannot certify a compiler pair",
                ));
            }
        }
    }
    if !completed || units == 0 || !executable {
        return Err(io::Error::other(
            "the actual compiler has no complete executable witness",
        ));
    }
    Ok(products)
}

fn inventory(
    products: BTreeSet<PathBuf>,
    target: &Path,
    directory: &Path,
) -> io::Result<BTreeMap<PathBuf, File>> {
    let mut held = BTreeMap::new();
    for product in products {
        let relative = product
            .strip_prefix(target)
            .map_err(io::Error::other)?
            .to_path_buf();
        let copied = directory.join(&relative);
        std::fs::create_dir_all(
            copied
                .parent()
                .ok_or_else(|| io::Error::other("a product parent"))?,
        )?;
        let original = file(&product, None)?.digest;
        std::fs::copy(&product, &copied)?;
        if file(&product, None)?.digest != original || file(&copied, None)?.digest != original {
            return Err(io::Error::other(
                "the compiler product changed during immutable publication",
            ));
        }
        held.insert(relative, file(&copied, None)?);
    }
    Ok(held)
}

impl Actual {
    fn run(
        stage: Stage,
        inputs: &Inputs,
        (root, target, parent): (&Path, &Path, &Path),
    ) -> io::Result<Self> {
        fresh(target)?;
        let source = crate::fixture::fingerprint(root);
        let directory = tempfile::Builder::new()
            .prefix(stage.name())
            .tempdir_in(parent)?;
        let identity = directory
            .path()
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or_else(|| io::Error::other("an actual compiler owner has no identity"))?
            .to_owned();
        let mut command = inputs.command(root, target);
        let argv = arguments(&command)?;
        let began = std::time::Instant::now();
        let child = match crate::process::SupervisedChild::launch(&mut command) {
            Ok(child) => child,
            Err(source) => {
                let work = crate::cost::Work::direct(
                    &identity,
                    crate::cost::DirectLaunch::Failed {
                        cause: source.to_string(),
                    },
                );
                crate::cost::record(
                    root,
                    &serde_json::to_value(&work).map_err(io::Error::other)?,
                    &serde_json::Value::Null,
                )?;
                return Err(io::Error::other(source));
            }
        };
        let pid = child
            .id()
            .filter(|pid| *pid != 0)
            .ok_or_else(|| io::Error::other("the actual compiler has no live kernel identity"))?;
        let process = njutest_process::ForeignProcess::retain(pid)?
            .ok_or_else(|| io::Error::other("the actual compiler generation cannot be retained"))?;
        let kernel = process.identity().token();
        let output = child.wait_with_output().map_err(io::Error::other)?;
        drop(process);
        crate::cost::build(
            root,
            &format!("reproducible::{identity}"),
            began.elapsed(),
            &output.stdout,
        )?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "actual {} compiler refusal: {}",
                stage.name(),
                crate::process::strict_utf8(&output.stderr)
            )));
        }
        if crate::fixture::fingerprint(root) != source {
            return Err(io::Error::other(
                "the compiler source changed during its actual witness",
            ));
        }
        let products = outputs(&output.stdout, root, target)?;
        let inventory = inventory(products, target, directory.path())?;
        Ok(Self {
            stage,
            identity,
            pid,
            kernel,
            argv,
            environment: hex::encode(Sha256::digest(
                serde_json::to_vec(&inputs.environment).map_err(io::Error::other)?,
            )),
            root: root.to_path_buf(),
            source,
            stdout_digest: hex::encode(Sha256::digest(&output.stdout)),
            stderr_digest: hex::encode(Sha256::digest(&output.stderr)),
            stdout: output.stdout,
            stderr: output.stderr,
            products: directory.keep(),
            files: inventory,
        })
    }

    fn verifies(
        &self,
        inputs: &Inputs,
        (root, target): (&Path, &Path),
        stage: Stage,
    ) -> io::Result<super::Built> {
        if self.stage != stage
            || self.pid == 0
            || self.kernel.is_empty()
            || self.root != root
            || self.argv != arguments(&inputs.command(root, target))?
            || self.environment
                != hex::encode(Sha256::digest(
                    serde_json::to_vec(&inputs.environment).map_err(io::Error::other)?,
                ))
            || self.stdout_digest != hex::encode(Sha256::digest(&self.stdout))
            || self.stderr_digest != hex::encode(Sha256::digest(&self.stderr))
        {
            return Err(io::Error::other(
                "the compiler pair has changed actual provenance",
            ));
        }
        let expected = outputs(&self.stdout, root, target)?;
        let expected: BTreeSet<_> = expected
            .into_iter()
            .map(|path| {
                path.strip_prefix(target)
                    .map(Path::to_path_buf)
                    .map_err(io::Error::other)
            })
            .collect::<io::Result<_>>()?;
        if expected != self.files.keys().cloned().collect::<BTreeSet<_>>() {
            return Err(io::Error::other(
                "the compiler pair inventory is incomplete",
            ));
        }
        let mut programs = BTreeMap::new();
        for (relative, original) in &self.files {
            if relative.is_absolute()
                || relative
                    .components()
                    .any(|component| component == std::path::Component::ParentDir)
                || file(&self.products.join(relative), None)? != *original
            {
                return Err(io::Error::other(
                    "the independent compiler products changed",
                ));
            }
        }
        for message in cargo_metadata::Message::parse_stream(io::Cursor::new(&self.stdout)) {
            if let cargo_metadata::Message::CompilerArtifact(artifact) = message?
                && let Some(executable) = artifact.executable
            {
                let relative = executable
                    .as_std_path()
                    .strip_prefix(target)
                    .map_err(io::Error::other)?;
                let original = self
                    .files
                    .get(relative)
                    .ok_or_else(|| io::Error::other("a missing original executable"))?;
                if programs
                    .insert(artifact.target.name, original.digest.clone())
                    .is_some()
                {
                    return Err(io::Error::other("duplicate original executable identity"));
                }
            }
        }
        Ok(programs)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pair {
    schema: String,
    key: String,
    inputs: Inputs,
    original: Actual,
    changed: Actual,
    restored: Actual,
}

impl Pair {
    fn answer(
        &self,
        inputs: &Inputs,
        (root, target, products): (&Path, &Path, &Path),
    ) -> io::Result<bool> {
        if self.schema != SCHEMA
            || self.key != inputs.key()?
            || self.inputs != *inputs
            || !distinct(&self.original, &self.changed, &self.restored)
        {
            return Err(io::Error::other(
                "the compiler control is not an independent complete pair",
            ));
        }
        let mut changed = inputs.source.clone();
        let mut source = std::fs::read(root.join("src/lib.rs"))?;
        source.extend_from_slice(CHANGE);
        let changed_source = changed
            .iter_mut()
            .find(|(path, _digest)| path == "src/lib.rs")
            .ok_or_else(|| io::Error::other("the complete graph has no original source"))?;
        changed_source.1 = hex::encode(Sha256::digest(source));
        if self.original.source != inputs.source
            || self.changed.source != changed
            || self.restored.source != inputs.source
            || [&self.original, &self.changed, &self.restored]
                .iter()
                .any(|actual| {
                    actual.products.parent() != Some(products)
                        || actual.products.file_name().and_then(OsStr::to_str)
                            != Some(actual.identity.as_str())
                })
        {
            return Err(io::Error::other(
                "the three actual witnesses read another source graph",
            ));
        }
        let original = self
            .original
            .verifies(inputs, (root, target), Stage::Original)?;
        let changed = self
            .changed
            .verifies(inputs, (root, target), Stage::Changed)?;
        if changed.is_empty() {
            return Err(io::Error::other(
                "the changed source has no real compiler witness",
            ));
        }
        let restored = self
            .restored
            .verifies(inputs, (root, target), Stage::Restored)?;
        Ok(!original.is_empty() && original == restored)
    }
}

fn distinct(original: &Actual, changed: &Actual, restored: &Actual) -> bool {
    [
        (original, changed),
        (original, restored),
        (changed, restored),
    ]
    .iter()
    .all(|(left, right)| left.identity != right.identity && left.kernel != right.kernel)
}

struct Restoration {
    path: PathBuf,
    bytes: Vec<u8>,
    changed: bool,
}

impl Restoration {
    fn of(root: &Path) -> io::Result<Self> {
        let path = root.join("src/lib.rs");
        Ok(Self {
            bytes: std::fs::read(&path)?,
            path,
            changed: false,
        })
    }

    fn change(&mut self) -> io::Result<()> {
        self.changed = true;
        let mut changed = self.bytes.clone();
        changed.extend_from_slice(CHANGE);
        std::fs::write(&self.path, changed)
    }

    fn restore(&mut self) -> io::Result<()> {
        std::fs::write(&self.path, &self.bytes)?;
        self.changed = false;
        Ok(())
    }
}

impl Drop for Restoration {
    fn drop(&mut self) {
        if self.changed
            && let Err(source) = self.restore()
        {
            eprintln!(
                "the owned source {} was not restored: {source}",
                self.path.display()
            );
            std::process::abort();
        }
    }
}

fn record(path: &Path) -> io::Result<Pair> {
    crate::strictjson::decode_slice(&std::fs::read(path)?).map_err(io::Error::other)
}

fn publish(path: &Path, pair: &Pair) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("the compiler pair has no retained owner"))?;
    let mut pending = tempfile::NamedTempFile::new_in(parent)?;
    pending.write_all(&serde_json::to_vec(pair).map_err(io::Error::other)?)?;
    pending.persist(path).map_err(io::Error::other)?;
    Ok(())
}

fn root() -> io::Result<PathBuf> {
    let retained = crate::paths::environment_for_a_toolchain_run(&[])
        .into_iter()
        .find(|(name, _value)| {
            crate::paths::same_name(name, OsStr::new("NJUTEST_FIXTURE_BUILD_CACHE"))
        })
        .map(|(_name, value)| PathBuf::from(value))
        .ok_or_else(|| io::Error::other("the compiler pair has no explicit retained owner"))?;
    if !retained.is_absolute() {
        return Err(io::Error::other("the compiler pair owner must be absolute"));
    }
    let root = retained.join("independent-compiler-pairs-v1");
    std::fs::create_dir_all(&root)?;
    std::fs::canonicalize(root)
}

/// How many leading hex digits of a pair's key name the directory whose `pair.json` keeps the whole key.
const NAME_LENGTH: usize = 16;

/// The directory one key's pair lives in, named by a short prefix of the key so its deepest build product stays inside the Windows linker's path limit.
fn owner(root: &Path, key: &str) -> io::Result<PathBuf> {
    key.get(..NAME_LENGTH)
        .map(|prefix| root.join(prefix))
        .ok_or_else(|| io::Error::other("a compiler pair key is shorter than its directory name"))
}

fn lease(root: &Path) -> io::Result<std::fs::File> {
    let lease = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("preparation.lock"))?;
    let began = std::time::Instant::now();
    let locked = lease.lock();
    let wait = crate::cost::HostWait {
        owner: root.display().to_string(),
        cause: "independent compiler pair preparation".to_owned(),
        elapsed_ns: u64::try_from(began.elapsed().as_nanos()).map_err(io::Error::other)?,
        machine: crate::cost::WaitMachine {
            os: std::env::consts::OS.to_owned(),
            cpus: u64::try_from(std::thread::available_parallelism()?.get())
                .map_err(io::Error::other)?,
        },
    };
    let mut work = crate::cost::Work::none();
    work.host_waits.push(wait);
    crate::cost::record(
        root,
        &serde_json::to_value(work).map_err(io::Error::other)?,
        &serde_json::Value::Null,
    )?;
    locked?;
    Ok(lease)
}

fn prepared(inputs: &Inputs, (source, root): (&Path, &Path)) -> io::Result<()> {
    let current = match std::fs::metadata(root) {
        Ok(metadata) if metadata.is_dir() => Some(crate::fixture::fingerprint(root)),
        Ok(_) => {
            return Err(io::Error::other(
                "the retained source graph is not a directory",
            ));
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => None,
        Err(source) => return Err(source),
    };
    if current.as_ref() == Some(&inputs.source) {
        return Ok(());
    }
    if current.is_some() {
        return Err(io::Error::other(
            "the immutable original source graph changed",
        ));
    }
    crate::fixture::copy_tree(source, root);
    if crate::fixture::fingerprint(root) != inputs.source
        || crate::fixture::fingerprint(source) != inputs.source
    {
        return Err(io::Error::other(
            "the source graph changed while its owner copied it",
        ));
    }
    Ok(())
}

pub(super) fn answer() -> io::Result<bool> {
    let root = root()?;
    let source = crate::paths::fixtures_dir().join("fixture-equivalent");
    answer_in(&root, &source)
}

fn answer_in(root: &Path, source: &Path) -> io::Result<bool> {
    let preparation = lease(root)?;
    let latest = record(&root.join("latest.json"));
    let previous = match &latest {
        Ok(pair) => Some(&pair.inputs),
        Err(source) => {
            eprintln!("compiler-reproducibility-prior: {source}");
            None
        }
    };
    let inputs = Inputs::of(source, root, previous)?;
    let owner = owner(root, &inputs.key()?)?;
    std::fs::create_dir_all(&owner)?;
    let original = owner.join("source");
    let target = owner.join("target");
    let products = owner.join("products");
    std::fs::create_dir_all(&products)?;
    prepared(&inputs, (source, &original))?;
    match record(&owner.join("pair.json")) {
        Ok(pair) => match pair.answer(&inputs, (&original, &target, &products)) {
            Ok(answer) => {
                println!(
                    "compiler-reproducibility-reuse {} original={} control={}",
                    pair.key, pair.original.identity, pair.restored.identity
                );
                drop(preparation);
                return Ok(answer);
            }
            Err(source) => eprintln!("compiler-reproducibility-miss: {source}"),
        },
        Err(source) => eprintln!("compiler-reproducibility-miss: {source}"),
    }
    let mut restoration = Restoration::of(&original)?;
    let first = Actual::run(Stage::Original, &inputs, (&original, &target, &products))?;
    restoration.change()?;
    let changed = Actual::run(Stage::Changed, &inputs, (&original, &target, &products))?;
    restoration.restore()?;
    let restored = Actual::run(Stage::Restored, &inputs, (&original, &target, &products))?;
    if Inputs::of(source, root, Some(&inputs))? != inputs {
        return Err(io::Error::other(
            "complete compiler inputs changed while producing their witnesses",
        ));
    }
    let pair = Pair {
        schema: SCHEMA.to_owned(),
        key: inputs.key()?,
        inputs: inputs.clone(),
        original: first,
        changed,
        restored,
    };
    let answer = pair.answer(&inputs, (&original, &target, &products))?;
    publish(&owner.join("pair.json"), &pair)?;
    publish(&root.join("latest.json"), &pair)?;
    println!(
        "compiler-reproducibility-pair {} original={} control={}",
        pair.key, pair.original.identity, pair.restored.identity
    );
    drop(restoration);
    drop(preparation);
    Ok(answer)
}

#[cfg(test)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "these behavior controls assert after fallible real filesystem and compiler setup"
)]
mod tests {
    use super::{Inputs, Pair, answer_in, file, record};
    use sha2::Digest as _;
    use std::io;

    #[test]
    fn a_pair_directory_spells_sixteen_digits_of_the_key_its_record_keeps_whole() -> io::Result<()>
    {
        let root = std::path::Path::new("pairs");
        let key = "0123456789abcdef".repeat(4);
        assert_eq!(
            super::owner(root, &key)?,
            root.join("0123456789abcdef"),
            "a pair's directory spells 16 of its key's 64 digits, and pair.json keeps all 64"
        );
        let refused = super::owner(root, "0123456789abcde")
            .expect_err("a key shorter than a name names no directory");
        assert!(refused.to_string().contains("shorter"), "{refused}");
        Ok(())
    }

    #[test]
    fn restored_input_bytes_recover_their_semantic_identity() -> io::Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("compiler-input");
        std::fs::write(&path, b"input A")?;
        let original = file(&path, None)?;
        std::fs::write(&path, b"input B")?;
        let changed = file(&path, Some(&original))?;
        assert_ne!(
            original, changed,
            "changed input bytes withdraw the original identity"
        );
        std::fs::write(&path, b"input A")?;
        let restored = file(&path, Some(&changed))?;
        assert_eq!(
            original, restored,
            "restored input bytes recover their original identity after verified reobservation"
        );
        Ok(())
    }

    #[test]
    fn a_retained_pair_requires_three_distinct_actual_compiler_producers() -> io::Result<()> {
        let temporary = tempfile::tempdir()?;
        let directory = std::fs::canonicalize(temporary.path())?;
        let source = crate::paths::fixtures_dir().join("fixture-equivalent");
        assert_eq!(
            answer_in(&directory, &source)?,
            super::super::REVERTED_CHANGE_REPRODUCES
        );
        let mut pair = record(&directory.join("latest.json"))?;
        let inputs = Inputs::of(&source, &directory, Some(&pair.inputs))?;
        let owner = super::owner(&directory, &inputs.key()?)?;
        pair.changed.pid = pair.original.pid;
        pair.changed.kernel.clone_from(&pair.original.kernel);
        assert!(
            pair.answer(
                &inputs,
                (
                    &owner.join("source"),
                    &owner.join("target"),
                    &owner.join("products")
                ),
            )
            .is_err(),
            "a changed-source witness cannot reuse another actual producer identity"
        );
        drop(temporary);
        Ok(())
    }

    #[derive(Debug, Clone, Copy)]
    enum Tampering {
        OriginalControl,
        Fresh,
        RawOutput,
        Arguments,
        Environment,
        Source,
        Inventory,
        KernelGeneration,
    }

    impl Tampering {
        const fn next(self) -> Option<Self> {
            match self {
                Self::OriginalControl => Some(Self::Fresh),
                Self::Fresh => Some(Self::RawOutput),
                Self::RawOutput => Some(Self::Arguments),
                Self::Arguments => Some(Self::Environment),
                Self::Environment => Some(Self::Source),
                Self::Source => Some(Self::Inventory),
                Self::Inventory => Some(Self::KernelGeneration),
                Self::KernelGeneration => None,
            }
        }

        fn plant(self, pair: &mut Pair, original: &std::path::Path) -> io::Result<()> {
            match self {
                Self::OriginalControl => pair.restored = record(original)?.original,
                Self::Fresh => {
                    let mut stdout = Vec::new();
                    let mut changed = false;
                    for line in pair.restored.stdout.split(|byte| *byte == b'\n') {
                        if line.is_empty() {
                            continue;
                        }
                        let mut message: serde_json::Value =
                            crate::strictjson::decode_slice(line).map_err(io::Error::other)?;
                        if message.get("reason").and_then(serde_json::Value::as_str)
                            == Some("compiler-artifact")
                        {
                            *message.get_mut("fresh").expect("the actual fresh bit") =
                                serde_json::Value::Bool(true);
                            changed = true;
                        }
                        stdout.extend(serde_json::to_vec(&message).map_err(io::Error::other)?);
                        stdout.push(b'\n');
                    }
                    assert!(changed, "the real compiler emitted an artifact");
                    pair.restored.stdout_digest = hex::encode(super::Sha256::digest(&stdout));
                    pair.restored.stdout = stdout;
                }
                Self::RawOutput => pair.restored.stdout.push(b'!'),
                Self::Arguments => pair.restored.argv.push("--release".to_owned()),
                Self::Environment => pair.restored.environment.push('0'),
                Self::Source => pair.restored.source = pair.changed.source.clone(),
                Self::Inventory => {
                    assert!(pair.restored.files.pop_first().is_some());
                }
                Self::KernelGeneration => pair.restored.kernel.clone_from(&pair.original.kernel),
            }
            Ok(())
        }
    }

    #[test]
    fn a_retained_pair_verifies_every_actual_provenance_input() -> io::Result<()> {
        let temporary = tempfile::tempdir()?;
        let directory = std::fs::canonicalize(temporary.path())?;
        let source = crate::paths::fixtures_dir().join("fixture-equivalent");
        assert_eq!(
            answer_in(&directory, &source)?,
            super::super::REVERTED_CHANGE_REPRODUCES
        );
        let original = directory.join("latest.json");
        let actual = record(&original)?;
        let inputs = Inputs::of(&source, &directory, Some(&actual.inputs))?;
        let owner = super::owner(&directory, &inputs.key()?)?;
        let paths = (
            owner.join("source"),
            owner.join("target"),
            owner.join("products"),
        );
        assert_eq!(
            actual.answer(&inputs, (&paths.0, &paths.1, &paths.2))?,
            super::super::REVERTED_CHANGE_REPRODUCES
        );
        let mut remaining = Some(Tampering::OriginalControl);
        while let Some(alteration) = remaining {
            let mut pair = record(&original)?;
            alteration.plant(&mut pair, &original)?;
            assert!(
                pair.answer(&inputs, (&paths.0, &paths.1, &paths.2))
                    .is_err(),
                "altered actual compiler provenance remains refused: {alteration:?}"
            );
            remaining = alteration.next();
        }
        let relative = actual
            .restored
            .files
            .keys()
            .next()
            .expect("actual products");
        let product = actual.restored.products.join(relative);
        let bytes = std::fs::read(&product)?;
        std::fs::write(&product, b"altered actual compiler product")?;
        let refusal = actual
            .answer(&inputs, (&paths.0, &paths.1, &paths.2))
            .expect_err("altered actual compiler products remain refused");
        assert_eq!(refusal.kind(), io::ErrorKind::Other);
        std::fs::write(&product, bytes)?;
        assert_eq!(
            actual.answer(&inputs, (&paths.0, &paths.1, &paths.2))?,
            super::super::REVERTED_CHANGE_REPRODUCES
        );
        drop(temporary);
        Ok(())
    }

    #[test]
    fn a_restored_source_graph_reuses_the_original_actual_pair() -> io::Result<()> {
        let temporary = tempfile::tempdir()?;
        let directory = std::fs::canonicalize(temporary.path())?;
        let fixture = crate::fixture::Fixture::copy("fixture-equivalent");
        let source = fixture.root();
        assert_eq!(
            answer_in(&directory, source)?,
            super::super::REVERTED_CHANGE_REPRODUCES
        );
        let original = record(&directory.join("latest.json"))?;
        let bytes = fixture.read("src/lib.rs");
        let changed = std::str::from_utf8(&bytes)
            .map_err(io::Error::other)?
            .replace("n * 2", "n / 2");
        fixture.write("src/lib.rs", changed.as_bytes());
        assert_eq!(
            answer_in(&directory, source)?,
            super::super::REVERTED_CHANGE_REPRODUCES
        );
        let different = record(&directory.join("latest.json"))?;
        assert_ne!(original.key, different.key);
        assert_ne!(original.original.identity, different.original.identity);
        fixture.write("src/lib.rs", &bytes);
        assert_eq!(
            answer_in(&directory, source)?,
            super::super::REVERTED_CHANGE_REPRODUCES
        );
        let current = Inputs::of(source, &directory, Some(&different.inputs))?;
        assert_eq!(current.key()?, original.key);
        let retained = record(&super::owner(&directory, &current.key()?)?.join("pair.json"))?;
        assert_eq!(retained.original.identity, original.original.identity);
        assert_eq!(retained.changed.identity, original.changed.identity);
        assert_eq!(retained.restored.identity, original.restored.identity);
        drop(temporary);
        Ok(())
    }

    #[test]
    fn concurrent_questions_retain_one_actual_independent_pair() -> io::Result<()> {
        let temporary = tempfile::tempdir()?;
        let directory = std::fs::canonicalize(temporary.path())?;
        let source = crate::paths::fixtures_dir().join("fixture-equivalent");
        let started = std::sync::Barrier::new(3);
        std::thread::scope(|scope| {
            let workers: Vec<_> = (0..3)
                .map(|_request| {
                    crate::thread::ScopedThread::launch(scope, || {
                        started.wait();
                        answer_in(&directory, &source)
                    })
                })
                .collect();
            for worker in workers {
                assert_eq!(
                    worker.join().map_err(io::Error::other)??,
                    super::super::REVERTED_CHANGE_REPRODUCES
                );
            }
            Ok::<(), io::Error>(())
        })?;
        let pair = record(&directory.join("latest.json"))?;
        let owner = super::owner(&directory, &pair.key)?;
        let mut observations = Vec::new();
        for entry in std::fs::read_dir(owner.join("products"))? {
            observations.push(entry?.file_name());
        }
        assert_eq!(observations.len(), 3, "the one real three-process pair");
        for actual in [pair.original, pair.changed, pair.restored] {
            assert!(observations.contains(&std::ffi::OsString::from(actual.identity)));
        }
        drop(temporary);
        Ok(())
    }
}
