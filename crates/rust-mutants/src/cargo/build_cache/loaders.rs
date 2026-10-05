// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Owned native library search inputs bind namespace, absence, aliases, content and modes.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use super::toolchain::Identities;
use crate::capdir::{Dir, Entry, Kind, Name};
use crate::sensitive::Sensitive;
use crate::vars::Variables;

#[derive(Debug)]
pub(in crate::cargo) struct Inputs {
    namespaces: BTreeMap<&'static str, Option<OsString>>,
    digest: String,
}

impl Inputs {
    pub(in crate::cargo) fn of(env: &Variables, identities: &Identities) -> io::Result<Self> {
        loader_environment(env)?;
        Self::capture(env, None, identities, search_variables())
    }

    pub(in crate::cargo) fn compiler(
        env: &Variables,
        cwd: &Path,
        identities: &Identities,
    ) -> io::Result<Self> {
        loader_environment(env)?;
        if cfg!(unix)
            && cwd.is_absolute()
            && search_variables().iter().all(|name| {
                env.var(name).is_none_or(|value| {
                    value.is_empty()
                        || search_paths(value)
                            .is_ok_and(|paths| paths.iter().all(|path| path.is_absolute()))
                })
            })
        {
            return Self::of(env, identities);
        }
        Self::capture(env, Some(cwd), identities, search_variables())
    }

    pub(in crate::cargo) fn observation(
        env: &Variables,
        identities: &Identities,
    ) -> io::Result<Self> {
        let names: &[&'static str] = if cfg!(target_os = "macos") {
            &["DYLD_FALLBACK_LIBRARY_PATH"]
        } else {
            &[]
        };
        Self::capture(env, None, identities, names)
    }

    fn capture(
        env: &Variables,
        cwd: Option<&Path>,
        identities: &Identities,
        names: &[&'static str],
    ) -> io::Result<Self> {
        if cfg!(windows) && cwd.is_none() && names.contains(&"PATH") && env.var("PATH").is_some() {
            return Err(refused(
                Path::new(""),
                io::Error::new(
                    io::ErrorKind::Unsupported,
                    "the native DLL search namespace requires the actual compiler cwd",
                ),
            ));
        }
        let mut namespaces = BTreeMap::new();
        let mut digest = Sha256::new();
        super::field(&mut digest, b"native-library-inputs-v3");
        if let Some(cwd) = cwd {
            if !cwd.is_absolute() {
                return Err(refused(
                    cwd,
                    io::Error::new(
                        io::ErrorKind::Unsupported,
                        "a captured loader cwd must be absolute",
                    ),
                ));
            }
            super::field(&mut digest, cwd.as_os_str().as_encoded_bytes());
        }
        for &name in names {
            super::field(&mut digest, name.as_bytes());
            let value = env.var(name).map(OsStr::to_os_string);
            match &value {
                None => super::field(&mut digest, b"absent-variable"),
                Some(value) => {
                    super::field(&mut digest, b"present-variable");
                    super::field(&mut digest, value.as_encoded_bytes());
                    if !value.is_empty() || cfg!(any(windows, target_os = "macos")) {
                        for path in search_paths(value)
                            .map_err(|source| refused(Path::new(value), source))?
                        {
                            let resolved =
                                resolve(&path, cwd).map_err(|source| refused(&path, source))?;
                            capture(
                                &resolved,
                                identities,
                                &mut digest,
                                (name.contains("FRAMEWORK"), &mut Vec::new()),
                            )
                            .map_err(|source| refused(&path, source))?;
                        }
                    }
                }
            }
            namespaces.insert(name, value);
        }
        if cfg!(windows)
            && let Some(cwd) = cwd
        {
            super::field(&mut digest, b"native-current-directory");
            capture(cwd, identities, &mut digest, (false, &mut Vec::new()))
                .map_err(|source| refused(cwd, source))?;
        }
        Ok(Self {
            namespaces,
            digest: hex::encode(digest.finalize()),
        })
    }

    pub(in crate::cargo) fn admits(&self, name: &str, value: &OsStr) -> bool {
        self.namespaces.get(name).and_then(Option::as_deref) == Some(value)
    }

    pub(in crate::cargo) fn digest(&self) -> &str {
        &self.digest
    }
}

/// The variables this platform's dynamic loader searches libraries by, which the loader inputs bind rather than refuse.
pub(super) const fn search_variables() -> &'static [&'static str] {
    if cfg!(target_os = "linux") {
        &["LD_LIBRARY_PATH"]
    } else if cfg!(target_os = "macos") {
        &[
            "DYLD_LIBRARY_PATH",
            "DYLD_FRAMEWORK_PATH",
            "DYLD_FALLBACK_LIBRARY_PATH",
            "DYLD_FALLBACK_FRAMEWORK_PATH",
            "DYLD_VERSIONED_LIBRARY_PATH",
            "DYLD_VERSIONED_FRAMEWORK_PATH",
        ]
    } else if cfg!(windows) {
        &["PATH"]
    } else {
        &[]
    }
}

/// The prefixes of the variables this platform's loader reads: the ELF loader's `LD_`, and on macOS dyld's `DYLD_` beside the `LD_` its linker reads; the Windows loader reads only `PATH`.
const fn loader_prefixes() -> &'static [&'static str] {
    if cfg!(target_os = "macos") {
        &["DYLD_", "LD_"]
    } else if cfg!(windows) {
        &[]
    } else {
        &["LD_"]
    }
}

/// Whether this platform's loader reads `name`, so that a value it holds is refused unless [`search_variables`] binds what it names; any other variable is bound by its value alone.
pub(in crate::cargo) fn loader_variable(name: &OsStr) -> bool {
    loader_prefixes()
        .iter()
        .any(|prefix| crate::vars::Spelling::HOST.begins(name, prefix))
}

fn search_paths(value: &OsStr) -> io::Result<Vec<PathBuf>> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
        if value.as_bytes().contains(&b'$') {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "loader token expansion requires the original executable context",
            ));
        }
        Ok(value
            .as_bytes()
            .split(|byte| matches!(byte, b':' | b';'))
            .map(|bytes| PathBuf::from(OsString::from_vec(bytes.to_vec())))
            .collect())
    }
    #[cfg(not(target_os = "linux"))]
    {
        #[cfg(windows)]
        {
            use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
            let units = value.encode_wide().collect::<Vec<_>>();
            if units.contains(&u16::from(b'"')) {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "a quoted DLL search path is unsupported",
                ));
            }
            Ok(units
                .split(|unit| *unit == u16::from(b';'))
                .map(|units| PathBuf::from(OsString::from_wide(units)))
                .collect())
        }
        #[cfg(not(windows))]
        {
            let paths: Vec<PathBuf> = std::env::split_paths(value).collect();
            #[cfg(target_os = "macos")]
            if paths.iter().any(|path| path.as_os_str().is_empty()) {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "an empty Darwin search entry does not bind the captured cwd",
                ));
            }
            if paths.iter().any(|path| {
                let bytes = path.as_os_str().as_encoded_bytes();
                bytes.starts_with(b"@")
            }) {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "loader token expansion requires the original executable context",
                ));
            }
            Ok(paths)
        }
    }
}

