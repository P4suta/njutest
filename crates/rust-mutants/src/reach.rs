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
use crate::cargo::{CompileKind, CompileOptions, compile};
use crate::coverage::{
    Block, Point, Tools, covered, instrumented, profile_pattern, written_profiles,
};
use crate::execute::{self, Context, ExecRequest};
use crate::runner::{Cancel, Watched};
use crate::session::PrepareOptions;
use crate::trace::Recorder;
use crate::workspace::{SessionError, Workspace};

/// The limitation a session states when the tree could not be built with instrumentation.
pub const UNBUILDABLE: &str = "coverage-build-failed";

/// The limitation a session states when the LLVM tools are not installed.
pub const TOOLS_MISSING: &str = "coverage-tools-missing";

/// The limitation a session states when the tools ran and said nothing usable.
pub const UNMEASURED: &str = "coverage-not-measured";

/// The limitation a session states when the project configures its own compiler flags, which a coverage build would have to replace.
pub const CONFIGURED_FLAGS: &str = "coverage-refused-configured-rustflags";

/// The variable a coverage build's flags are put in, which is the encoded form so a value with a space cannot become two flags.
const ENCODED_RUSTFLAGS: &str = "CARGO_ENCODED_RUSTFLAGS";

/// The plain form, which cargo ignores when the encoded one is set.
const RUSTFLAGS: &str = "RUSTFLAGS";

/// What separates arguments inside the encoded form.
const SEPARATOR: char = '\u{1f}';

/// What each target reached, and what the measurement could not establish.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
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
    if configures_flags(root) {
        return Ok(refused(CONFIGURED_FLAGS, trace));
    }
    let target_dir = workspace.target_dir.join("coverage");
    let built = compile(
        &workspace.driver(cancel),
        &CompileOptions {
            kind: CompileKind::Tests,
            packages: Vec::new(),
            target_dir: Some(target_dir.clone()),
            locked: workspace.locked,
            offline: workspace.offline,
            timeout: Workspace::timeout(options.build_timeout),
            env: instrumenting(&workspace.base_env),
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
fn run_targets(
    reading: &Reading<'_>,
    targets: &[execute::TestTarget],
    within: (&Workspace, &PrepareOptions),
    cancel: &Cancel,
) -> Reached {
    let (workspace, options) = within;
    let mut reached = Reached::default();
    for target in targets {
        if cancel.is_cancelled() {
            break;
        }
        let pattern = profile_pattern(reading.profiles, &key(target));
        let context = Context {
            base_env: &workspace.base_env,
            cargo: Some(workspace.toolchain.cargo()),
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

/// Whether the tree configures its own compiler flags, which the coverage build would have to replace and cannot merge without deciding which of cargo's tables apply.
fn configures_flags(root: &Path) -> bool {
    for name in ["config.toml", "config"] {
        let path = root.join(".cargo").join(name);
        if std::fs::read_to_string(&path).is_ok_and(|text| text.contains("rustflags")) {
            return true;
        }
    }
    false
}

/// The environment a coverage build adds: the caller's own flags, then the instrumentation.
fn instrumenting(base: &[(OsString, OsString)]) -> Vec<(OsString, OsString)> {
    let mut flags: Vec<String> = Vec::new();
    if let Some((_, encoded)) = base.iter().find(|(name, _)| name == ENCODED_RUSTFLAGS) {
        flags.extend(
            encoded
                .to_string_lossy()
                .split(SEPARATOR)
                .filter(|flag| !flag.is_empty())
                .map(str::to_owned),
        );
    } else if let Some((_, plain)) = base.iter().find(|(name, _)| name == RUSTFLAGS) {
        flags.extend(
            plain
                .to_string_lossy()
                .split_whitespace()
                .map(str::to_owned),
        );
    }
    flags.push(INSTRUMENT.to_owned());
    vec![
        (
            OsString::from(ENCODED_RUSTFLAGS),
            OsString::from(flags.join(&SEPARATOR.to_string())),
        ),
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
