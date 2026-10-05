// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Toolchain content identities memoized only while their filesystem change stamps agree with stamps that had settled when they were taken.

use std::collections::BTreeMap;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use njutest_fixture_tree::settled::{self, Taken};
use sha2::{Digest as _, Sha256};

use super::File;
use crate::cargo::{CompileOptions, Toolchain};
use crate::vars::Variables;

/// What a toolchain input's metadata says about the generation of its content.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Stamp {
    length: u64,
    modified: SystemTime,
    mode: u32,
    #[cfg(windows)]
    changed: (u64, crate::wide::Wide, i64),
    #[cfg(unix)]
    changed: (i64, i64, u64, u64),
}

impl settled::Stamp for Stamp {
    fn newest(&self) -> Option<SystemTime> {
        #[cfg(unix)]
        let changed = settled::since_unix_epoch(self.changed.0, self.changed.1)?;
        #[cfg(windows)]
        let changed = settled::since_windows_epoch(self.changed.2)?;
        Some(self.modified.max(changed))
    }

    fn same(&self, other: &Self, _: settled::Comparing) -> bool {
        let Self {
            length,
            modified,
            mode,
            changed,
        } = self;
        *length == other.length
            && *modified == other.modified
            && *mode == other.mode
            && *changed == other.changed
    }
}

/// One actual read of an input: the stamp it was taken under and the content it found.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields, from = "Recorded", into = "Recorded")]
struct Capture {
    taken: Taken<Stamp>,
    content: File,
}

/// A capture as a published record holds it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Recorded {
    stamp: Stamp,
    moment: SystemTime,
    content: File,
}

impl From<Recorded> for Capture {
    fn from(recorded: Recorded) -> Self {
        Self {
            taken: Taken::recorded(recorded.stamp, recorded.moment),
            content: recorded.content,
        }
    }
}

impl From<Capture> for Recorded {
    fn from(capture: Capture) -> Self {
        let (stamp, moment) = capture.taken.into_parts();
        Self {
            stamp,
            moment,
            content: capture.content,
        }
    }
}

impl Capture {
    /// Whether `other` found the same content under the same stamp, whenever either was taken.
    fn same(&self, other: &Self) -> bool {
        !self.taken.changed(other.taken.stamp()) && self.content == other.content
    }

    /// Whether `other` found other content under a stamp both had settled at, which no file can do.
    fn contradicts(&self, other: &Self) -> bool {
        other.taken.settled()
            && self.taken.holds(other.taken.stamp())
            && self.content != other.content
    }
}

#[derive(Debug, Default)]
struct Memo {
    files: BTreeMap<PathBuf, Capture>,
    captures: BTreeMap<PathBuf, Vec<Capture>>,
    attempts: Vec<PathBuf>,
    completed_bytes: u64,
    published_reads: usize,
    opened: u64,
    images: BTreeMap<PathBuf, (Taken<Stamp>, bool)>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::cargo) struct Retained {
    files: BTreeMap<PathBuf, Capture>,
    captures: BTreeMap<PathBuf, Vec<Capture>>,
}

impl Retained {
    fn verify(&self, admitted: &BTreeMap<PathBuf, File>) -> io::Result<()> {
        for (path, capture) in &self.files {
            let (stamp, content) = (capture.taken.stamp(), &capture.content);
            if !path.is_absolute()
                || content.size != stamp.length
                || content.mode != stamp.mode
                || content.digest.len() != 64
                || !content
                    .digest
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
                || admitted
                    .get(path)
                    .is_some_and(|original| original != content)
                || !self
                    .captures
                    .get(path)
                    .is_some_and(|captures| captures.iter().any(|known| known.same(capture)))
            {
                return Err(io::Error::other(
                    "retained identities differ from their original observation",
                ));
            }
        }
        Ok(())
    }

    pub(in crate::cargo) fn merge(&mut self, incoming: &Self) -> io::Result<()> {
        self.verify(&BTreeMap::new())?;
        incoming.verify(&BTreeMap::new())?;
        for (path, captures) in &incoming.captures {
            let original = self.captures.entry(path.clone()).or_default();
            for capture in captures {
                if original.iter().any(|known| known.contradicts(capture)) {
                    return Err(io::Error::other("conflicting original input generation"));
                }
                if !original.iter().any(|known| known.same(capture)) {
                    original.push(capture.clone());
                }
            }
        }
        for (path, incoming) in &incoming.files {
            match self.files.get(path) {
                Some(previous) if previous.same(incoming) => {
                    if incoming.taken.moment() > previous.taken.moment() {
                        self.files.insert(path.clone(), incoming.clone());
                    }
                }
                Some(previous) => {
                    let current = open(path)?.1;
                    if !incoming.taken.changed(current.stamp()) {
                        self.files.insert(path.clone(), incoming.clone());
                    } else if previous.taken.changed(current.stamp()) {
                        return Err(io::Error::other("input publication changed generation"));
                    }
                }
                None => {
                    self.files.insert(path.clone(), incoming.clone());
                }
            }
        }
        self.verify(&BTreeMap::new())
    }
}

