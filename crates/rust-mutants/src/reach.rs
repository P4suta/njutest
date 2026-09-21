// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Which test targets could have observed a mutant at all.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::EngineError;
use crate::cargo::config::{Configured, ENCODED_RUSTFLAGS, RUSTFLAGS};
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

/// What each target reached, and what the measurement could not establish.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
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

/// The limitation a cargo configuration refuses a coverage measurement with, before a build is attempted.
#[must_use]
pub const fn refusal(flags: &Configured) -> Option<&'static str> {
    if flags.unreadable {
        return Some(UNREADABLE_CONFIGURATION);
    }
    if flags.target_specific {
        return Some(CONFIGURED_FLAGS);
    }
    None
}

fn measure(
    workspace: &Workspace,
    options: &PrepareOptions,
    cancel: &Cancel,
    trace: &Recorder,
) -> Result<Reached, EngineError> {
    let root = workspace.snapshot_root();
    let flags = config::configured(root, config::home(&workspace.base_env).as_deref());
    if let Some(named) = refusal(&flags) {
        return Ok(refused(named, trace));
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
            env: instrumenting(&workspace.base_env, &flags)?,
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
    )?;
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
            touch: None,
            steps: None,
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
    let found: BTreeSet<PathBuf> = messages
        .iter()
        .filter_map(|message| match message {
            crate::cargo::Message::CompilerArtifact(artifact) => artifact.executable.clone(),
            crate::cargo::Message::CompilerMessage(_)
            | crate::cargo::Message::BuildScriptExecuted(_)
            | crate::cargo::Message::BuildFinished { .. }
            | crate::cargo::Message::Other { .. } => None,
        })
        .collect();
    found.into_iter().collect()
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
    let raw = match written_profiles(profiles, &key(target)) {
        Ok(raw) => raw,
        Err(_) => return None,
    };
    if raw.is_empty() {
        return None;
    }
    let merged = profiles.join(format!("{}.profdata", key(target)));
    if tools.merge(&raw, &merged, watch).is_err() {
        return None;
    }
    let files = match tools.export(&merged, reading.binaries, watch) {
        Ok(files) => relative(files, root),
        Err(_) => return None,
    };
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
            match file.path.strip_prefix(root) {
                Ok(stripped) => file.path = stripped.to_path_buf(),
                Err(_coverage_path_is_outside_the_workspace) => {}
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
fn instrumenting(
    base: &[(OsString, OsString)],
    flags: &Configured,
) -> Result<Vec<(OsString, OsString)>, config::ConfigError> {
    let encoded = match config::encoded(base, flags, &[INSTRUMENT])? {
        Some(encoded) => encoded,
        None => OsString::new(),
    };
    Ok(vec![
        (OsString::from(ENCODED_RUSTFLAGS), encoded),
        (OsString::from(RUSTFLAGS), OsString::new()),
    ])
}

/// The flag that instruments every region, spelled without a space so it survives every form of the variable.
const INSTRUMENT: &str = "-Cinstrument-coverage";

/// Where the measurement's own build and profiles live, for a caller that sweeps.
#[must_use]
pub fn directory(target_dir: &Path) -> PathBuf {
    target_dir.join("coverage")
}

/// A measurement an earlier run of the same tree already made.
pub mod remembered {
    use std::path::{Path, PathBuf};

    use super::Reached;

    /// The directory remembered measurements live in, below the caller's cache directory.
    pub const LAYOUT: &str = "rust-mutants/measurements-v1";

    /// Bumped when what a measurement holds changes, or when a release finds a reason not to trust one written before it.
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
            let text = match std::fs::read_to_string(self.path()) {
                Ok(text) => text,
                Err(_) => return None,
            };
            match crate::strictjson::decode_str(&text) {
                Ok(reached) => Some(reached),
                Err(_) => None,
            }
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
                if let Err(cleanup_error) = std::fs::remove_file(&temporary) {
                    drop(cleanup_error);
                }
                return;
            }
            if std::fs::rename(&temporary, self.path()).is_err()
                && let Err(cleanup_error) = std::fs::remove_file(&temporary)
            {
                drop(cleanup_error);
            }
        }
    }
}