fn resolve(path: &Path, cwd: Option<&Path>) -> io::Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    #[cfg(windows)]
    if path.has_root()
        || matches!(
            path.components().next(),
            Some(std::path::Component::Prefix(_))
        )
    {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "a drive-relative DLL search path requires drive state",
        ));
    }
    cwd.map(|cwd| cwd.join(path)).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "a relative loader search directory requires the captured cwd",
        )
    })
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::cargo) struct RuntimeAttestation {
    schema: u32,
    digest: String,
}

#[derive(Debug)]
pub(in crate::cargo) struct RuntimeInputs {
    original: RuntimeAttestation,
    env: Sensitive<Variables>,
    cwd: PathBuf,
    identities: Identities,
}

impl RuntimeInputs {
    pub(in crate::cargo) fn capture(
        env: &Variables,
        cwd: &Path,
        identities: &Identities,
    ) -> io::Result<Self> {
        loader_environment(env)?;
        let inputs = Inputs::capture(env, Some(cwd), identities, search_variables())?;
        Ok(Self {
            original: RuntimeAttestation {
                schema: 1,
                digest: inputs.digest,
            },
            env: Sensitive::new(env.clone()),
            cwd: cwd.to_path_buf(),
            identities: identities.clone(),
        })
    }

    pub(in crate::cargo) fn attestation(&self) -> RuntimeAttestation {
        self.original.clone()
    }

    pub(in crate::cargo) fn restore(
        original: &RuntimeAttestation,
        env: &Variables,
        cwd: &Path,
        identities: &Identities,
    ) -> io::Result<Self> {
        if original.schema != 1
            || original.digest.len() != 64
            || !original
                .digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid original loader attestation",
            ));
        }
        loader_environment(env)?;
        let retained = Self {
            original: original.clone(),
            env: Sensitive::new(env.clone()),
            cwd: cwd.to_path_buf(),
            identities: identities.clone(),
        };
        retained.verify()?;
        Ok(retained)
    }

    pub(in crate::cargo) fn verify(&self) -> io::Result<()> {
        let current = Inputs::capture(
            self.env.expose(),
            Some(&self.cwd),
            &self.identities,
            search_variables(),
        )?;
        if current.digest != self.original.digest {
            return Err(io::Error::other(
                "the original runtime loader namespace changed",
            ));
        }
        Ok(())
    }
}

fn loader_environment(env: &Variables) -> io::Result<()> {
    for (name, value) in env.canonical() {
        if !value.is_empty()
            && loader_variable(&name)
            && !search_variables()
                .iter()
                .any(|allowed| name.as_os_str() == OsStr::new(allowed))
        {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "an opaque runtime loader variable is active",
            ));
        }
    }
    Ok(())
}

fn capture(
    path: &Path,
    identities: &Identities,
    digest: &mut Sha256,
    (frameworks, ancestors): (bool, &mut Vec<PathBuf>),
) -> io::Result<()> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "a loader search directory must be absolute",
        ));
    }
    super::field(digest, path.as_os_str().as_encoded_bytes());
    let canonical = match reached(path)? {
        Reached::At(canonical) => canonical,
        Reached::Absent => {
            super::field(digest, b"absent");
            return Ok(());
        }
        Reached::Untraversable => {
            super::field(digest, b"untraversable");
            return Ok(());
        }
    };
    super::field(digest, canonical.as_os_str().as_encoded_bytes());
    #[cfg(windows)]
    let fixed = fixed_search_directories()?;
    #[cfg(not(windows))]
    let fixed: [PathBuf; 0] = [];
    if among(&canonical, &fixed) {
        super::field(digest, b"windows-fixed-search-directory");
        return Ok(());
    }
    if ancestors.contains(&canonical) {
        super::field(digest, b"ancestor-directory-alias");
        return Ok(());
    }
    ancestors.push(canonical.clone());
    let directory = Dir::open(&canonical)?;
    let before = directory.status()?;
    super::field(digest, &before.identity.volume.to_be_bytes());
    super::field(digest, &before.identity.object.to_be_bytes());
    let mut entries = directory.entries()?;
    entries.sort();
    for entry in &entries {
        capture_entry(
            (&directory, &canonical),
            entry,
            (identities, digest),
            (frameworks, ancestors),
        )?;
    }
    let mut after = directory.entries()?;
    after.sort();
    let unmoved = match reached(path)? {
        Reached::At(again) => Dir::open(&again)?.status()?.identity == before.identity,
        Reached::Absent | Reached::Untraversable => false,
    };
    if entries != after || !unmoved {
        return Err(io::Error::other("the loader search namespace changed"));
    }
    ancestors.pop();
    Ok(())
}

fn capture_entry(
    (directory, root): (&Dir, &Path),
    text: &str,
    (identities, digest): (&Identities, &mut Sha256),
    (frameworks, ancestors): (bool, &mut Vec<PathBuf>),
) -> io::Result<()> {
    let name = Name::new(text).map_err(io::Error::other)?;
    let path = root.join(text);
    let status = match directory.status_at(name) {
        Ok(Some(status)) => status,
        Ok(None) => return Err(io::Error::other("a loader search entry disappeared")),
        Err(denied) if denied.kind() == io::ErrorKind::PermissionDenied => {
            unreadable(digest, &path);
            return Ok(());
        }
        Err(source) => return Err(source),
    };
    super::field(digest, text.as_bytes());
    let descend = frameworks || (cfg!(target_os = "linux") && text == "glibc-hwcaps");
    match status.kind {
        Kind::Directory => {
            super::field(digest, b"directory");
            if descend {
                capture(&path, identities, digest, (true, ancestors))?;
            }
        }
        Kind::File => match readable(&path, identities)? {
            Readable::NotLibrary => not_an_image(digest, &path),
            Readable::Unreadable => unreadable(digest, &path),
            Readable::Library => match super::toolchain::reused(&path, identities)? {
                Some(state) => bound(digest, &path, &state),
                None => match directory.open_entry(name)? {
                    Entry::File(file) => captured_file(&path, &file, identities, digest)?,
                    Entry::Dir(_) | Entry::Other => {
                        return Err(io::Error::other("the loader search entry changed kind"));
                    }
                },
            },
        },
        Kind::ExecutionAlias => super::field(digest, b"app-execution-alias"),
        Kind::Other => capture_link(&path, identities, digest, (descend, ancestors))?,
    }
    if directory.status_at(name)? != Some(status) {
        return Err(io::Error::other("the loader search entry changed identity"));
    }
    Ok(())
}

