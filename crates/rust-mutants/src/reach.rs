// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Which test targets could have observed a mutant at all.
//!
//! A target whose run never executed the line a mutant sits on cannot have
//! noticed it, so running it proves nothing and costs a process. This layer
//! measures that once, on the pristine tree, and every later execution is
//! routed by it.
//!
//! Everything here fails open into *more* work, never less: a build that will
//! not instrument, tools that are not installed, a project that configures its
//! own compiler flags — each leaves the measurement empty, and an empty
//! measurement routes every mutant to every target, exactly as if this layer
//! did not exist.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::EngineError;
use crate::cargo::config::Configured;
use crate::cargo::{CompileKind, CompileOptions, compile, config};
use crate::coverage::{
    Block, Point, Tools, covered, instrumented, profile_pattern, written_profiles,
};
use crate::execute::{self, Context, ExecRequest};
use crate::runner::{Cancel, Watched};
use crate::session::PrepareOptions;
use crate::trace::Recorder;
use crate::workspace::{SessionError, Workspace};

/// The limitation a session states when a cargo configuration file could not be parsed.
pub use crate::limitation::CARGO_CONFIGURATION_UNREADABLE as UNREADABLE_CONFIGURATION;
/// The limitation a session states when the tree could not be built with instrumentation.
pub use crate::limitation::COVERAGE_BUILD_FAILED as UNBUILDABLE;
/// The limitation a session states when the tools ran and said nothing usable.
pub use crate::limitation::COVERAGE_NOT_MEASURED as UNMEASURED;
/// The limitation a session states when the project configures compiler flags for a target, which a coverage build cannot put back.
pub use crate::limitation::COVERAGE_REFUSED_CONFIGURED_RUSTFLAGS as CONFIGURED_FLAGS;
/// The limitation a session states when the LLVM tools are not installed.
pub use crate::limitation::COVERAGE_TOOLS_MISSING as TOOLS_MISSING;

/// The variable a coverage build's flags are put in, which is the encoded form so a value with a space cannot become two flags.
const ENCODED_RUSTFLAGS: &str = "CARGO_ENCODED_RUSTFLAGS";

/// The plain form, which cargo ignores when the encoded one is set.
const RUSTFLAGS: &str = "RUSTFLAGS";

/// What each target reached, and what the measurement could not establish.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Reached {
    /// Every target that ran, by identity, with the blocks its run covered. Empty when nothing was measured.
    pub targets: BTreeMap<String, BTreeSet<Block>>,
    /// Every block the coverage build instrumented at all, whether or not it ran. A place outside this is a place the measurement says nothing about — code in another binary, code the instrumented build did not compile — and nothing about it may be concluded.
    pub instrumented: BTreeSet<Block>,
    /// Why the measurement is not what it could be, in the order it was found out.
    pub limitations: Vec<String>,
}

impl Reached {
    /// Whether anything was measured at all. Nothing measured routes every mutant to every target.
    #[must_use]
    pub fn measured(&self) -> bool {
        !self.targets.is_empty()
    }

    /// Whether it reached every target it set out to, which is what makes it worth remembering.
    ///
    /// A measurement names the targets it could not read. One that names any
    /// is a measurement of some of them: sound to route by, because what it
    /// could not read stays in every route, and wrong to keep, because a later
    /// run would have nothing to tell it from a whole one.
    #[must_use]
    pub fn whole(&self) -> bool {
        self.limitations
            .iter()
            .all(|limitation| !limitation.starts_with(UNMEASURED))
    }

    /// The targets whose run covered `position` in `path`, in identity order, or nothing at all when the measurement never instrumented that place and so says nothing about it.
    #[must_use]
    pub fn covering(&self, path: &Path, position: Point) -> Option<Vec<&str>> {
        if !self
            .instrumented
            .iter()
            .any(|block| block.contains(path, position))
        {
            return None;
        }
        Some(
            self.targets
                .iter()
                .filter(|(_, blocks)| blocks.iter().any(|block| block.contains(path, position)))
                .map(|(id, _)| id.as_str())
                .collect(),
        )
    }
}