#[derive(Debug, Clone)]
pub(in crate::cargo) struct Identities {
    files: Arc<Mutex<Memo>>,
    publication: Arc<Mutex<Option<Publication>>>,
}

#[derive(Debug, Clone)]
struct Publication {
    record: PathBuf,
    cursor: PathBuf,
    key: String,
}

impl Identities {
    pub(in crate::cargo) fn empty() -> Self {
        Self {
            files: Arc::new(Mutex::new(Memo::default())),
            publication: Arc::new(Mutex::new(None)),
        }
    }

    pub(in crate::cargo) fn retain(
        &self,
        admitted: &BTreeMap<PathBuf, File>,
    ) -> io::Result<Retained> {
        let memo = self
            .files
            .lock()
            .map_err(|source| io::Error::other(source.to_string()))?;
        let retained = Retained {
            files: memo.files.clone(),
            captures: memo.captures.clone(),
        };
        drop(memo);
        retained.verify(admitted)?;
        Ok(retained)
    }

    pub(in crate::cargo) fn restore(
        retained: Retained,
        admitted: &BTreeMap<PathBuf, File>,
    ) -> io::Result<Self> {
        retained.verify(admitted)?;
        Ok(Self {
            files: Arc::new(Mutex::new(Memo {
                files: retained.files,
                captures: retained.captures,
                attempts: Vec::new(),
                completed_bytes: 0,
                published_reads: 0,
                opened: 0,
                images: BTreeMap::new(),
            })),
            publication: Arc::new(Mutex::new(None)),
        })
    }

    pub(in crate::cargo) fn accept(&self, retained: Retained) -> io::Result<()> {
        retained.verify(&BTreeMap::new())?;
        let mut memo = self
            .files
            .lock()
            .map_err(|source| io::Error::other(source.to_string()))?;
        let mut merged = retained;
        merged.merge(&Retained {
            files: memo.files.clone(),
            captures: memo.captures.clone(),
        })?;
        merged.verify(&BTreeMap::new())?;
        memo.files = merged.files;
        memo.captures = merged.captures;
        drop(memo);
        Ok(())
    }

    pub(in crate::cargo) fn attach(
        &self,
        (record, cursor): (&Path, &Path),
        key: &str,
    ) -> io::Result<()> {
        *self
            .publication
            .lock()
            .map_err(|source| io::Error::other(source.to_string()))? = Some(Publication {
            record: record.to_path_buf(),
            cursor: cursor.to_path_buf(),
            key: key.to_owned(),
        });
        let mut memo = self
            .files
            .lock()
            .map_err(|source| io::Error::other(source.to_string()))?;
        memo.published_reads = memo.attempts.len();
        drop(memo);
        Ok(())
    }

    pub(in crate::cargo) fn persist(&self) -> io::Result<()> {
        let publication = self
            .publication
            .lock()
            .map_err(|source| io::Error::other(source.to_string()))?
            .clone();
        let Some(publication) = publication else {
            return Ok(());
        };
        let reads = {
            let memo = self
                .files
                .lock()
                .map_err(|source| io::Error::other(source.to_string()))?;
            if memo.attempts.len() == memo.published_reads {
                return Ok(());
            }
            memo.attempts.len()
        };
        super::super::observed::persist_identities(
            (&publication.record, &publication.cursor),
            &publication.key,
            self,
        )?;
        self.files
            .lock()
            .map_err(|source| io::Error::other(source.to_string()))?
            .published_reads = reads;
        Ok(())
    }

    /// Counts one loader input opened and identified in full.
    pub(in crate::cargo) fn opened(&self) -> io::Result<()> {
        let mut memo = self
            .files
            .lock()
            .map_err(|source| io::Error::other(source.to_string()))?;
        memo.opened = memo
            .opened
            .checked_add(1)
            .ok_or_else(|| io::Error::other("loader open count overflow"))?;
        drop(memo);
        Ok(())
    }

    /// The loader inputs opened and identified in full so far.
    #[cfg(all(test, target_os = "macos"))]
    pub(in crate::cargo) fn opens(&self) -> io::Result<u64> {
        Ok(self
            .files
            .lock()
            .map_err(|source| io::Error::other(source.to_string()))?
            .opened)
    }

    pub(in crate::cargo) fn work(&self) -> io::Result<(u64, u64, Vec<PathBuf>)> {
        let memo = self
            .files
            .lock()
            .map_err(|source| io::Error::other(source.to_string()))?;
        Ok((
            u64::try_from(memo.attempts.len()).map_err(io::Error::other)?,
            memo.completed_bytes,
            memo.attempts.clone(),
        ))
    }
}