fn capture_link(
    path: &Path,
    identities: &Identities,
    digest: &mut Sha256,
    (descend, ancestors): (bool, &mut Vec<PathBuf>),
) -> io::Result<()> {
    let target = std::fs::read_link(path)?;
    super::field(digest, target.as_os_str().as_encoded_bytes());
    match reached(path)? {
        Reached::At(canonical) => {
            let file = crate::capdir::open_file_at(&canonical)?;
            let status = crate::capdir::file_status(&file)?;
            match status.kind {
                Kind::File => match readable(&canonical, identities)? {
                    Readable::NotLibrary => not_an_image(digest, &canonical),
                    Readable::Unreadable => unreadable(digest, &canonical),
                    Readable::Library => captured_file(&canonical, &file, identities, digest)?,
                },
                Kind::Directory => {
                    super::field(digest, b"directory-alias");
                    super::field(digest, canonical.as_os_str().as_encoded_bytes());
                    super::field(digest, &status.identity.volume.to_be_bytes());
                    super::field(digest, &status.identity.object.to_be_bytes());
                    if descend {
                        capture(&canonical, identities, digest, (true, ancestors))?;
                    }
                }
                Kind::ExecutionAlias | Kind::Other => {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "a loader alias target is not a file or directory",
                    ));
                }
            }
            let unmoved = match reached(path)? {
                Reached::At(again) => again == canonical,
                Reached::Absent | Reached::Untraversable => false,
            };
            if !unmoved || crate::capdir::file_status(&file)? != status {
                return Err(io::Error::other("the loader alias target changed"));
            }
            Ok(())
        }
        Reached::Absent => {
            super::field(digest, b"absent-link-target");
            Ok(())
        }
        Reached::Untraversable => {
            super::field(digest, b"untraversable-link-target");
            Ok(())
        }
    }
}

/// The directories the Windows loader searches before any search path, each spelled canonically: the system directory, the 16-bit system directory and the Windows directory, leaving out one that names nothing, as no search path can lead to it.
#[cfg(windows)]
fn fixed_search_directories() -> io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    for directory in crate::capdir::fixed_search_directories()? {
        match reached(&directory)? {
            Reached::At(canonical) => found.push(canonical),
            Reached::Absent | Reached::Untraversable => {}
        }
    }
    Ok(found)
}

/// Whether `canonical` is one of the `fixed` directories, compared as Windows compares a path, without regard to ASCII case.
fn among(canonical: &Path, fixed: &[PathBuf]) -> bool {
    let spelled = canonical.as_os_str().as_encoded_bytes();
    fixed.iter().any(|directory| {
        directory
            .as_os_str()
            .as_encoded_bytes()
            .eq_ignore_ascii_case(spelled)
    })
}

/// Where a loader search path leads, as far as the host lets this process, and so every process a compile starts, follow it.
enum Reached {
    /// The object it names, spelled canonically.
    At(PathBuf),
    /// No object: the path names nothing.
    Absent,
    /// A mount point on the way the host refuses to traverse, a refusal every process this one starts inherits, so no loader of theirs reaches anything through it.
    Untraversable,
}

/// Where `path` leads, telling a path that names nothing and one the host refuses to traverse from one that could not be read.
fn reached(path: &Path) -> io::Result<Reached> {
    match std::fs::canonicalize(path) {
        Ok(canonical) => Ok(Reached::At(canonical)),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(Reached::Absent),
        Err(source) if untraversable(&source) => Ok(Reached::Untraversable),
        Err(source) => Err(source),
    }
}

/// The code Windows refuses to traverse a mount point a non-administrator made with, `ERROR_UNTRUSTED_MOUNT_POINT`, in a process under redirection trust.
#[cfg(windows)]
const UNTRUSTED_MOUNT_POINT: Option<u32> =
    Some(windows_sys::Win32::Foundation::ERROR_UNTRUSTED_MOUNT_POINT);

/// No platform but Windows refuses to traverse a mount point by who made it.
#[cfg(not(windows))]
const UNTRUSTED_MOUNT_POINT: Option<u32> = None;

/// Whether `source` is the host refusing to traverse a mount point it does not trust.
fn untraversable(source: &io::Error) -> bool {
    UNTRUSTED_MOUNT_POINT.is_some_and(|untrusted| {
        source
            .raw_os_error()
            .is_some_and(|code| code.cast_unsigned() == untrusted)
    })
}

fn captured_file(
    path: &Path,
    file: &std::fs::File,
    identities: &Identities,
    digest: &mut Sha256,
) -> io::Result<()> {
    identities.opened()?;
    let held = crate::capdir::file_status(file)?;
    match held.kind {
        Kind::File => {}
        Kind::Directory | Kind::ExecutionAlias | Kind::Other => {
            return Err(io::Error::other("a loader input is not a regular file"));
        }
    }
    let before = crate::capdir::open_file_at(path)?;
    if crate::capdir::file_status(&before)? != held {
        return Err(io::Error::other("the loader input changed identity"));
    }
    let state = super::toolchain::identity(path, identities)?;
    let after = crate::capdir::open_file_at(path)?;
    if crate::capdir::file_status(&after)? != held {
        return Err(io::Error::other("the loader input changed identity"));
    }
    bound(digest, path, &state);
    Ok(())
}

/// What a regular file is to a loader that runs as this user.
enum Readable {
    Library,
    NotLibrary,
    Unreadable,
}

/// Classifies a regular file for the loader: a file this user may not read is one no loader running as this user can load either.
fn readable(path: &Path, identities: &Identities) -> io::Result<Readable> {
    match super::toolchain::loadable(path, identities) {
        Ok(true) => Ok(Readable::Library),
        Ok(false) => Ok(Readable::NotLibrary),
        Err(denied) if denied.kind() == io::ErrorKind::PermissionDenied => Ok(Readable::Unreadable),
        Err(source) => Err(source),
    }
}

fn unreadable(digest: &mut Sha256, path: &Path) {
    super::field(digest, path.as_os_str().as_encoded_bytes());
    super::field(digest, b"unreadable-by-this-user");
}

fn not_an_image(digest: &mut Sha256, path: &Path) {
    super::field(digest, path.as_os_str().as_encoded_bytes());
    super::field(digest, b"not-a-loadable-image");
}

/// Whether these leading bytes begin an image a dynamic loader can load as a library: anything it cannot rule out counts as one.
pub(super) fn image(head: &[u8]) -> bool {
    match head {
        [0xcf | 0xce, 0xfa, 0xed, 0xfe, ..] => !matches!(
            head.get(12..)
                .and_then(<[u8]>::first_chunk::<4>)
                .map(|filetype| u32::from_le_bytes(*filetype)),
            Some(1 | 2 | 4 | 10)
        ),
        [0xfe, 0xed, 0xfa, 0xce | 0xcf, ..] | [0xca, 0xfe, 0xba, 0xbe | 0xbf, ..] => true,
        [0x7f, b'E', b'L', b'F', ..] => elf_library(head),
        [b'M', b'Z', ..] => pe_library(head),
        _ => false,
    }
}

fn elf_library(head: &[u8]) -> bool {
    let Some(kind) = head.get(16..).and_then(<[u8]>::first_chunk::<2>) else {
        return true;
    };
    let kind = match head.get(5) {
        Some(2) => u16::from_be_bytes(*kind),
        Some(_) | None => u16::from_le_bytes(*kind),
    };
    !matches!(kind, 1 | 2 | 4)
}