/// What the measurement is made of: the tree, and how its build is bounded.
#[derive(Debug, Clone, Copy)]
pub struct Asking<'a> {
    /// The snapshot to build and run.
    pub workspace: &'a Workspace,
    /// How the build is bounded.
    pub options: &'a PrepareOptions,
}

/// Measures which target reached what, on the tree as it stands.
///
/// # Errors
/// Only a failure to make the directory the profiles are written to, which
/// no later phase could work around.
pub fn establish(
    asking: &Asking<'_>,
    cancel: &Cancel,
    trace: &Recorder,
) -> Result<Reached, EngineError> {
    let Asking { workspace, options } = *asking;
    if !options.coverage {
        return Ok(Reached::default());
    }
    let phase = trace.phase("coverage");
    let reached = measure(workspace, options, cancel, trace);
    phase.end();
    reached
}

fn measure(
    workspace: &Workspace,
    options: &PrepareOptions,
    cancel: &Cancel,
    trace: &Recorder,
) -> Result<Reached, EngineError> {
    let root = workspace.snapshot_root();
    let flags = config::configured(root, config::home(&workspace.base_env).as_deref());
    if flags.unreadable {
        return Ok(refused(UNREADABLE_CONFIGURATION, trace));
    }
    if flags.target_specific {
        return Ok(refused(CONFIGURED_FLAGS, trace));
    }
    let target_dir = workspace.target_dir.join("coverage");
    let built = compile(
        &workspace.driver(cancel),
        &CompileOptions {
            kind: CompileKind::Tests,
            packages: options.packages.clone(),
            target_dir: Some(target_dir.clone()),
            locked: workspace.locked,
            offline: workspace.offline,
            timeout: Workspace::timeout(options.build_timeout),
            env: instrumenting(&workspace.base_env, &flags),
            build: options.build.clone(),
        },
    );
    let Ok(built) = built else {
        return Ok(refused(UNBUILDABLE, trace));
    };
    if !built.success {
        return Ok(refused(UNBUILDABLE, trace));
    }
    let targets = execute::targets_of(
        &built.messages,
        &workspace.metadata.packages,
        Some(&target_dir),
    );
    if targets.is_empty() {
        return Ok(refused(UNMEASURED, trace));
    }
    let watch = Watched::new(cancel, &workspace.trace);
    let Ok(tools) = Tools::locate(&workspace.toolchain, root, &watch) else {
        return Ok(refused(TOOLS_MISSING, trace));
    };
    let profiles = target_dir.join("profiles");
    std::fs::create_dir_all(&profiles).map_err(|source| SessionError::WriteFailed {
        path: profiles.display().to_string(),
        source,
    })?;

    let binaries = executables(&built.messages);
    let reached = run_targets(
        &Reading {
            tools: &tools,
            profiles: &profiles,
            root,
            binaries: &binaries,
            watch: &watch,
        },
        &targets,
        (workspace, options),
        cancel,
    );
    if reached.targets.is_empty() {
        return Ok(refused(UNMEASURED, trace));
    }
    trace.note(
        "coverage",
        &format!(
            "{} targets measured, {} blocks",
            reached.targets.len(),
            reached.targets.values().map(BTreeSet::len).sum::<usize>()
        ),
    );
    Ok(reached)
}