pub(super) fn inputs(
    (toolchain, cwd): (&Toolchain, &Path),
    options: &CompileOptions,
    env: &Variables,
    files: &mut BTreeMap<PathBuf, File>,
) -> io::Result<super::loaders::Inputs> {
    let root = toolchain
        .sysroot()
        .ok_or_else(|| io::Error::other("unbound compiler sysroot"))?;
    check_environment(env, |name, value| {
        known_compiler((Some(root), toolchain.rustc()), name, value)
            || super::loaders::search_variables().contains(&name)
    })?;
    let loaders = super::loaders::Inputs::compiler(env, cwd, toolchain.identities())?;
    environment(root, toolchain.rustc(), env, &loaders)?;
    for path in [toolchain.cargo(), toolchain.rustc()] {
        files.insert(path.to_path_buf(), identity(path, toolchain.identities())?);
    }
    let target = match &options.build.target {
        Some(target)
            if Path::new(target)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("json")) =>
        {
            return Err(io::Error::other("custom target toolchain inputs"));
        }
        Some(target) => target.as_str(),
        None => toolchain.host(),
    };
    directory(
        &root.join("lib/rustlib").join(target).join("lib"),
        files,
        toolchain.identities(),
    )?;
    let tools = root.join("lib/rustlib").join(toolchain.host()).join("bin");
    directory(&tools, files, toolchain.identities())?;
    for entry in std::fs::read_dir(root.join("lib"))? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            let path = entry.path();
            files.insert(path.clone(), identity(&path, toolchain.identities())?);
        }
    }
    #[cfg(windows)]
    for entry in std::fs::read_dir(root.join("bin"))? {
        let entry = entry?;
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("dll"))
        {
            files.insert(path.clone(), identity(&path, toolchain.identities())?);
        }
    }
    toolchain.identities().persist()?;
    Ok(loaders)
}

pub(in crate::cargo) fn environment(
    root: &Path,
    rustc: &Path,
    env: &Variables,
    loaders: &super::loaders::Inputs,
) -> io::Result<()> {
    check_environment(env, |name, value| {
        known_compiler((Some(root), rustc), name, value) || loaders.admits(name, value)
    })
}

pub(in crate::cargo) fn observation_environment(
    compiler: (Option<&Path>, &Path),
    env: &Variables,
) -> io::Result<()> {
    check_environment(env, |name, value| {
        known_compiler(compiler, name, value)
            || cfg!(target_os = "macos") && name == "DYLD_FALLBACK_LIBRARY_PATH"
    })
}

fn known_compiler(
    (root, rustc): (Option<&Path>, &Path),
    name: &str,
    value: &std::ffi::OsStr,
) -> bool {
    if name == "RUSTC" {
        return Path::new(value) == rustc;
    }
    if name != "RUSTDOC" {
        return false;
    }
    match root {
        Some(root) => {
            Path::new(value)
                == root
                    .join("bin")
                    .join(format!("rustdoc{}", std::env::consts::EXE_SUFFIX))
        }
        None => true,
    }
}

fn check_environment(
    env: &Variables,
    admits: impl Fn(&str, &std::ffi::OsStr) -> bool,
) -> io::Result<()> {
    for (name, value) in env.canonical() {
        let Some(name) = name.to_str() else {
            return Err(io::Error::other("non-textual environment name"));
        };
        if !value.is_empty() && !admits(name, value) && opaque_variable(name) {
            return Err(io::Error::other(format!(
                "the {name} input names an opaque compiler, linker or loader graph"
            )));
        }
    }
    Ok(())
}

fn opaque_variable(name: &str) -> bool {
    matches!(
        name,
        "RUSTC_WRAPPER"
            | "RUSTC_WORKSPACE_WRAPPER"
            | "RUSTC"
            | "RUSTDOC"
            | "COMPILER_PATH"
            | "GCC_EXEC_PREFIX"
            | "LIBRARY_PATH"
    ) || name.starts_with("CARGO_")
        && [
            "_LINKER",
            "_RUNNER",
            "_RUSTC",
            "_RUSTDOC",
            "_RUSTC_WRAPPER",
            "_RUSTC_WORKSPACE_WRAPPER",
            "_RUSTFLAGS",
            "_RUSTDOCFLAGS",
        ]
        .iter()
        .any(|suffix| name.ends_with(suffix))
        && !matches!(
            name,
            "CARGO_ENCODED_RUSTFLAGS" | "CARGO_ENCODED_RUSTDOCFLAGS"
        )
        || super::loaders::loader_variable(std::ffi::OsStr::new(name))
}

fn directory(
    root: &Path,
    files: &mut BTreeMap<PathBuf, File>,
    identities: &Identities,
) -> io::Result<()> {
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            directory(&path, files, identities)?;
        } else {
            files.insert(path.clone(), identity(&path, identities)?);
        }
    }
    Ok(())
}

/// The identity the memo already holds for a canonical regular file whose stamp still agrees with a settled one, without reading or canonicalizing it.
///
/// # Errors
///
/// The file's stamp cannot be read, or the memo is poisoned.
pub(in crate::cargo) fn reused(
    canonical: &Path,
    identities: &Identities,
) -> io::Result<Option<File>> {
    let Some(current) = path_stamp(canonical)? else {
        return Ok(None);
    };
    let held = identities
        .files
        .lock()
        .map_err(|source| io::Error::other(source.to_string()))?
        .files
        .get(canonical)
        .filter(|held| held.taken.holds(&current))
        .cloned();
    let Some(held) = held else {
        return Ok(None);
    };
    match path_stamp(canonical)? {
        Some(again) if held.taken.holds(&again) => Ok(Some(held.content)),
        Some(_) | None => Ok(None),
    }
}