fn pe_library(head: &[u8]) -> bool {
    let Some(offset) = head.get(0x3c..).and_then(<[u8]>::first_chunk::<4>) else {
        return true;
    };
    let Ok(offset) = usize::try_from(u32::from_le_bytes(*offset)) else {
        return true;
    };
    if head.get(offset..).and_then(<[u8]>::first_chunk::<4>) != Some(b"PE\0\0") {
        return true;
    }
    let Some(characteristics) = offset
        .checked_add(22)
        .and_then(|at| head.get(at..))
        .and_then(<[u8]>::first_chunk::<2>)
    else {
        return true;
    };
    u16::from_le_bytes(*characteristics) & 0x2000 != 0
}

fn bound(digest: &mut Sha256, path: &Path, state: &super::File) {
    super::field(digest, path.as_os_str().as_encoded_bytes());
    super::field(digest, state.digest.as_bytes());
    super::field(digest, &state.mode.to_be_bytes());
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
#[error("{code}: the loader input {} could not be bound ({:?}, {})", path.display(), source.kind(), super::os_code(source), code = LoaderInputError::code().code)]
struct LoaderInputError {
    path: PathBuf,
    source: io::Error,
}

impl LoaderInputError {
    const fn code() -> crate::error::ErrorCode {
        crate::error::COMPILER_INPUT_UNREADABLE
    }
}

fn refused(path: &Path, source: io::Error) -> io::Error {
    io::Error::new(
        source.kind(),
        LoaderInputError {
            path: path.to_path_buf(),
            source,
        },
    )
}

#[cfg(test)]
pub(in crate::cargo) mod tests {
    /// A library image this platform's loader would load, carrying `payload`, for the tests that bind one.
    pub(in crate::cargo) fn test_library(payload: &[u8]) -> Vec<u8> {
        let mut image = if cfg!(target_os = "macos") {
            vec![
                0xcf, 0xfa, 0xed, 0xfe, 0x0c, 0, 0, 0x01, 0, 0, 0, 0, 6, 0, 0, 0,
            ]
        } else if cfg!(windows) {
            portable_executable(0x2000)
        } else {
            vec![
                0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 0,
            ]
        };
        image.extend_from_slice(payload);
        image
    }

    /// The smallest portable executable header with these COFF characteristics.
    fn portable_executable(characteristics: u16) -> Vec<u8> {
        let mut image = b"MZ".to_vec();
        image.resize(0x3c, 0);
        image.extend_from_slice(&0x40_u32.to_le_bytes());
        image.extend_from_slice(b"PE\0\0");
        image.resize(0x40 + 22, 0);
        image.extend_from_slice(&characteristics.to_le_bytes());
        image
    }

    #[test]
    fn only_an_image_a_loader_could_load_counts_as_a_library() {
        let macho = |filetype: u8| {
            vec![
                0xcf, 0xfa, 0xed, 0xfe, 0x0c, 0, 0, 0x01, 0, 0, 0, 0, filetype, 0, 0, 0,
            ]
        };
        let elf = |data: u8, kind: [u8; 2]| {
            let mut image = vec![
                0x7f, b'E', b'L', b'F', 2, data, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ];
            image.extend_from_slice(&kind);
            image
        };
        for (head, library) in [
            (macho(6), true),
            (macho(8), true),
            (macho(1), false),
            (macho(2), false),
            (macho(10), false),
            (vec![0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 2], true),
            (elf(1, [3, 0]), true),
            (elf(1, [2, 0]), false),
            (elf(1, [1, 0]), false),
            (elf(2, [0, 3]), true),
            (elf(2, [0, 2]), false),
            (vec![0x7f, b'E', b'L', b'F'], true),
            (portable_executable(0x2000), true),
            (portable_executable(0x0002), false),
            (b"MZ".to_vec(), true),
            (b"!<arch>\n".to_vec(), false),
            (b"#!/bin/sh\n".to_vec(), false),
            (Vec::new(), false),
            (test_library(b"payload"), true),
        ] {
            assert_eq!(super::image(&head), library, "{head:02x?}");
        }
    }

    #[test]
    fn a_search_directory_is_fixed_exactly_where_it_is_one_the_loader_searches_before_the_path() {
        use std::path::{Path, PathBuf};
        let fixed = [
            PathBuf::from(r"\\?\C:\WINDOWS\system32"),
            PathBuf::from(r"\\?\C:\WINDOWS\System"),
            PathBuf::from(r"\\?\C:\WINDOWS"),
        ];
        for spelled in [
            r"\\?\C:\WINDOWS\system32",
            r"\\?\c:\windows\SYSTEM32",
            r"\\?\C:\Windows\System32",
            r"\\?\C:\Windows\system",
            r"\\?\c:\Windows",
        ] {
            assert!(
                super::among(Path::new(spelled), &fixed),
                "{spelled} is a directory the loader searched before any search path"
            );
        }
        for other in [
            r"\\?\C:\WINDOWS\system32\drivers",
            r"\\?\C:\WINDOWS\SysWOW64",
            r"\\?\C:\WINDOWS\system32x",
            r"\\?\D:\WINDOWS\system32",
            r"\\?\C:\",
            r"C:\WINDOWS\system32",
            "/usr/lib",
        ] {
            assert!(
                !super::among(Path::new(other), &fixed),
                "{other} is a directory only a search path leads the loader to"
            );
        }
        assert!(
            !super::among(Path::new(r"\\?\C:\WINDOWS\system32"), &[]),
            "a loader that searches no fixed directory first leaves every search path to be read"
        );
    }
}

#[cfg(all(test, target_os = "macos"))]
mod macos_tests {
    use super::{Inputs, LoaderInputError};
    use crate::cargo::build_cache::toolchain::Identities;
    use crate::vars::Variables;

    #[test]
    fn a_directory_alias_is_a_bound_search_entry_and_retargeting_changes_it() {
        let directory = tempfile::tempdir().expect("owned loader parent");
        let search = directory.path().join("search");
        let target = directory.path().join("directory");
        std::fs::create_dir_all(&search).expect("owned search directory");
        std::fs::create_dir_all(&target).expect("owned directory target");
        let alias = search.join("candidate");
        std::os::unix::fs::symlink(&target, &alias).expect("directory alias");
        let mut env = Variables::default();
        env.set("DYLD_FALLBACK_LIBRARY_PATH", &search);
        let identities = Identities::empty();
        let before = Inputs::of(&env, &identities).expect("a directory alias is not a library");
        assert_eq!(
            before.digest(),
            Inputs::of(&env, &identities)
                .expect("unchanged alias")
                .digest()
        );
        let library = directory.path().join("library");
        std::fs::write(
            &library,
            super::tests::test_library(b"actual library input"),
        )
        .expect("owned library target");
        std::fs::remove_file(&alias).expect("retarget owned alias");
        std::os::unix::fs::symlink(&library, &alias).expect("library alias");
        assert_ne!(
            before.digest(),
            Inputs::of(&env, &identities)
                .expect("verified library alias")
                .digest()
        );
    }

    #[test]
    fn an_unchanged_search_directory_is_bound_again_without_opening_its_libraries() {
        let directory = tempfile::tempdir().expect("owned loader search");
        for index in 0..64 {
            std::fs::write(
                directory.path().join(format!("lib{index}.dylib")),
                super::tests::test_library(b"actual library input"),
            )
            .expect("owned library");
        }
        for index in 0..64 {
            crate::cargo::build_cache::toolchain::tests::settle(
                &directory.path().join(format!("lib{index}.dylib")),
            );
        }
        let mut env = Variables::default();
        env.set("DYLD_FALLBACK_LIBRARY_PATH", directory.path());
        let identities = Identities::empty();
        let before = Inputs::of(&env, &identities).expect("original inputs");
        let first = identities.opens().expect("first capture count");
        assert!(
            first >= 64,
            "the first capture opened {first} libraries, not every one of the 64"
        );
        let again = Inputs::of(&env, &identities).expect("unchanged inputs");
        assert_eq!(before.digest(), again.digest());
        let second = identities.opens().expect("second capture count") - first;
        assert_eq!(
            second, 0,
            "the unchanged second capture opened {second} libraries again"
        );
    }

    #[test]
    fn a_file_no_loader_can_load_is_named_without_being_identified() {
        let directory = tempfile::tempdir().expect("owned loader search");
        let executable = directory.path().join("suite-0123456789abcdef");
        let archive = directory.path().join("libdependency.rlib");
        let library = directory.path().join("libmacro.dylib");
        let mut program = vec![
            0xcf, 0xfa, 0xed, 0xfe, 0x0c, 0, 0, 0x01, 0, 0, 0, 0, 2, 0, 0, 0,
        ];
        program.extend_from_slice(b"a test executable");
        std::fs::write(&executable, program).expect("owned executable");
        std::fs::write(&archive, b"!<arch>\nfirst").expect("owned archive");
        std::fs::write(&library, super::tests::test_library(b"a proc macro"))
            .expect("owned library");
        let mut env = Variables::default();
        env.set("DYLD_FALLBACK_LIBRARY_PATH", directory.path());
        let identities = Identities::empty();
        let before = Inputs::of(&env, &identities).expect("original inputs");
        assert_eq!(
            identities.opens().expect("capture count"),
            1,
            "only the one loadable image is identified in full"
        );
        std::fs::write(&archive, b"!<arch>\nother").expect("changed archive");
        assert_eq!(
            before.digest(),
            Inputs::of(&env, &identities)
                .expect("archive change")
                .digest(),
            "no loader reads an archive, so its bytes are not a loader input"
        );
        std::fs::write(&archive, super::tests::test_library(b"now an image"))
            .expect("archive becomes an image");
        assert_ne!(
            before.digest(),
            Inputs::of(&env, &identities).expect("new image").digest(),
            "a file that becomes loadable is bound"
        );
    }

    #[test]
    fn a_file_this_user_cannot_read_is_named_and_reading_it_later_rebinds() {
        use std::os::unix::fs::PermissionsExt as _;
        let directory = tempfile::tempdir().expect("owned loader search");
        let sealed = directory.path().join("libsealed.dylib");
        std::fs::write(&sealed, super::tests::test_library(b"a library")).expect("owned library");
        std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o000))
            .expect("withdraw read access");
        if std::fs::File::open(&sealed).is_ok() {
            return;
        }
        let mut env = Variables::default();
        env.set("DYLD_FALLBACK_LIBRARY_PATH", directory.path());
        let identities = Identities::empty();
        let unreadable =
            Inputs::of(&env, &identities).expect("an unreadable file is a bound input");
        assert_eq!(
            unreadable.digest(),
            Inputs::of(&env, &identities)
                .expect("still unreadable")
                .digest()
        );
        std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o644))
            .expect("restore read access");
        assert_ne!(
            unreadable.digest(),
            Inputs::of(&env, &identities)
                .expect("now readable")
                .digest(),
            "a file that becomes readable is bound by its content"
        );
    }

    #[test]
    fn the_native_system_library_namespace_has_complete_typed_inputs() {
        let mut env = Variables::default();
        env.set("DYLD_FALLBACK_LIBRARY_PATH", "/usr/lib");
        Inputs::of(&env, &Identities::empty()).expect("actual native system library namespace");
    }

    #[test]
    fn a_fallback_library_change_is_bound_even_with_its_original_mtime() {
        let directory = tempfile::tempdir().expect("owned loader search");
        let path = directory.path().join("libcandidate.dylib");
        std::fs::write(&path, super::tests::test_library(b"first")).expect("original library");
        let modified = std::fs::metadata(&path)
            .expect("original metadata")
            .modified()
            .expect("original mtime");
        crate::cargo::build_cache::toolchain::tests::settle(&path);
        let mut env = Variables::default();
        env.set("DYLD_FALLBACK_LIBRARY_PATH", directory.path());
        let identities = Identities::empty();
        let before = Inputs::of(&env, &identities).expect("original inputs");
        assert!(before.admits("DYLD_FALLBACK_LIBRARY_PATH", directory.path().as_os_str()));
        assert!(!before.admits("DYLD_INSERT_LIBRARIES", directory.path().as_os_str()));
        assert_eq!(
            before.digest(),
            Inputs::of(&env, &identities)
                .expect("unchanged inputs")
                .digest()
        );
        std::fs::write(&path, super::tests::test_library(b"other"))
            .expect("changed bytes of the same length");
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("owned library")
            .set_modified(modified)
            .expect("restore mtime");
        assert_ne!(
            before.digest(),
            Inputs::of(&env, &identities)
                .expect("changed inputs")
                .digest()
        );
    }

    #[test]
    fn an_absent_fallback_directory_cannot_certify_a_later_library() {
        let directory = tempfile::tempdir().expect("owned loader parent");
        let search = directory.path().join("search");
        let mut env = Variables::default();
        env.set("DYLD_FALLBACK_LIBRARY_PATH", &search);
        let identities = Identities::empty();
        let absent = Inputs::of(&env, &identities).expect("observed absent directory");
        std::fs::create_dir_all(&search).expect("new search directory");
        std::fs::write(
            search.join("library"),
            super::tests::test_library(b"actual bytes"),
        )
        .expect("new library");
        assert_ne!(
            absent.digest(),
            Inputs::of(&env, &identities)
                .expect("present directory")
                .digest()
        );
    }

    #[test]
    fn relative_and_empty_search_entries_retain_typed_input_refusals() {
        for value in ["relative", ":/absolute"] {
            let mut env = Variables::default();
            env.set("DYLD_FALLBACK_LIBRARY_PATH", value);
            let error = Inputs::of(&env, &Identities::empty())
                .expect_err("an unbound search entry must be refused");
            assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);
            let named = error
                .get_ref()
                .and_then(|source| source.downcast_ref::<LoaderInputError>())
                .expect("named loader input cause");
            assert!(!named.path.is_absolute());
            assert_eq!(named.source.kind(), std::io::ErrorKind::Unsupported);
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod linux_tests {
    use super::Inputs;
    use crate::cargo::build_cache::toolchain::Identities;
    use crate::vars::Variables;

    #[test]
    fn the_actual_cargo_injected_linux_search_namespace_is_bound() {
        let env = Variables::of(njutest_devkit::paths::environment_for_a_run());
        let actual = env
            .var("LD_LIBRARY_PATH")
            .expect("the real Cargo test process carries its native loader namespace");
        assert!(
            !actual.is_empty(),
            "the actual native search namespace is nonempty"
        );
        let inputs = Inputs::of(&env, &Identities::empty()).expect("native loader capture");
        assert!(
            inputs.admits("LD_LIBRARY_PATH", actual),
            "the exact Cargo-injected Linux namespace must be bound for verified reuse"
        );
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos", windows)))]
mod namespace_tests {
    use super::{Inputs, RuntimeAttestation, RuntimeInputs, search_variables};
    use crate::cargo::build_cache::toolchain::Identities;
    use crate::vars::Variables;
    use std::ffi::OsStr;
    use std::path::Path;

    fn environment(name: &str, root: &Path) -> Variables {
        let mut env = Variables::default();
        env.set(name, root);
        env
    }

    /// Variables some platform's loader reads and no search binds, each beside whether this platform's loader is one of them.
    const READ_BY_THIS_LOADER: [(&str, bool); 3] = [
        ("LD_PRELOAD", cfg!(unix)),
        ("LD_UNKNOWN_NAMESPACE", cfg!(unix)),
        ("DYLD_INSERT_LIBRARIES", cfg!(target_os = "macos")),
    ];

    #[test]
    fn a_variable_is_refused_as_a_loader_input_only_where_this_platform_s_loader_reads_it() {
        let directory = tempfile::tempdir().expect("owned cwd");
        let identities = Identities::empty();
        let rustc = directory.path().join("rustc");
        let unloaded = Inputs::compiler(&Variables::default(), directory.path(), &identities)
            .expect("an environment naming no loader input");
        for (name, read) in READ_BY_THIS_LOADER {
            let env = environment(name, directory.path());
            let compiler = Inputs::compiler(&env, directory.path(), &identities);
            assert_eq!(
                compiler.is_err(),
                read,
                "{name} is refused by the loader capture exactly where this loader reads it: \
                 {compiler:?}"
            );
            let checked =
                super::super::toolchain::environment(directory.path(), &rustc, &env, &unloaded);
            assert_eq!(
                checked.is_err(),
                read,
                "{name} is refused by the compiler environment exactly where this loader reads \
                 it: {checked:?}"
            );
        }
    }

    #[test]
    fn only_a_refusal_to_traverse_a_mount_point_the_host_does_not_trust_leads_nowhere() {
        let untrusted = std::io::Error::from_raw_os_error(448);
        assert_eq!(
            super::untraversable(&untrusted),
            cfg!(windows),
            "ERROR_UNTRUSTED_MOUNT_POINT is a refusal to traverse on Windows and no code elsewhere"
        );
        for other in [
            std::io::Error::from(std::io::ErrorKind::NotFound),
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
            std::io::Error::from_raw_os_error(5),
            std::io::Error::other("unreadable"),
        ] {
            assert!(
                !super::untraversable(&other),
                "{other:?} is a failure to read, never a refusal to traverse"
            );
        }
        let refused = super::refused(Path::new("search"), untrusted).to_string();
        assert!(
            refused.contains("RM1024") && refused.contains("os error 448"),
            "a refusal names the operating system's code, which is never translated: {refused}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_search_path_naming_the_directories_the_loader_searches_first_reads_none_of_their_entries()
    {
        let directory = tempfile::tempdir().expect("an owned compile directory");
        let [system, sixteen, windows] =
            crate::capdir::fixed_search_directories().expect("the directories Windows names");
        let mut path = system.as_os_str().to_ascii_uppercase();
        for spelled in [windows.join(""), sixteen] {
            path.push(";");
            path.push(spelled.as_os_str());
        }
        let identities = Identities::empty();
        let first = Inputs::compiler(
            &environment("PATH", Path::new(&path)),
            directory.path(),
            &identities,
        )
        .expect("a search path of the directories the loader searches first binds");
        assert_eq!(
            identities.opens().expect("the libraries identified"),
            0,
            "the loader searches these directories before any search path, so their entries are \
             no loader input of the path's"
        );
        assert_eq!(
            identities.work().expect("the files read"),
            (0, 0, Vec::new()),
            "and no entry of theirs is read"
        );
        let respelled =
            Inputs::compiler(&environment("PATH", &system), directory.path(), &identities)
                .expect("the system directory spelled as Windows names it binds");
        assert_ne!(
            first.digest(),
            respelled.digest(),
            "a fixed directory is bound by how the path spells it"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_junction_this_user_made_on_the_search_path_is_bound_whether_or_not_the_host_traverses_it()
    {
        let directory = tempfile::tempdir().expect("owned search parent");
        let identities = Identities::empty();
        let target = directory.path().join("target");
        std::fs::create_dir_all(&target).expect("the junction's target");
        let library = target.join("library.dll");
        std::fs::write(&library, super::tests::test_library(b"one"))
            .expect("a library behind the junction");
        let junction = directory.path().join("junction");
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&target)
            .output()
            .expect("mklink runs");
        assert!(made.status.success(), "a junction was made: {made:?}");
        let env = environment("PATH", &junction);
        let first = Inputs::compiler(&env, directory.path(), &identities)
            .expect("a junction on the search path and in the directory compiled in binds");
        let again = Inputs::compiler(&env, directory.path(), &identities)
            .expect("the same namespace binds again");
        assert_eq!(first.digest(), again.digest());
        if std::fs::canonicalize(&junction).is_ok() {
            std::fs::write(&library, super::tests::test_library(b"two"))
                .expect("a changed library behind a junction the host traverses");
            assert_ne!(
                first.digest(),
                Inputs::compiler(&env, directory.path(), &identities)
                    .expect("the changed namespace binds")
                    .digest(),
                "where the host traverses the junction, what is behind it is bound"
            );
        }
    }

    #[test]
    fn every_native_search_variable_binds_contents_additions_removals_and_exact_values() {
        for &name in search_variables() {
            let directory = tempfile::tempdir().expect("owned native search directory");
            let library = directory.path().join("library");
            std::fs::write(
                &library,
                super::tests::test_library(b"original library bytes"),
            )
            .expect("original library");
            let env = environment(name, directory.path());
            let identities = Identities::empty();
            let original = RuntimeInputs::capture(&env, directory.path(), &identities)
                .expect("original namespace");
            let attestation: RuntimeAttestation = njutest_devkit::strictjson::decode_slice(
                &serde_json::to_vec(&original.attestation()).expect("original immutable record"),
            )
            .expect("retained original record");
            original.verify().expect("unchanged exact namespace");
            RuntimeInputs::restore(&attestation, &env, directory.path(), &identities)
                .expect("verified reuse");
            assert!(
                Inputs::compiler(&env, directory.path(), &identities)
                    .expect("compiler namespace")
                    .admits(name, directory.path().as_os_str())
            );
            assert!(
                !Inputs::compiler(&env, directory.path(), &identities)
                    .expect("compiler namespace")
                    .admits(name, OsStr::new("a different value"))
            );
            let added = directory.path().join("new-library");
            std::fs::write(&added, super::tests::test_library(b"new candidate"))
                .expect("new search candidate");
            original
                .verify()
                .expect_err("adding a search candidate invalidates the original namespace");
            RuntimeInputs::restore(&attestation, &env, directory.path(), &identities)
                .expect_err("the original publication must not be replaced");
            std::fs::remove_file(&added).expect("restore exact namespace");
            original
                .verify()
                .expect("unchanged exact namespace is reusable again");
            std::fs::remove_file(&library).expect("remove original candidate");
            original
                .verify()
                .expect_err("removing a search candidate invalidates the original namespace");
        }
    }

    #[test]
    fn an_original_attestation_refuses_same_stamp_content_changes_and_file_modes() {
        let directory = tempfile::tempdir().expect("owned search directory");
        let library = directory.path().join("library");
        std::fs::write(&library, super::tests::test_library(b"AAAA")).expect("original contents");
        let stamp = std::fs::metadata(&library)
            .expect("original metadata")
            .modified()
            .expect("original stamp");
        let env = environment(
            search_variables()
                .first()
                .expect("supported native variable"),
            directory.path(),
        );
        let identities = Identities::empty();
        let original =
            RuntimeInputs::capture(&env, directory.path(), &identities).expect("original capture");
        std::fs::write(&library, super::tests::test_library(b"BBBB"))
            .expect("same length replacement");
        std::fs::File::options()
            .write(true)
            .open(&library)
            .expect("owned library")
            .set_modified(stamp)
            .expect("restore same modified stamp");
        original
            .verify()
            .expect_err("content identity must not trust restored metadata");
        let changed = RuntimeInputs::capture(&env, directory.path(), &identities)
            .expect("changed original capture");
        let unchanged = std::fs::metadata(&library)
            .expect("owned mode")
            .permissions();
        let mut mode = unchanged.clone();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            mode.set_mode(mode.mode() ^ 0o100);
        }
        #[cfg(windows)]
        mode.set_readonly(!mode.readonly());
        std::fs::set_permissions(&library, mode).expect("change owned mode");
        changed.verify().expect_err("file mode is a bound input");
        std::fs::set_permissions(&library, unchanged).expect("owned cleanup");
    }

    #[test]
    fn a_settled_library_rewritten_under_its_old_mtime_is_bound_again() {
        let directory = tempfile::tempdir().expect("owned search directory");
        let library = directory.path().join("library");
        std::fs::write(&library, super::tests::test_library(b"AAAA")).expect("original contents");
        let modified = std::fs::metadata(&library)
            .expect("original metadata")
            .modified()
            .expect("original mtime");
        crate::cargo::build_cache::toolchain::tests::settle(&library);
        let env = environment(
            search_variables()
                .first()
                .expect("supported native variable"),
            directory.path(),
        );
        let identities = Identities::empty();
        let original =
            RuntimeInputs::capture(&env, directory.path(), &identities).expect("original capture");
        original
            .verify()
            .expect("a settled unchanged library is reused");
        std::fs::write(&library, super::tests::test_library(b"BBBB"))
            .expect("same length replacement");
        std::fs::File::options()
            .write(true)
            .open(&library)
            .expect("owned library")
            .set_modified(modified)
            .expect("restore the original mtime");
        original
            .verify()
            .expect_err("a write after a settled stamp is dated newer than it");
    }

    #[test]
    fn missing_roots_and_captured_cwd_are_original_inputs() {
        let directory = tempfile::tempdir().expect("owned cwd");
        let missing = directory.path().join("missing");
        let name = search_variables()
            .first()
            .expect("supported native variable");
        let env = environment(name, &missing);
        let identities = Identities::empty();
        let original =
            RuntimeInputs::capture(&env, directory.path(), &identities).expect("bound absence");
        original.verify().expect("unchanged absence");
        std::fs::create_dir_all(&missing).expect("missing root becomes present");
        original
            .verify()
            .expect_err("changed or unsupported original loader input is refused");
        let env = environment(name, Path::new("missing"));
        Inputs::of(&env, &identities).expect_err("unrecorded cwd remains opaque");
        let compiler = Inputs::compiler(&env, directory.path(), &identities)
            .expect("actual compiler cwd resolves relative root");
        assert!(compiler.admits(name, OsStr::new("missing")));
        let original = RuntimeInputs::capture(&env, directory.path(), &identities)
            .expect("actual captured cwd resolves relative root");
        original.verify().expect("same actual cwd namespace");
        let other = tempfile::tempdir().expect("different actual cwd");
        RuntimeInputs::restore(&original.attestation(), &env, other.path(), &identities)
            .expect_err("changed or unsupported original loader input is refused");
        std::fs::write(
            missing.join("library"),
            super::tests::test_library(b"new relative candidate"),
        )
        .expect("changed relative namespace");
        assert_ne!(
            compiler.digest(),
            Inputs::compiler(&env, directory.path(), &identities)
                .expect("changed compiler namespace")
                .digest()
        );
        original
            .verify()
            .expect_err("changed or unsupported original loader input is refused");
    }

    #[test]
    fn absent_empty_changed_unknown_injected_and_corrupt_inputs_are_not_interchangeable() {
        let directory = tempfile::tempdir().expect("owned cwd");
        let identities = Identities::empty();
        let mut env = Variables::default();
        let absent =
            RuntimeInputs::capture(&env, directory.path(), &identities).expect("absent namespace");
        env.set(
            search_variables()
                .first()
                .expect("supported native variable"),
            "",
        );
        RuntimeInputs::restore(&absent.attestation(), &env, directory.path(), &identities)
            .expect_err("changed or unsupported original loader input is refused");
        for (name, read) in READ_BY_THIS_LOADER {
            env.set(name, "foreign injection");
            let compiler = Inputs::compiler(&env, directory.path(), &identities);
            assert_eq!(
                compiler.is_err(),
                read,
                "{name} is an opaque compiler injection exactly where this loader reads it: \
                 {compiler:?}"
            );
            let runtime = RuntimeInputs::capture(&env, directory.path(), &identities);
            assert_eq!(
                runtime.is_err(),
                read,
                "{name} is an opaque runtime injection exactly where this loader reads it: \
                 {runtime:?}"
            );
            env.remove(name);
        }
        let mut bad = absent.attestation();
        bad.schema = 0;
        RuntimeInputs::restore(&bad, &Variables::default(), directory.path(), &identities)
            .expect_err("changed or unsupported original loader input is refused");
        bad.schema = 1;
        bad.digest.clear();
        RuntimeInputs::restore(&bad, &Variables::default(), directory.path(), &identities)
            .expect_err("changed or unsupported original loader input is refused");
        let mut record = serde_json::to_value(absent.attestation()).expect("original record");
        record
            .as_object_mut()
            .expect("typed record")
            .insert("unknown".to_owned(), serde_json::Value::Bool(true));
        njutest_devkit::strictjson::decode_slice::<RuntimeAttestation>(
            &serde_json::to_vec(&record).expect("serialized corrupt record"),
        )
        .expect_err("unknown original attestation field is refused");
    }

    #[test]
    fn the_original_opaque_toolchain_observation_admission_remains_refused() {
        let directory = tempfile::tempdir().expect("owned opaque observation graph");
        for name in ["LD_LIBRARY_PATH", "DYLD_LIBRARY_PATH"] {
            let env = environment(name, directory.path());
            let inputs = Inputs::observation(&env, &Identities::empty())
                .expect("original observation inputs");
            assert!(
                !inputs.admits(name, directory.path().as_os_str()),
                "original six-process refusal for {name}"
            );
        }
        #[cfg(target_os = "macos")]
        {
            let env = environment("DYLD_FALLBACK_LIBRARY_PATH", directory.path());
            let inputs = Inputs::observation(&env, &Identities::empty())
                .expect("original supported observation namespace");
            assert!(inputs.admits("DYLD_FALLBACK_LIBRARY_PATH", directory.path().as_os_str()));
        }
    }

    #[cfg(unix)]
    #[test]
    fn original_file_aliases_refuse_retargeting_and_new_targets() {
        let directory = tempfile::tempdir().expect("owned alias parent");
        let search = directory.path().join("search");
        std::fs::create_dir_all(&search).expect("owned search root");
        let first = directory.path().join("first");
        let second = directory.path().join("second");
        std::fs::write(&first, super::tests::test_library(b"same candidate bytes"))
            .expect("first candidate");
        std::fs::write(&second, super::tests::test_library(b"same candidate bytes"))
            .expect("second candidate");
        let alias = search.join("alias");
        std::os::unix::fs::symlink(&first, &alias).expect("original alias");
        let env = environment(
            search_variables()
                .first()
                .expect("supported native variable"),
            &search,
        );
        let identities = Identities::empty();
        let original = RuntimeInputs::capture(&env, directory.path(), &identities)
            .expect("original alias namespace");
        original.verify().expect("unchanged exact alias");
        std::fs::remove_file(&alias).expect("remove original alias");
        std::os::unix::fs::symlink(&second, &alias).expect("retarget same-content alias");
        original
            .verify()
            .expect_err("changed or unsupported original loader input is refused");
        std::fs::remove_file(&alias).expect("remove retargeted alias");
        let missing = directory.path().join("missing");
        std::os::unix::fs::symlink(&missing, &alias).expect("original absent alias target");
        let absent = RuntimeInputs::capture(&env, directory.path(), &identities)
            .expect("bound absent alias target");
        std::fs::write(&missing, super::tests::test_library(b"now present"))
            .expect("alias target becomes present");
        absent
            .verify()
            .expect_err("changed or unsupported original loader input is refused");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn native_linux_delimiters_empty_components_and_hwcaps_are_bound() {
        let directory = tempfile::tempdir().expect("actual cwd");
        let search = directory.path().join("search");
        let optimized = search.join("glibc-hwcaps/x86-64-v3");
        std::fs::create_dir_all(&optimized).expect("native hwcaps namespace");
        let library = optimized.join("library.so");
        std::fs::write(
            &library,
            super::tests::test_library(b"original hwcaps bytes"),
        )
        .expect("native optimized candidate");
        let mut env = Variables::default();
        env.set("LD_LIBRARY_PATH", format!("search;{}:", search.display()));
        let original = RuntimeInputs::capture(&env, directory.path(), &Identities::empty())
            .expect("both native delimiters and cwd component");
        original
            .verify()
            .expect("unchanged complete hwcaps namespace");
        std::fs::write(
            &library,
            super::tests::test_library(b"changed hwcaps bytes"),
        )
        .expect("changed optimized candidate");
        original
            .verify()
            .expect_err("changed or unsupported original loader input is refused");
        env.set("LD_LIBRARY_PATH", "$ORIGIN/relative");
        RuntimeInputs::capture(&env, directory.path(), &Identities::empty())
            .expect_err("changed or unsupported original loader input is refused");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_native_framework_namespace_binds_nested_candidates_and_ancestor_aliases() {
        let directory = tempfile::tempdir().expect("native framework root");
        let version = directory.path().join("Actual.framework/Versions/A");
        std::fs::create_dir_all(&version).expect("actual framework version");
        let library = version.join("Actual");
        std::fs::write(
            &library,
            super::tests::test_library(b"original framework bytes"),
        )
        .expect("framework candidate");
        std::os::unix::fs::symlink("A", version.parent().expect("versions").join("Current"))
            .expect("native version alias");
        std::os::unix::fs::symlink(directory.path(), version.join("ancestor"))
            .expect("ancestor alias");
        let env = environment("DYLD_FRAMEWORK_PATH", directory.path());
        let original = RuntimeInputs::capture(&env, directory.path(), &Identities::empty())
            .expect("complete original framework graph");
        original.verify().expect("unchanged framework aliases");
        std::fs::write(
            &library,
            super::tests::test_library(b"changed framework bytes"),
        )
        .expect("changed framework candidate");
        original
            .verify()
            .expect_err("changed or unsupported original loader input is refused");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_darwin_empty_components_retain_the_original_opaque_refusal() {
        let directory = tempfile::tempdir().expect("actual captured cwd");
        for &name in search_variables() {
            for value in [
                String::new(),
                ":".to_owned(),
                format!("{}:", directory.path().display()),
            ] {
                let mut env = Variables::default();
                env.set(name, value);
                let error = RuntimeInputs::capture(&env, directory.path(), &Identities::empty())
                    .expect_err("a Darwin root search must not be rebound to captured cwd");
                assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);
            }
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_darwin_at_paths_require_original_executable_context() {
        let directory = tempfile::tempdir().expect("actual captured cwd");
        for name in [
            "DYLD_LIBRARY_PATH",
            "DYLD_FALLBACK_LIBRARY_PATH",
            "DYLD_FRAMEWORK_PATH",
            "DYLD_FALLBACK_FRAMEWORK_PATH",
        ] {
            for value in [
                "@rpath/overrides",
                "@loader_path/overrides",
                "@executable_path/overrides",
                "@unknown/overrides",
            ] {
                let mut env = Variables::default();
                env.set(name, value);
                let error = RuntimeInputs::capture(&env, directory.path(), &Identities::empty())
                    .expect_err("a Darwin runpath stack must not be rebound to captured cwd");
                assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);
            }
        }
    }
}
