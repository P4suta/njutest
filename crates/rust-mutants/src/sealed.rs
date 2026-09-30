// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The sealed build: the instrumented snapshot compiled for `wasm32-wasip1`, and what each of its test modules holds (ADR 0046).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::cargo::{CargoError, Compiled, Package, Unit};
use crate::execute::TestTarget;

/// The target a sealed build compiles for.
pub const TARGET: &str = "wasm32-wasip1";

/// The transcript store's directory below the caller's cache root.
pub const TRANSCRIPTS_LAYOUT: &str = rust_mutants_sealed::TRANSCRIPTS_LAYOUT;

/// The builds that make the sealed modules, in order: every test harness but an example's, which keeps going past one that refuses, then the examples.
pub const BUILDS: [crate::cargo::CompileKind; 2] = [
    crate::cargo::CompileKind::SealedTests,
    crate::cargo::CompileKind::SealedExamples,
];

/// Why a native test target has no sealed module, so that nothing only it reaches can be judged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Unsealed {
    /// The run was not asked to seal anything.
    NotAsked,
    /// The toolchain holds no standard library for the sealed target.
    TargetMissing,
    /// The sealed build produced no module for the target: it does not build for the sealed target.
    NotBuilt,
    /// rustdoc's report of the doctests it built for the sealed target did not account for every binary it handed on, or a merged binary did not name the doctests it holds.
    DoctestsUnaccounted,
    /// No native baseline ran the target, so which of its tests the suite runs is not known.
    NotVerified,
    /// The target's native baseline did not name every test it ran, so which of its tests the suite runs is not known.
    NativeUnnamed,
    /// The target is a procedural macro's tests, which cargo builds for the host whatever target it is given.
    ProcMacro,
    /// The module's harness did not list its tests on the sealed host.
    NotListed,
    /// The target has no libtest harness: it answers by its exit code alone, which no sealed execution can tell from a test that exited early.
    NoHarness,
    /// Which flags cargo compiles the sealed target with is not known, so the link arguments the host needs cannot be added to them: the configuration could not be read, or a `cfg(…)` table's predicate names what a target alone does not decide.
    FlagsUnmerged,
    /// The toolchain's standard library could not be made to answer the temporary and the home directory from the environment: the object that answers them did not build, or the probe linked with it did not answer as a standard library that reads `TMPDIR` and `HOME` does.
    PlatformUnanswered,
}

impl Unsealed {
    /// The name a report spells it with.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::NotAsked => "not-asked",
            Self::TargetMissing => "target-missing",
            Self::NotBuilt => "not-built",
            Self::DoctestsUnaccounted => "doctests-unaccounted",
            Self::NotVerified => "not-verified",
            Self::NativeUnnamed => "native-unnamed",
            Self::ProcMacro => "proc-macro",
            Self::NotListed => "not-listed",
            Self::NoHarness => "no-harness",
            Self::FlagsUnmerged => "flags-unmerged",
            Self::PlatformUnanswered => "platform-unanswered",
        }
    }

    /// What to do so that what this names seals.
    #[must_use]
    pub const fn remedy(self) -> &'static str {
        match self {
            Self::NotAsked => "ask the run to seal what it measures",
            Self::TargetMissing => "rustup target add wasm32-wasip1",
            Self::NotBuilt => {
                "make the target's tests build for wasm32-wasip1, which the sealed build's own \
                 error names, or put what cannot build behind cfg(not(target_family = \"wasm\"))"
            }
            Self::NotVerified => "let the run verify its baselines, which --no-verify skips",
            Self::NativeUnnamed => {
                "keep libtest's own report of the target whole: a test that prints over it, or a \
                 harness that reports another way, leaves its tests unnamed"
            }
            Self::DoctestsUnaccounted => {
                "run `cargo test --doc --target wasm32-wasip1` to see what rustdoc reported of the \
                 doctests, or what a merged binary printed before it announced them"
            }
            Self::ProcMacro => {
                "a procedural macro runs in the compiler, so what only its own tests reach rests on \
                 their native lead"
            }
            Self::NotListed => {
                "run the module with --list on wasmtime to see why its harness did not list its tests"
            }
            Self::NoHarness => {
                "give the target libtest's harness, which `harness = false` takes away, so that each \
                 of its tests says how it ended"
            }
            Self::FlagsUnmerged => {
                "configure the flags of wasm32-wasip1 under its own triple, or in build.rustflags, \
                 rather than under a cfg(…) predicate cargo alone decides, and keep every cargo \
                 configuration file readable"
            }
            Self::PlatformUnanswered => {
                "the run's trace says why the toolchain's standard library did not answer its \
                 temporary and home directories from TMPDIR and HOME; a toolchain whose \
                 std::env::temp_dir and std::env::home_dir the probe finds and rewrites seals"
            }
        }
    }
}

