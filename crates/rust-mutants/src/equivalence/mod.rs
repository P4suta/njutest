// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether the compiler renders a mutation identically to the program it mutates.

pub mod artifacts;

use std::path::Path;
use std::time::Duration;

use crate::EngineError;
use crate::cargo::{CompileKind, CompileOptions, compile};
use crate::catalog::Candidate;
use crate::execute::targets_of;
use crate::runner::Cancel;
use crate::splice::{Splice, apply};
use crate::trace::Recorder;
use crate::workspace::{OpenOptions, Workspace};

pub use artifacts::{Artifacts, Identity, Recompiled};

/// The reason a mutation whose file this tree does not hold establishes nothing.
pub const NO_SUCH_FILE: &str = "the tree holds no file the mutation is in";

/// The reason a build that did not succeed establishes nothing.
pub const DID_NOT_BUILD: &str =
    "the original tree did not build, so there are not two programs to compare";

/// The reason a mutation the compiler refuses establishes nothing about equivalence.
pub const DOES_NOT_BUILD: &str =
    "the mutated tree does not build, so there are not two programs to compare";

/// The reason a control that stopped matching withdraws the layer.
pub const CONTROL_DRIFTED: &str = "the original tree stopped building to the bytes it built to, so nothing here compares two programs";

/// The reason a mutated build whose spliced unit cargo reused establishes nothing.
pub const NOT_RECOMPILED: &str = "cargo reused the artifact of a unit that read the spliced file, so what was compared was built before the splice";

/// The reason a mutated build no unit of which read the spliced file establishes nothing.
pub const SPLICE_UNREAD: &str =
    "no unit of the mutated build read the spliced file, so nothing compared was compiled from it";

/// The reason a control whose restored unit cargo reused withdraws the layer.
pub const CONTROL_NOT_RECOMPILED: &str = "cargo reused the artifact of a unit that read the restored file, so the control compared what the mutated build left and says nothing of how the tree builds";

/// The variable that takes the build history out of what a build emits.
const WHOLE_BUILDS: (&str, &str) = ("CARGO_INCREMENTAL", "0");

/// What to prove equivalence with: how to open a tree of this layer's own, and how long one build may take.
#[derive(Debug, Clone, Default)]
pub struct ProveOptions {
    /// How the tree is copied and which cargo builds it.
    pub open: OpenOptions,
    /// How long one build may take.
    pub timeout: Option<Duration>,
    /// What the project is compiled as, which is a parameter of the question rather than of the answer.
    pub build: crate::cargo::BuildConfig,
}

/// A tree of its own, built once, and asked one mutation at a time whether the compiler renders it identically.
#[derive(Debug)]
pub struct Prover {
    workspace: Workspace,
    original: Option<Artifacts>,
    settled: bool,
    withdrawn: Option<&'static str>,
    options: ProveOptions,
}

/// What one build of the tree produced: the executables, and every unit with whether cargo compiled it.
struct Built {
    artifacts: Artifacts,
    units: Vec<crate::cargo::Unit>,
}

impl Prover {
    /// Copies the tree, builds it, and remembers what it built to.
    ///
    /// # Errors
    /// Whatever stopped the copy or the build.
    pub fn open(
        root: &Path,
        options: &ProveOptions,
        cancel: &Cancel,
        trace: &Recorder,
    ) -> Result<Self, EngineError> {
        let mut open = options.open.clone();
        open.trace = trace.clone();
        open.env.set(WHOLE_BUILDS.0, WHOLE_BUILDS.1);
        let workspace = Workspace::open(root, open, cancel)?;
        let mut prover = Self {
            workspace,
            original: None,
            settled: false,
            withdrawn: None,
            options: options.clone(),
        };
        prover.original = prover.build(cancel)?.map(|built| built.artifacts);
        Ok(prover)
    }

