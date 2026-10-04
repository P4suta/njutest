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
        super::field(&mut digest, b"native-library-inputs-v2");
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

const fn search_variables() -> &'static [&'static str] {
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
            && (name.as_encoded_bytes().starts_with(b"LD_")
                || name.as_encoded_bytes().starts_with(b"DYLD_"))
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
    let canonical = match std::fs::canonicalize(path) {
        Ok(canonical) => canonical,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            super::field(digest, b"absent");
            return Ok(());
        }
        Err(source) => return Err(source),
    };
    super::field(digest, canonical.as_os_str().as_encoded_bytes());
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
    if entries != after
        || Dir::open(&std::fs::canonicalize(path)?)?.status()?.identity != before.identity
    {
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
    let status = directory
        .status_at(name)?
        .ok_or_else(|| io::Error::other("a loader search entry disappeared"))?;
    super::field(digest, text.as_bytes());
    let descend = frameworks || (cfg!(target_os = "linux") && text == "glibc-hwcaps");
    match status.kind {
        Kind::Directory => {
            super::field(digest, b"directory");
            if descend {
                capture(&path, identities, digest, (true, ancestors))?;
            }
        }
        Kind::File => match directory.open_entry(name)? {
            Entry::File(file) => captured_file(&path, &file, identities, digest)?,
            Entry::Dir(_) | Entry::Other => {
                return Err(io::Error::other("the loader search entry changed kind"));
            }
        },
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
    match std::fs::canonicalize(path) {
        Ok(canonical) => {
            let file = crate::capdir::open_file_at(&canonical)?;
            let status = crate::capdir::file_status(&file)?;
            match status.kind {
                Kind::File => captured_file(&canonical, &file, identities, digest)?,
                Kind::Directory => {
                    super::field(digest, b"directory-alias");
                    super::field(digest, canonical.as_os_str().as_encoded_bytes());
                    super::field(digest, &status.identity.volume.to_be_bytes());
                    super::field(digest, &status.identity.object.to_be_bytes());
                    if descend {
                        capture(&canonical, identities, digest, (true, ancestors))?;
                    }
                }
                Kind::Other => {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "a loader alias target is not a file or directory",
                    ));
                }
            }
            if std::fs::canonicalize(path)? != canonical
                || crate::capdir::file_status(&file)? != status
            {
                return Err(io::Error::other("the loader alias target changed"));
            }
            Ok(())
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            super::field(digest, b"absent-link-target");
            Ok(())
        }
        Err(source) => Err(source),
    }
}

fn captured_file(
    path: &Path,
    file: &std::fs::File,
    identities: &Identities,
    digest: &mut Sha256,
) -> io::Result<()> {
    let held = crate::capdir::file_status(file)?;
    match held.kind {
        Kind::File => {}
        Kind::Directory | Kind::Other => {
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
    super::field(digest, path.as_os_str().as_encoded_bytes());
    super::field(digest, state.digest.as_bytes());
    super::field(digest, &state.mode.to_be_bytes());
    Ok(())
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
#[error("{code}: the loader input {} could not be bound ({:?})", path.display(), source.kind(), code = LoaderInputError::code().code)]
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
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
        std::fs::write(&library, b"actual library input").expect("owned library target");
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
    fn the_native_system_library_namespace_has_complete_typed_inputs() {
        let mut env = Variables::default();
        env.set("DYLD_FALLBACK_LIBRARY_PATH", "/usr/lib");
        Inputs::of(&env, &Identities::empty()).expect("actual native system library namespace");
    }

    #[test]
    fn a_fallback_library_change_is_bound_even_with_its_original_mtime() {
        let directory = tempfile::tempdir().expect("owned loader search");
        let path = directory.path().join("libcandidate.dylib");
        std::fs::write(&path, b"first").expect("original library");
        let modified = std::fs::metadata(&path)
            .expect("original metadata")
            .modified()
            .expect("original mtime");
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
        std::fs::write(&path, b"other").expect("changed bytes of the same length");
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
        std::fs::write(search.join("library"), b"actual bytes").expect("new library");
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

    #[test]
    fn every_native_search_variable_binds_contents_additions_removals_and_exact_values() {
        for &name in search_variables() {
            let directory = tempfile::tempdir().expect("owned native search directory");
            let library = directory.path().join("library");
            std::fs::write(&library, b"original library bytes").expect("original library");
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
            std::fs::write(&added, b"new candidate").expect("new search candidate");
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
        std::fs::write(&library, b"AAAA").expect("original contents");
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
        std::fs::write(&library, b"BBBB").expect("same length replacement");
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
        let mut mode = std::fs::metadata(&library)
            .expect("owned mode")
            .permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            mode.set_mode(mode.mode() ^ 0o100);
        }
        #[cfg(windows)]
        mode.set_readonly(!mode.readonly());
        std::fs::set_permissions(&library, mode).expect("change owned mode");
        changed.verify().expect_err("file mode is a bound input");
        #[cfg(windows)]
        {
            let mut mode = std::fs::metadata(&library)
                .expect("cleanup mode")
                .permissions();
            mode.set_readonly(false);
            std::fs::set_permissions(&library, mode).expect("owned cleanup");
        }
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
        std::fs::write(missing.join("library"), b"new relative candidate")
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
        env.set("LD_PRELOAD", "foreign injection");
        Inputs::compiler(&env, directory.path(), &identities)
            .expect_err("opaque compiler injection");
        RuntimeInputs::capture(&env, directory.path(), &identities)
            .expect_err("changed or unsupported original loader input is refused");
        env.remove("LD_PRELOAD");
        env.set("DYLD_INSERT_LIBRARIES", "foreign injection");
        RuntimeInputs::capture(&env, directory.path(), &identities)
            .expect_err("changed or unsupported original loader input is refused");
        env.remove("DYLD_INSERT_LIBRARIES");
        env.set("LD_UNKNOWN_NAMESPACE", "unknown loader input");
        Inputs::compiler(&env, directory.path(), &identities)
            .expect_err("unknown compiler loader input");
        RuntimeInputs::capture(&env, directory.path(), &identities)
            .expect_err("changed or unsupported original loader input is refused");
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
        std::fs::write(&first, b"same candidate bytes").expect("first candidate");
        std::fs::write(&second, b"same candidate bytes").expect("second candidate");
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
        std::fs::write(&missing, b"now present").expect("alias target becomes present");
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
        std::fs::write(&library, b"original hwcaps bytes").expect("native optimized candidate");
        let mut env = Variables::default();
        env.set("LD_LIBRARY_PATH", format!("search;{}:", search.display()));
        let original = RuntimeInputs::capture(&env, directory.path(), &Identities::empty())
            .expect("both native delimiters and cwd component");
        original
            .verify()
            .expect("unchanged complete hwcaps namespace");
        std::fs::write(&library, b"changed hwcaps bytes").expect("changed optimized candidate");
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
        std::fs::write(&library, b"original framework bytes").expect("framework candidate");
        std::os::unix::fs::symlink("A", version.parent().expect("versions").join("Current"))
            .expect("native version alias");
        std::os::unix::fs::symlink(directory.path(), version.join("ancestor"))
            .expect("ancestor alias");
        let env = environment("DYLD_FRAMEWORK_PATH", directory.path());
        let original = RuntimeInputs::capture(&env, directory.path(), &Identities::empty())
            .expect("complete original framework graph");
        original.verify().expect("unchanged framework aliases");
        std::fs::write(&library, b"changed framework bytes").expect("changed framework candidate");
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