/// The compiler and rustdoc flags a sealed build uses, each encoded as cargo reads `CARGO_ENCODED_RUSTFLAGS`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flags {
    /// What every sealed module is compiled with.
    pub compile: Vec<String>,
    /// What every doctest built for the sealed target is compiled with.
    pub document: Vec<String>,
}

impl Flags {
    /// The flags the sealed build uses: what the tree already compiles and documents the sealed target with, as cargo would choose them from `env` and the configuration `layered`, judged against the sealed target's `facts`, and then `linked`, each a linker argument every sealed module needs.
    ///
    /// # Errors
    /// [`Unsealed::FlagsUnmerged`] where the configuration could not be read, a flag variable is not text, or a `cfg(…)` table's predicate is not one `facts` decide, so that which flags cargo would use is not known.
    pub fn of(
        env: &crate::vars::Variables,
        layered: &crate::cargo::config::Layered,
        facts: &crate::facts::Facts,
        linked: &[String],
    ) -> Result<Self, Unsealed> {
        if layered.unreadable {
            return Err(Unsealed::FlagsUnmerged);
        }
        let mut compile_targeted: Option<Vec<String>> = None;
        let mut document_targeted: Option<Vec<String>> = None;
        for targeted in &layered.targets {
            let applies = if targeted.key == TARGET {
                true
            } else if let Some(rest) = targeted.key.strip_prefix("cfg(") {
                let predicate = rest.strip_suffix(')').ok_or(Unsealed::FlagsUnmerged)?;
                crate::facts::Predicate::parse(predicate)
                    .map_err(|_undecided| Unsealed::FlagsUnmerged)?
                    .holds(facts)
            } else {
                false
            };
            if !applies {
                continue;
            }
            if let Some(flags) = &targeted.rustflags {
                compile_targeted
                    .get_or_insert_with(Vec::new)
                    .extend(flags.iter().cloned());
            }
            if let Some(flags) = &targeted.rustdocflags {
                document_targeted
                    .get_or_insert_with(Vec::new)
                    .extend(flags.iter().cloned());
            }
        }
        let inherited = |names| {
            crate::cargo::config::inherited_as(env, names)
                .map_err(|_not_text| Unsealed::FlagsUnmerged)
        };
        let mut compile = match inherited((
            crate::cargo::config::ENCODED_RUSTFLAGS,
            crate::cargo::config::RUSTFLAGS,
        ))? {
            Some(flags) => flags,
            None => match compile_targeted {
                Some(targeted) => targeted,
                None => layered.build.clone(),
            },
        };
        let mut document = match inherited((ENCODED_RUSTDOCFLAGS, RUSTDOCFLAGS))? {
            Some(flags) => flags,
            None => match document_targeted {
                Some(targeted) => targeted,
                None => layered.build_doc.clone(),
            },
        };
        compile.extend(linked.iter().cloned());
        document.extend(linked.iter().cloned());
        Ok(Self { compile, document })
    }

    /// The environment a sealed build adds for these flags: each in its encoded variable, and its plain one empty.
    #[must_use]
    pub fn environment(&self) -> crate::vars::Variables {
        let separator = crate::cargo::config::SEPARATOR.to_string();
        crate::vars::Variables::of([
            (
                std::ffi::OsString::from(crate::cargo::config::ENCODED_RUSTFLAGS),
                std::ffi::OsString::from(self.compile.join(&separator)),
            ),
            (
                std::ffi::OsString::from(crate::cargo::config::RUSTFLAGS),
                std::ffi::OsString::new(),
            ),
            (
                std::ffi::OsString::from(ENCODED_RUSTDOCFLAGS),
                std::ffi::OsString::from(self.document.join(&separator)),
            ),
            (
                std::ffi::OsString::from(RUSTDOCFLAGS),
                std::ffi::OsString::new(),
            ),
        ])
    }
}