/// Runs every target once with nothing active and reads back what each covered.
///
/// A measurement that stops early says which targets it never reached. A
/// partial measurement that does not is one a route reads as "these targets
/// ran and covered nothing", which is the difference between a mutant nobody
/// could notice and a mutant nobody looked at.
fn run_targets(
    reading: &Reading<'_>,
    targets: &[execute::TestTarget],
    within: (&Workspace, &PrepareOptions),
    cancel: &Cancel,
) -> Reached {
    let (workspace, options) = within;
    let mut reached = Reached::default();
    for (at, target) in targets.iter().enumerate() {
        if cancel.is_cancelled() {
            reached.limitations.extend(
                targets
                    .get(at..)
                    .unwrap_or_default()
                    .iter()
                    .map(|left| format!("{UNMEASURED}:{}", left.id)),
            );
            break;
        }
        let pattern = profile_pattern(reading.profiles, &key(target));
        let context = Context {
            base_env: &workspace.base_env,
            cargo: Some(workspace.toolchain.cargo()),
            sysroot: workspace.toolchain.sysroot(),
            active: None,
            probe: None,
            profile: Some(&pattern),
        };
        let request = ExecRequest::new(target)
            .with_timeout(Workspace::timeout(options.build_timeout))
            .with_scratch(reading.profiles);
        let ran = execute::exec(&request, &context, cancel, &workspace.trace);
        drop(ran);
        match blocks_of(reading, target) {
            Some(measured) => {
                reached.instrumented.extend(measured.instrumented);
                reached.targets.insert(target.id.clone(), measured.covered);
            }
            None => reached
                .limitations
                .push(format!("{UNMEASURED}:{}", target.id)),
        }
    }
    reached
}

/// Every executable the build produced, test harnesses and plain binaries alike.
fn executables(messages: &[crate::cargo::Message]) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = messages
        .iter()
        .filter_map(|message| match message {
            crate::cargo::Message::CompilerArtifact(artifact) => artifact.executable.clone(),
            _ => None,
        })
        .collect();
    found.sort();
    found.dedup();
    found
}

/// A target's identity as one file name: the identity is a path of its own, and a profile is a file beside the others rather than a tree.
fn key(target: &execute::TestTarget) -> String {
    target.id.replace('/', "-")
}

/// What one target's run is read back with.
struct Reading<'a> {
    tools: &'a Tools,
    profiles: &'a Path,
    root: &'a Path,
    /// Every binary the coverage build produced, because a test that spawns one of them writes its counters into the same profile.
    binaries: &'a [PathBuf],
    watch: &'a Watched<'a>,
}

/// What one target's run said: the blocks it executed, and every block its binary carries.
struct Measured {
    covered: BTreeSet<Block>,
    instrumented: BTreeSet<Block>,
}

/// What one target's processes covered, or nothing when the tools could not say.
fn blocks_of(reading: &Reading<'_>, target: &execute::TestTarget) -> Option<Measured> {
    let Reading {
        tools,
        profiles,
        root,
        watch,
        ..
    } = *reading;
    let raw = written_profiles(profiles, &key(target)).ok()?;
    if raw.is_empty() {
        return None;
    }
    let merged = profiles.join(format!("{}.profdata", key(target)));
    tools.merge(&raw, &merged, watch).ok()?;
    let files = relative(tools.export(&merged, reading.binaries, watch).ok()?, root);
    Some(Measured {
        covered: covered(&files),
        instrumented: instrumented(&files),
    })
}

/// The export's file paths, made relative to the tree, so a block can be compared with a mutant's path.
fn relative(
    files: Vec<crate::coverage::FileRegions>,
    root: &Path,
) -> Vec<crate::coverage::FileRegions> {
    files
        .into_iter()
        .map(|mut file| {
            if let Ok(stripped) = file.path.strip_prefix(root) {
                file.path = stripped.to_path_buf();
            }
            file
        })
        .collect()
}

/// A measurement that could not be made, stated rather than assumed away.
fn refused(limitation: &str, trace: &Recorder) -> Reached {
    trace.note("coverage", limitation);
    Reached {
        targets: BTreeMap::new(),
        instrumented: BTreeSet::new(),
        limitations: vec![limitation.to_owned()],
    }
}

/// The environment a coverage build adds: whatever the tree already compiles with, then the instrumentation.
fn instrumenting(base: &[(OsString, OsString)], flags: &Configured) -> Vec<(OsString, OsString)> {
    let encoded = config::encoded(base, flags, &[INSTRUMENT]).unwrap_or_default();
    vec![
        (OsString::from(ENCODED_RUSTFLAGS), encoded),
        (OsString::from(RUSTFLAGS), OsString::new()),
    ]
}