    /// Whether the compiler renders `candidate` identically to what it mutates.
    ///
    /// # Errors
    /// Whatever stopped a build or a write.
    pub fn identical(
        &mut self,
        candidate: &Candidate,
        cancel: &Cancel,
    ) -> Result<Identity, EngineError> {
        if let Some(why) = self.withdrawn {
            return Ok(Identity::NotEstablished(why));
        }
        let Some(original) = self.original.as_ref() else {
            return Ok(Identity::NotEstablished(DID_NOT_BUILD));
        };
        let path = self.workspace.snapshot_root().join(&candidate.path);
        let Ok(source) = std::fs::read(&path) else {
            return Ok(Identity::NotEstablished(NO_SUCH_FILE));
        };
        let spliced = apply(
            &source,
            &[Splice {
                span: candidate.span,
                original: candidate.original.clone(),
                replacement: candidate.replacement.clone(),
            }],
        )
        .map(|(bytes, _map)| bytes);
        let Ok(spliced) = spliced else {
            return Ok(Identity::NotEstablished(NO_SUCH_FILE));
        };
        if std::fs::write(&path, &spliced).is_err() {
            return Ok(Identity::NotEstablished(NO_SUCH_FILE));
        }
        let mutated = self.build(cancel);
        if std::fs::write(&path, &source).is_err() {
            return Ok(self.withdraw(CONTROL_DRIFTED));
        }
        let Some(mutated) = mutated? else {
            return Ok(Identity::NotEstablished(DOES_NOT_BUILD));
        };
        match artifacts::recompiled(&mutated.units, &path) {
            Recompiled::Every => {}
            Recompiled::Reused => return Ok(Identity::NotEstablished(NOT_RECOMPILED)),
            Recompiled::Unread => return Ok(Identity::NotEstablished(SPLICE_UNREAD)),
        }
        let answer = artifacts::compare(original, &mutated.artifacts);
        if matches!(answer, Identity::NotEstablished(_))
            || (answer == Identity::Differs && self.settled)
        {
            return Ok(answer);
        }
        let Some(control) = self.build(cancel)? else {
            return Ok(self.withdraw(CONTROL_DRIFTED));
        };
        match artifacts::recompiled(&control.units, &path) {
            Recompiled::Every => {}
            Recompiled::Reused | Recompiled::Unread => {
                return Ok(self.withdraw(CONTROL_NOT_RECOMPILED));
            }
        }
        if &control.artifacts == original {
            self.settled = true;
            Ok(answer)
        } else {
            Ok(self.withdraw(CONTROL_DRIFTED))
        }
    }

    /// Withdraws every answer from here on for `why`, and answers this one with it.
    const fn withdraw(&mut self, why: &'static str) -> Identity {
        self.withdrawn = Some(why);
        Identity::NotEstablished(why)
    }

    /// Whether a control has already withdrawn this layer's answers.
    #[must_use]
    pub const fn withdrawn(&self) -> bool {
        self.withdrawn.is_some()
    }

    /// Removes the tree.
    ///
    /// # Errors
    /// Whatever stopped the removal.
    pub fn close(self) -> Result<(), EngineError> {
        let preserved_directories = self.workspace.close()?;
        for preserved_directory in preserved_directories {
            drop(preserved_directory);
        }
        Ok(())
    }

    /// What one build of the tree produced, or nothing when the tree did not build.
    fn build(&self, cancel: &Cancel) -> Result<Option<Built>, EngineError> {
        let options = CompileOptions {
            kind: CompileKind::Tests,
            locked: self.options.open.locked,
            offline: self.options.open.offline,
            timeout: self.options.timeout,
            build: self.options.build.clone(),
            ..CompileOptions::new(self.workspace.build_dir().nested("equivalence"))
        };
        let built = compile(&self.workspace.driver(cancel), &options)?;
        match built.completion() {
            crate::cargo::Completion::Built => {}
            crate::cargo::Completion::Refused => return Ok(None),
        }
        let targets = targets_of(
            &built.messages,
            &self.workspace.metadata().packages,
            options.target_dir.path(),
        )?;
        let executables: Vec<(&str, &Path)> = targets
            .iter()
            .map(|target| (target.id(), target.executable.as_path()))
            .collect();
        Ok(Some(Built {
            artifacts: artifacts::digests(executables)?,
            units: built.units,
        }))
    }
}