/// The variable cargo reads rustdoc's flags from in their encoded form, before [`RUSTDOCFLAGS`].
pub const ENCODED_RUSTDOCFLAGS: &str = "CARGO_ENCODED_RUSTDOCFLAGS";

/// The plain form of rustdoc's flags.
pub const RUSTDOCFLAGS: &str = "RUSTDOCFLAGS";

/// The linker arguments every sealed module is built with so the host can start it in its package's directory, spelled as `rustc` takes one.
#[must_use]
pub fn start_linked() -> Vec<String> {
    rust_mutants_sealed::START_LINK_ARGS
        .iter()
        .map(|argument| format!("-Clink-arg={argument}"))
        .collect()
}

/// Whether a preparation builds the sealed modules beside the native build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sealing {
    /// It builds none, so every standing rests on native executions, which are leads.
    Off,
    /// It builds them.
    On,
}

/// Whether the toolchain whose sysroot is `sysroot` holds the sealed target's standard library.
///
/// # Errors
/// A directory that is there and could not be listed, which says nothing about the target.
pub fn installed(sysroot: &Path) -> std::io::Result<bool> {
    let libdir = sysroot.join("lib").join("rustlib").join(TARGET).join("lib");
    let entries = match std::fs::read_dir(&libdir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let path = PathBuf::from(entry?.file_name());
        let standard = path
            .file_stem()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|stem| stem.starts_with("libstd-"));
        let archive = path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("rlib"));
        if standard && archive {
            return Ok(true);
        }
    }
    Ok(false)
}

/// One test module of the sealed build: the sealed build of one native test target.
#[derive(Debug, Clone)]
pub struct Module {
    /// The target as the sealed build built it, whose executable is the module.
    pub target: TestTarget,
    /// Every source file compiled into it, absolute.
    pub sources: BTreeSet<PathBuf>,
}

/// One library's doctests, as the sealed build captured them.
#[derive(Debug, Clone)]
pub struct Doctests {
    /// The native documentation target they are the sealed build of.
    pub target: TestTarget,
    /// Every binary rustdoc handed on, and which doctest each holds.
    pub captured: doctest::Captured,
    /// Every source file compiled into the library they link, absolute.
    pub sources: BTreeSet<PathBuf>,
}

/// What the sealed build produced for each native test target, and why it produced nothing for the rest.
#[derive(Debug, Clone, Default)]
pub struct SealedBuild {
    /// The build cache's claim, shared until the last copy of these modules is dropped.
    owner: Option<std::sync::Arc<BuildClaim>>,
    /// Each native target's sealed module, by target identity.
    pub modules: BTreeMap<String, Module>,
    /// Each native documentation target's captured doctests, by target identity.
    pub doctests: BTreeMap<String, Doctests>,
    /// Each native target with no sealed module, by target identity, and why.
    pub unsealed: BTreeMap<String, Unsealed>,
}

/// A sealed build cache's claim, released when the last shared set of its modules is dropped.
#[derive(Debug)]
pub(crate) struct BuildClaim {
    owner: crate::tempowner::Owner,
}

impl BuildClaim {
    /// A claim that writes its release whenever the build or its last modules let it go.
    pub(crate) const fn new(owner: crate::tempowner::Owner) -> Self {
        Self { owner }
    }
}

impl Drop for BuildClaim {
    fn drop(&mut self) {
        match self.owner.release() {
            Ok(()) => {}
            Err(_unreleased) => {}
        }
    }
}

impl SealedBuild {
    /// A tree with nothing sealed, each of `native` for `why`.
    #[must_use]
    pub fn none(native: &[TestTarget], why: Unsealed) -> Self {
        Self {
            owner: None,
            modules: BTreeMap::new(),
            doctests: BTreeMap::new(),
            unsealed: native
                .iter()
                .map(|target| (target.id().to_owned(), why))
                .collect(),
        }
    }

    /// Keeps the build cache claimed while these modules may be read or run.
    pub(crate) fn keeping(mut self, owner: Option<std::sync::Arc<BuildClaim>>) -> Self {
        self.owner = owner;
        self
    }