/// Whether a canonical regular file begins an image a dynamic loader can load as a library, read again until its stamp has settled.
///
/// # Errors
///
/// The file cannot be read, changed while its header was read, or the memo is poisoned.
pub(in crate::cargo) fn loadable(canonical: &Path, identities: &Identities) -> io::Result<bool> {
    if let Some(current) = path_stamp(canonical)? {
        let known = identities
            .files
            .lock()
            .map_err(|source| io::Error::other(source.to_string()))?
            .images
            .get(canonical)
            .filter(|(taken, _)| taken.holds(&current))
            .cloned();
        if let Some((taken, image)) = known
            && path_stamp(canonical)?.is_some_and(|again| taken.holds(&again))
        {
            return Ok(image);
        }
    }
    let (header, before) = open(canonical)?;
    let mut head = Vec::new();
    io::Read::take(&header, 4096).read_to_end(&mut head)?;
    if before.changed(&stamp(&header)?) {
        return Err(io::Error::other(
            "a loader input changed while its header was read",
        ));
    }
    let image = super::loaders::image(&head);
    identities
        .files
        .lock()
        .map_err(|source| io::Error::other(source.to_string()))?
        .images
        .insert(canonical.to_path_buf(), (before, image));
    Ok(image)
}

#[cfg(unix)]
fn path_stamp(path: &Path) -> io::Result<Option<Stamp>> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.is_file() {
        unix_stamp(&metadata).map(Some)
    } else {
        Ok(None)
    }
}

#[cfg(windows)]
fn path_stamp(path: &Path) -> io::Result<Option<Stamp>> {
    if !std::fs::symlink_metadata(path)?.is_file() {
        return Ok(None);
    }
    stamp(&std::fs::File::open(path)?).map(Some)
}

#[cfg(unix)]
fn unix_stamp(metadata: &std::fs::Metadata) -> io::Result<Stamp> {
    use std::os::unix::fs::MetadataExt as _;
    Ok(Stamp {
        length: metadata.len(),
        modified: metadata.modified()?,
        mode: metadata.mode(),
        changed: (
            metadata.ctime(),
            metadata.ctime_nsec(),
            metadata.dev(),
            metadata.ino(),
        ),
    })
}

fn stamp(input: &std::fs::File) -> io::Result<Stamp> {
    let metadata = input.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::other("a toolchain input is not a regular file"));
    }
    #[cfg(unix)]
    {
        unix_stamp(&metadata)
    }
    #[cfg(windows)]
    {
        let (identity, time) = crate::capdir::change_stamp(input)?;
        Ok(Stamp {
            length: metadata.len(),
            modified: metadata.modified()?,
            mode: u32::from(metadata.permissions().readonly()),
            changed: (identity.volume, identity.object.into(), time),
        })
    }
}

fn open(path: &Path) -> io::Result<(std::fs::File, Taken<Stamp>)> {
    if !std::fs::symlink_metadata(path)?.is_file() {
        return Err(io::Error::other("a toolchain input is not a regular file"));
    }
    let held = std::fs::File::open(path)?;
    let current = Taken::take(|| stamp(&held))?;
    Ok((held, current))
}

fn unchanged(
    path: &Path,
    resolved: &Path,
    held: &std::fs::File,
    before: &Taken<Stamp>,
) -> io::Result<()> {
    if std::fs::canonicalize(path)? != resolved
        || before.changed(&stamp(held)?)
        || before.changed(open(resolved)?.1.stamp())
    {
        return Err(io::Error::other(
            "the toolchain changed while its content was read",
        ));
    }
    Ok(())
}

fn content(held: &mut std::fs::File, before: &Stamp) -> io::Result<File> {
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 65536];
    let mut size = 0_u64;
    loop {
        let count = held.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(
            buffer
                .get(..count)
                .ok_or_else(|| io::Error::other("input buffer count"))?,
        );
        size = size
            .checked_add(u64::try_from(count).map_err(io::Error::other)?)
            .ok_or_else(|| io::Error::other("toolchain input byte count overflow"))?;
    }
    Ok(File {
        size,
        digest: hex::encode(digest.finalize()),
        mode: before.mode,
    })
}

pub(in crate::cargo) fn identity(path: &Path, identities: &Identities) -> io::Result<File> {
    if !std::fs::symlink_metadata(path)?.is_file() {
        return Err(io::Error::other("a toolchain input is not a regular file"));
    }
    let resolved = std::fs::canonicalize(path)?;
    let (mut held, before) = open(&resolved)?;
    let mut memo = identities
        .files
        .lock()
        .map_err(|source| io::Error::other(source.to_string()))?;
    if let Some(previous) = memo.files.get(&resolved)
        && previous.taken.holds(before.stamp())
    {
        unchanged(path, &resolved, &held, &before)?;
        return Ok(previous.content.clone());
    }
    memo.attempts.push(path.to_path_buf());
    let identity = content(&mut held, before.stamp())?;
    memo.completed_bytes = memo
        .completed_bytes
        .checked_add(identity.size)
        .ok_or_else(|| io::Error::other("toolchain input byte count overflow"))?;
    unchanged(path, &resolved, &held, &before)?;
    if identity.size != before.stamp().length {
        return Err(io::Error::other(
            "the toolchain content length changed while read",
        ));
    }
    let captured = Capture {
        taken: before,
        content: identity.clone(),
    };
    let history = memo.captures.entry(resolved.clone()).or_default();
    if !history.iter().any(|known| known.same(&captured)) {
        history.push(captured.clone());
    }
    memo.files.insert(resolved, captured);
    drop(memo);
    Ok(identity)
}