/// The flag that instruments every region, spelled without a space so it survives every form of the variable.
const INSTRUMENT: &str = "-Cinstrument-coverage";

/// Where the measurement's own build and profiles live, for a caller that sweeps.
#[must_use]
pub fn directory(target_dir: &Path) -> PathBuf {
    target_dir.join("coverage")
}

/// A measurement an earlier run of the same tree already made.
///
/// What a coverage measurement establishes is a function of three things and
/// nothing else: the sources every unit compiled, the flags and dependencies
/// the manifests chose, and the toolchain that compiled it. None of them
/// changes because a mutation was written, so a tree measured yesterday and
/// unchanged today has already been measured — and the measurement is the most
/// expensive thing a run does, because instrumenting for coverage changes the
/// fingerprint of every crate and rebuilds the whole graph.
///
/// Remembering it is therefore the largest single piece of work a run can
/// remove, and the claim it rests on is the one the outcome store already
/// rests on: nothing that could change the answer changed.
///
/// Nothing here ever fails a run. A measurement that cannot be read is one the
/// run makes again, which is what it would have done anyway.
pub mod remembered {
    use std::path::{Path, PathBuf};

    use super::Reached;

    /// The directory remembered measurements live in, below the caller's cache directory.
    pub const LAYOUT: &str = "rust-mutants/measurements-v1";

    /// Bumped when what a measurement holds changes, or when a release finds a reason not to trust one written before it.
    ///
    /// Two: a measurement cut short used to be remembered as if it were whole,
    /// and a run that read one back routed away targets nobody had measured.
    pub const ABI: u32 = 2;

    /// Everything a measurement is a function of.
    #[derive(Debug, Clone, Copy)]
    pub struct Of<'a> {
        /// The digest of the pristine sources every unit of the build compiled.
        pub closure: &'a str,
        /// The digest of the manifests, the lock file, and the cargo configuration.
        pub manifests: &'a str,
        /// The toolchain that compiled it.
        pub toolchain: &'a str,
        /// The cargo arguments the tree was compiled with.
        pub build: &'a [String],
    }

    /// Where measurements of one tree are remembered, and under what name.
    #[derive(Debug, Clone)]
    pub struct Remembering {
        /// The directory to read and write under.
        pub directory: PathBuf,
        /// Everything the measurement is a function of, folded into one name.
        pub key: String,
    }

    impl Remembering {
        /// The name one measurement is filed under.
        #[must_use]
        pub fn of(directory: &Path, measured: &Of<'_>) -> Self {
            let Of {
                closure,
                manifests,
                toolchain,
                build,
            } = measured;
            let mut text = format!("{LAYOUT}\0{ABI}\0{closure}\0{manifests}\0{toolchain}");
            for argument in *build {
                text.push('\0');
                text.push_str(argument);
            }
            Self {
                directory: directory.to_path_buf(),
                key: crate::id::digest(text.as_bytes()),
            }
        }

        /// The file the measurement sits in.
        #[must_use]
        pub fn path(&self) -> PathBuf {
            self.directory.join(format!("{}.json", self.key))
        }

        /// What an earlier run of this exact tree measured, when one did and it still reads.
        #[must_use]
        pub fn read(&self) -> Option<Reached> {
            let text = std::fs::read_to_string(self.path()).ok()?;
            serde_json::from_str(&text).ok()
        }

        /// Remembers a measurement for the next run of this tree, and says nothing when it cannot.
        pub fn write(&self, reached: &Reached) {
            let Ok(text) = serde_json::to_string(reached) else {
                return;
            };
            if std::fs::create_dir_all(&self.directory).is_err() {
                return;
            }
            let temporary = self.directory.join(format!("{}.writing", self.key));
            if std::fs::write(&temporary, text.as_bytes()).is_err() {
                let _removed = std::fs::remove_file(&temporary);
                return;
            }
            if std::fs::rename(&temporary, self.path()).is_err() {
                let _removed = std::fs::remove_file(&temporary);
            }
        }
    }
}