    /// What the sealed builds, each of [`BUILDS`] as `compiled` into `target_dir`, and each documentation target's captured doctests or why there are none, answer for each of `native`.
    ///
    /// # Errors
    /// What reading the sealed build's targets refuses: a manifest that cannot be read.
    pub fn of(
        native: &[TestTarget],
        (compiled, mut captured): (
            &[Compiled],
            BTreeMap<String, Result<doctest::Captured, Unsealed>>,
        ),
        (packages, target_dir): (&[Package], &Path),
    ) -> Result<Self, CargoError> {
        let mut modules = BTreeMap::new();
        let laid_out = target_dir.join(TARGET);
        for build in compiled {
            for target in crate::execute::targets_of(&build.messages, packages, &laid_out)? {
                if target.kind() == crate::execute::TargetKind::ProcMacro || !target.harness {
                    continue;
                }
                let sources = sources_of(&target, &build.units, packages);
                modules.insert(target.id().to_owned(), Module { target, sources });
            }
        }
        let mut doctests = BTreeMap::new();
        let mut refused = BTreeMap::new();
        for target in native
            .iter()
            .filter(|target| target.kind() == crate::execute::TargetKind::Doc)
        {
            match captured.remove(target.id()) {
                Some(Ok(captured)) => {
                    let sources = compiled
                        .iter()
                        .flat_map(|build| library_sources(target, &build.units, packages))
                        .collect();
                    doctests.insert(
                        target.id().to_owned(),
                        Doctests {
                            target: target.clone(),
                            captured,
                            sources,
                        },
                    );
                }
                Some(Err(why)) => {
                    refused.insert(target.id().to_owned(), why);
                }
                None => {}
            }
        }
        let mut unsealed: BTreeMap<String, Unsealed> = native
            .iter()
            .filter(|target| {
                !modules.contains_key(target.id()) && !doctests.contains_key(target.id())
            })
            .map(|target| {
                let why = match target.kind() {
                    crate::execute::TargetKind::Lib
                    | crate::execute::TargetKind::Bin
                    | crate::execute::TargetKind::Test
                    | crate::execute::TargetKind::Example
                        if !target.harness =>
                    {
                        Unsealed::NoHarness
                    }
                    crate::execute::TargetKind::ProcMacro => Unsealed::ProcMacro,
                    crate::execute::TargetKind::Doc
                    | crate::execute::TargetKind::Lib
                    | crate::execute::TargetKind::Bin
                    | crate::execute::TargetKind::Test
                    | crate::execute::TargetKind::Example => Unsealed::NotBuilt,
                };
                (target.id().to_owned(), why)
            })
            .collect();
        unsealed.extend(refused);
        modules.retain(|id, _| native.iter().any(|target| target.id() == id));
        Ok(Self {
            owner: None,
            modules,
            doctests,
            unsealed,
        })
    }

    /// Whether any module or doctest of the sealed build compiled `source`.
    #[must_use]
    pub fn holds(&self, source: &Path) -> bool {
        self.modules
            .values()
            .any(|module| module.sources.contains(source))
            || self
                .doctests
                .values()
                .any(|doctests| doctests.sources.contains(source))
    }

    /// Whether the sealed build compiled `source` into the module of `target`.
    #[must_use]
    pub fn compiled(&self, target: &str, source: &Path) -> bool {
        self.modules
            .get(target)
            .is_some_and(|module| module.sources.contains(source))
    }
}

/// Every source file the compiler read for the library the documentation target `target` documents, which every doctest links.
fn library_sources(target: &TestTarget, units: &[Unit], packages: &[Package]) -> BTreeSet<PathBuf> {
    units
        .iter()
        .filter(|unit| !unit.test && unit.target.is_lib() && unit.target.name == target.name())
        .filter(|unit| {
            packages
                .iter()
                .any(|package| package.id == unit.package_id && package.name == target.package())
        })
        .flat_map(|unit| unit.sources.iter().cloned())
        .collect()
}

/// Every source file the compiler read for the test unit `target` is the module of.
fn sources_of(target: &TestTarget, units: &[Unit], packages: &[Package]) -> BTreeSet<PathBuf> {
    units
        .iter()
        .filter(|unit| unit.test && unit.target.name == target.name())
        .filter(|unit| {
            packages
                .iter()
                .any(|package| package.id == unit.package_id && package.name == target.package())
        })
        .flat_map(|unit| unit.sources.iter().cloned())
        .collect()
}

pub mod bench;
pub mod doctest;
pub mod platform;
pub mod record;
pub mod rerun;
pub mod standing;

#[cfg(test)]
mod tests;