#[cfg(test)]
pub(in crate::cargo) mod tests {
    /// Waits until a stamp of `path` taken now has settled, so an observation made after it may be reused.
    pub(in crate::cargo) fn settle(path: &std::path::Path) {
        let started = std::time::Instant::now();
        while !super::open(path)
            .expect("the stamp of a written input")
            .1
            .settled()
        {
            assert!(
                started.elapsed() < std::time::Duration::from_mins(1),
                "{} has not settled within a minute of being written",
                path.display()
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    #[test]
    fn an_accept_conserves_a_later_same_arc_capture() {
        let directory = tempfile::tempdir().expect("owned compiler inputs");
        let p = directory.path().join("p");
        let q = directory.path().join("q");
        std::fs::write(&p, b"first").expect("first actual bytes");
        std::fs::write(&q, b"second").expect("second actual bytes");
        let identities = super::Identities::empty();
        super::identity(&p, &identities).expect("first actual capture");
        let retained = identities
            .retain(&std::collections::BTreeMap::new())
            .expect("publication snapshot of the memo");
        super::identity(&q, &identities).expect("interleaved same-arc actual capture");
        identities.accept(retained).expect("publication accepted");
        let after = identities
            .retain(&std::collections::BTreeMap::new())
            .expect("current memo state");
        let resolved_q = std::fs::canonicalize(&q).expect("canonical supplemental object");
        assert!(
            after.files.contains_key(&resolved_q),
            "accept must conserve the later same-Arc capture, not replace it away: {:?}",
            after
                .files
                .keys()
                .map(|path| path.display().to_string())
                .collect::<Vec<String>>()
        );
        assert_eq!(
            super::identity(&q, &identities).expect("conserved known capture"),
            super::identity(&q, &super::Identities::empty()).expect("independently read q"),
            "the conserved capture must keep the genuine digest"
        );
    }

    #[test]
    fn supplemental_retained_content_cannot_replace_its_actual_capture() {
        let directory = tempfile::tempdir().expect("owned input captures");
        let known = directory.path().join("known");
        let supplemental = directory.path().join("supplemental");
        std::fs::write(&known, b"known").expect("known actual bytes");
        std::fs::write(&supplemental, b"other").expect("supplemental actual bytes");
        let identities = super::Identities::empty();
        let content = super::identity(&known, &identities).expect("actual known capture");
        super::identity(&supplemental, &identities).expect("actual supplemental capture");
        let admitted = std::collections::BTreeMap::from([(
            std::fs::canonicalize(&known).expect("known object"),
            content,
        )]);
        let mut retained = identities.retain(&admitted).expect("actual captures");
        retained
            .files
            .get_mut(&std::fs::canonicalize(&supplemental).expect("supplemental object"))
            .expect("supplemental capture")
            .content
            .digest = "a".repeat(64);
        assert_eq!(
            super::Identities::restore(retained, &admitted)
                .expect_err("a supplemental digest needs its actual original capture")
                .kind(),
            std::io::ErrorKind::Other
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_file_identity_wider_than_64_bits_reads_back_from_the_record_that_holds_it() {
        let directory = tempfile::tempdir().expect("owned compiler input");
        let path = directory.path().join("compiler");
        std::fs::write(&path, b"first").expect("compiler bytes");
        let identities = super::Identities::empty();
        super::identity(&path, &identities).expect("an actual digest");
        let mut retained = identities
            .retain(&std::collections::BTreeMap::new())
            .expect("the actual captures");
        let refs = crate::wide::Wide::from(1_313_205_263_863_309_263_863_309_u128);
        for capture in retained.files.values_mut() {
            let (mut stamp, moment) = capture.taken.clone().into_parts();
            stamp.changed.1 = refs;
            capture.taken = super::Taken::recorded(stamp, moment);
        }
        let bytes = serde_json::to_vec(&retained).expect("the record");
        let read: super::Retained =
            crate::strictjson::decode_slice(&bytes).expect("the record reads back");
        assert_eq!(
            read.files
                .values()
                .map(|capture| capture.taken.stamp().changed.1)
                .collect::<Vec<_>>(),
            [refs],
            "ReFS gives a file a 128-bit identity, and the record holding its stamp reads it \
             back as it was written"
        );
    }

    #[test]
    fn retained_identities_reuse_only_the_current_readable_file_generation() {
        let directory = tempfile::tempdir().expect("owned compiler input");
        let path = directory.path().join("compiler");
        std::fs::write(&path, b"first").expect("original compiler bytes");
        let modified = std::fs::metadata(&path)
            .expect("original metadata")
            .modified()
            .expect("original mtime");
        settle(&path);
        let identities = super::Identities::empty();
        let original = super::identity(&path, &identities).expect("original actual digest");
        let resolved = std::fs::canonicalize(&path).expect("original canonical object");
        let admitted = std::collections::BTreeMap::from([(resolved, original.clone())]);
        let bytes = serde_json::to_vec(
            &identities
                .retain(&admitted)
                .expect("original identity custody"),
        )
        .expect("original identity bytes");
        let restore = || {
            super::Identities::restore(
                crate::strictjson::decode_slice(&bytes).expect("retained identity bytes"),
                &admitted,
            )
            .expect("original actual identity")
        };
        let same = restore();
        assert_eq!(
            super::identity(&path, &same).expect("unchanged digest"),
            original
        );
        assert_eq!(
            same.work().expect("unchanged input work"),
            (0, 0, Vec::new())
        );
        std::fs::write(&path, b"other").expect("same-length changed compiler bytes");
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("owned compiler metadata")
            .set_modified(modified)
            .expect("restore old mtime");
        let changed = restore();
        assert_ne!(
            super::identity(&path, &changed)
                .expect("changed actual digest")
                .digest,
            original.digest
        );
        assert_eq!(
            changed.work().expect("changed input work"),
            (1, 5, vec![path.clone()])
        );
        let replacement = directory.path().join("replacement");
        std::fs::write(&replacement, b"third").expect("replacement bytes");
        std::fs::OpenOptions::new()
            .write(true)
            .open(&replacement)
            .expect("replacement metadata")
            .set_modified(modified)
            .expect("same replacement mtime");
        std::fs::remove_file(&path).expect("remove old named input");
        std::fs::rename(&replacement, &path).expect("replace the named input");
        let replaced = restore();
        assert_ne!(
            super::identity(&path, &replaced)
                .expect("replacement actual digest")
                .digest,
            original.digest
        );
        assert_eq!(
            replaced.work().expect("replacement input work"),
            (1, 5, vec![path.clone()])
        );
        std::fs::remove_file(&path).expect("remove replaced input");
        assert_eq!(
            super::identity(&path, &restore())
                .expect_err("a missing input cannot use its record")
                .kind(),
            std::io::ErrorKind::NotFound
        );
    }

    #[test]
    fn retained_identities_reject_content_that_disagrees_with_the_original_observation() {
        let directory = tempfile::tempdir().expect("owned compiler identity");
        let path = directory.path().join("compiler");
        std::fs::write(&path, b"first").expect("original bytes");
        let identities = super::Identities::empty();
        let original = super::identity(&path, &identities).expect("actual original digest");
        let resolved = std::fs::canonicalize(&path).expect("canonical input");
        let admitted = std::collections::BTreeMap::from([(resolved, original)]);
        let mut retained = identities.retain(&admitted).expect("original identity");
        for capture in retained.files.values_mut() {
            capture.content.digest = "a".repeat(64);
        }
        assert_eq!(
            super::Identities::restore(retained, &admitted)
                .expect_err("a changed digest cannot attest the original input")
                .kind(),
            std::io::ErrorKind::Other
        );
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_and_nonregular_inputs_never_return_a_memoized_identity() {
        use std::os::unix::fs::PermissionsExt as _;
        let directory = tempfile::tempdir().expect("owned compiler inputs");
        let path = directory.path().join("compiler");
        std::fs::write(&path, b"first").expect("original bytes");
        let identities = super::Identities::empty();
        super::identity(&path, &identities).expect("actual original input");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o0))
            .expect("refuse reading the input");
        assert_eq!(
            super::identity(&path, &identities)
                .expect_err("permission refusal preserves its cause")
                .kind(),
            std::io::ErrorKind::PermissionDenied
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .expect("restore owned input permissions");
        let alias = directory.path().join("alias");
        std::os::unix::fs::symlink(&path, &alias).expect("opaque leaf alias");
        for refused in [alias.as_path(), directory.path()] {
            assert_eq!(
                super::identity(refused, &identities)
                    .expect_err("a nonregular input cannot receive a file identity")
                    .kind(),
                std::io::ErrorKind::Other
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn refused_compilation_inputs_digest_no_loader_inputs() {
        let directory = tempfile::tempdir().expect("owned compilation inputs");
        let fallback = directory.path().join("fallback");
        std::fs::create_dir_all(&fallback).expect("owned loader namespace");
        for name in ["one", "two", "three"] {
            std::fs::write(fallback.join(name), b"owned-one").expect("owned input");
        }
        let mut env: crate::vars::Variables = njutest_devkit::paths::environment_for_a_run()
            .into_iter()
            .collect();
        env.set("RUSTC_WRAPPER", "");
        env.set("RUSTC_WORKSPACE_WRAPPER", "");
        env.set("LD_LIBRARY_PATH", "opaque");
        env.set("DYLD_FALLBACK_LIBRARY_PATH", &fallback);
        env.set(
            "NJUTEST_FIXTURE_BUILD_CACHE",
            directory.path().join("cache"),
        );
        let options = crate::cargo::LocateOptions {
            cargo: Some(njutest_devkit::paths::cargo_binary()),
            env: Some(env.clone()),
            ..crate::cargo::LocateOptions::default()
        };
        let cancel = crate::runner::Cancel::new();
        let trace = crate::trace::Recorder::wall(
            crate::trace::Sink::Memory(crate::trace::MemorySink::unbounded()),
            crate::testkit::trace::standalone_context(),
        );
        let toolchain = crate::cargo::Toolchain::locate(
            &options,
            directory.path(),
            &crate::runner::Watched::new(&cancel, &trace),
        )
        .expect("actual compiler observation before compilation input admission");
        let events = trace.events();
        let probes = events
            .iter()
            .filter_map(|event| {
                if let crate::trace::Payload::Exec { exec } = &event.payload {
                    Some((&exec.argv, &exec.dir))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        println!("actual compilation input locator probes: {probes:?}");
        assert_eq!(probes.len(), 3);
        let compile = crate::cargo::CompileOptions::new(crate::cargo::BuildDir::new(
            directory.path().join("target"),
            Vec::new(),
        ));
        for name in ["LD_LIBRARY_PATH", "RUSTC_WRAPPER", "RUSTDOC"] {
            let mut refused = env.clone();
            refused.set("LD_LIBRARY_PATH", "");
            refused.set(name, "opaque");
            let identities = super::Identities::empty();
            let bound = toolchain.clone().with_identities(&identities);
            let mut files = std::collections::BTreeMap::new();
            let result = super::inputs((&bound, directory.path()), &compile, &refused, &mut files);
            let refusal = result.expect_err("opaque compilation inputs are still refused");
            assert!(refusal.to_string().contains(name), "{name}: {refusal}");
            let work = identities.work().expect("actual compilation input work");
            println!("{name}: actual refused compilation input work: {work:?}");
            assert_eq!(work, (0, 0, Vec::new()));
            assert!(files.is_empty());
        }
    }

    #[test]
    fn a_stamp_taken_within_the_granularity_of_its_write_is_read_again() {
        let directory = tempfile::tempdir().expect("owned compiler input");
        let path = directory.path().join("compiler");
        std::fs::write(&path, b"first").expect("freshly written compiler bytes");
        let identities = super::Identities::empty();
        let first = super::identity(&path, &identities).expect("first actual digest");
        assert_eq!(
            super::identity(&path, &identities).expect("second actual digest"),
            first
        );
        assert_eq!(
            identities.work().expect("both actual reads"),
            (2, 10, vec![path.clone(), path]),
            "a stamp taken within the granularity of the write it describes is read again"
        );
    }

    #[test]
    fn a_retained_stamp_taken_within_the_granularity_of_its_write_is_read_again() {
        let directory = tempfile::tempdir().expect("owned compiler input");
        let path = directory.path().join("compiler");
        std::fs::write(&path, b"first").expect("freshly written compiler bytes");
        let identities = super::Identities::empty();
        let original = super::identity(&path, &identities).expect("original actual digest");
        let admitted = std::collections::BTreeMap::from([(
            std::fs::canonicalize(&path).expect("canonical compiler"),
            original.clone(),
        )]);
        let restored = super::Identities::restore(
            identities
                .retain(&admitted)
                .expect("original identity custody"),
            &admitted,
        )
        .expect("retained original identity");
        assert_eq!(
            super::identity(&path, &restored).expect("restored actual digest"),
            original
        );
        assert_eq!(
            restored.work().expect("the restored actual read"),
            (1, 5, vec![path]),
            "a retained stamp taken within the granularity of its write is read again"
        );
    }

    #[test]
    fn a_loader_input_stamped_within_the_granularity_of_its_write_is_not_reused() {
        let directory = tempfile::tempdir().expect("owned loader input");
        let path = directory.path().join("library");
        std::fs::write(&path, b"first").expect("freshly written loader bytes");
        let identities = super::Identities::empty();
        super::identity(&path, &identities).expect("actual loader digest");
        let resolved = std::fs::canonicalize(&path).expect("canonical loader input");
        assert_eq!(
            super::reused(&resolved, &identities).expect("loader input stamp"),
            None,
            "a stamp taken within the granularity of its write cannot stand for the bytes"
        );
    }

    #[test]
    fn a_loader_image_stamped_within_the_granularity_of_its_write_is_classified_again() {
        let directory = tempfile::tempdir().expect("owned loader input");
        let path = directory.path().join("archive");
        std::fs::write(&path, b"!<arch>\nfirst").expect("freshly written archive");
        let resolved = std::fs::canonicalize(&path).expect("canonical loader input");
        let identities = super::Identities::empty();
        assert!(!super::loadable(&resolved, &identities).expect("actual classification"));
        identities
            .files
            .lock()
            .expect("unpoisoned memo")
            .images
            .get_mut(&resolved)
            .expect("the classification the memo holds")
            .1 = true;
        assert!(
            !super::loadable(&resolved, &identities).expect("actual classification again"),
            "a stamp taken within the granularity of its write cannot stand for the header"
        );
    }

    /// One input's stamp, the moments its newest time makes unsettled and settled, and a capture of it under either with the digest `digit` repeated.
    fn captures(
        path: &std::path::Path,
    ) -> (
        [std::time::SystemTime; 2],
        impl Fn(std::time::SystemTime, &str) -> super::Capture,
    ) {
        let stamp = super::open(path).expect("an input stamp").1.into_parts().0;
        let newest = njutest_fixture_tree::settled::Stamp::newest(&stamp).expect("a change time");
        let settled = newest
            .checked_add(njutest_fixture_tree::settled::GRANULARITY)
            .and_then(|ripe| ripe.checked_add(std::time::Duration::from_secs(1)))
            .expect("a representable settled moment");
        let capture = move |moment, digit: &str| super::Capture {
            taken: super::Taken::recorded(stamp.clone(), moment),
            content: super::File {
                size: stamp.length,
                digest: digit.repeat(64),
                mode: stamp.mode,
            },
        };
        ([newest, settled], capture)
    }

    fn retained(path: &std::path::Path, capture: &super::Capture) -> super::Retained {
        let resolved = std::fs::canonicalize(path).expect("canonical input");
        super::Retained {
            files: std::collections::BTreeMap::from([(resolved.clone(), capture.clone())]),
            captures: std::collections::BTreeMap::from([(resolved, vec![capture.clone()])]),
        }
    }

    #[test]
    fn captures_of_one_stamp_contradict_each_other_only_when_both_had_settled() {
        let directory = tempfile::tempdir().expect("owned compiler input");
        let path = directory.path().join("compiler");
        std::fs::write(&path, b"first").expect("compiler bytes");
        let ([unsettled, settled], capture) = captures(&path);
        retained(&path, &capture(unsettled, "a"))
            .merge(&retained(&path, &capture(unsettled, "b")))
            .expect("an unsettled capture beside another is the history of a racy rewrite");
        let refusal = retained(&path, &capture(settled, "a"))
            .merge(&retained(&path, &capture(settled, "b")))
            .expect_err("one settled stamp cannot stand for two contents");
        assert!(
            refusal
                .to_string()
                .contains("conflicting original input generation"),
            "{refusal}"
        );
    }

    #[test]
    fn a_settled_capture_of_the_same_content_supersedes_an_unsettled_one() {
        let directory = tempfile::tempdir().expect("owned compiler input");
        let path = directory.path().join("compiler");
        std::fs::write(&path, b"first").expect("compiler bytes");
        let ([unsettled, settled], capture) = captures(&path);
        let held = capture(settled, "a");
        let mut merged = retained(&path, &capture(unsettled, "a"));
        merged
            .merge(&retained(&path, &held))
            .expect("one content under one stamp");
        let restored = super::Identities::restore(merged, &std::collections::BTreeMap::new())
            .expect("merged identities");
        assert_eq!(
            super::identity(&path, &restored).expect("the merged identity"),
            held.content,
            "the later, settled moment is the one the merged record keeps"
        );
        assert_eq!(restored.work().expect("merged reuse"), (0, 0, Vec::new()));
    }

    #[test]
    fn changed_toolchain_bytes_are_hashed_even_when_the_old_mtime_is_restored() {
        let directory = tempfile::tempdir().expect("toolchain identity");
        let path = directory.path().join("compiler");
        std::fs::write(&path, b"first").expect("original bytes");
        let modified = std::fs::metadata(&path)
            .expect("original stamp")
            .modified()
            .expect("original mtime");
        let identities = super::Identities::empty();
        let before = super::identity(&path, &identities).expect("original digest");
        std::fs::write(&path, b"other").expect("changed bytes of the same length");
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("compiler metadata")
            .set_modified(modified)
            .expect("restore the original mtime");
        let after = super::identity(&path, &identities).expect("changed digest");
        assert_ne!(
            before.digest, after.digest,
            "a same-length rewrite within one tick of the observation is read"
        );
        assert_eq!(
            identities.work().expect("the changed actual read"),
            (2, 10, vec![path.clone(), path])
        );
    }

    #[test]
    fn a_settled_stamp_is_reused_and_a_same_length_rewrite_after_it_is_read() {
        let directory = tempfile::tempdir().expect("toolchain identity");
        let path = directory.path().join("compiler");
        std::fs::write(&path, b"first").expect("original bytes");
        let modified = std::fs::metadata(&path)
            .expect("original stamp")
            .modified()
            .expect("original mtime");
        settle(&path);
        let identities = super::Identities::empty();
        let before = super::identity(&path, &identities).expect("original digest");
        assert_eq!(
            identities.work().expect("the first actual read"),
            (1, 5, vec![path.clone()])
        );
        assert_eq!(
            super::identity(&path, &identities).expect("stable memo identity"),
            before
        );
        assert_eq!(
            identities.work().expect("memo reuse reads no file"),
            (1, 5, vec![path.clone()])
        );
        std::fs::write(&path, b"other").expect("changed bytes of the same length");
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("compiler metadata")
            .set_modified(modified)
            .expect("restore the original mtime");
        let after = super::identity(&path, &identities).expect("changed digest");
        assert_ne!(before.digest, after.digest);
        assert_eq!(
            identities.work().expect("the changed actual read"),
            (2, 10, vec![path.clone(), path])
        );
    }
}
