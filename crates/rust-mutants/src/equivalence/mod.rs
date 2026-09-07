// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether the compiler renders a mutation identically to the program it mutates.
//!
//! The lemma is one sentence: build the program, build it again with one
//! mutation spliced in, and if every executable is byte for byte the same file,
//! the two are the same program and no test can tell them apart. It asks
//! nothing of the compiler — not that it be deterministic, and not that it be
//! correct — only that the same bytes behave the same way.
//!
//! What this module never says is *equivalent*. Absence and equivalence look
//! alike from here: a mutation of a function nothing calls is dropped by the
//! linker, and the artifacts come out identical for a reason that is the
//! opposite of reassuring. Saying `Identical` is the engine's whole claim, and
//! the premises that turn it into a verdict live where the evidence does
//! ([ADR 0013](../../../../docs/adr/0013-codegen-identity-is-the-equivalence-proof.md)).

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

pub use artifacts::{Artifacts, Identity};

/// The reason a mutation whose file this tree does not hold establishes nothing.
pub const NO_SUCH_FILE: &str = "the tree holds no file the mutation is in";

/// The reason a build that did not succeed establishes nothing.
pub const DID_NOT_BUILD: &str = "the tree with the mutation spliced in did not build";

/// The reason a mutation the compiler refuses establishes nothing about equivalence.
pub const DOES_NOT_BUILD: &str =
    "the mutated tree does not build, so there are not two programs to compare";

/// The reason a control that stopped matching withdraws the layer.
pub const CONTROL_DRIFTED: &str = "the original tree stopped building to the bytes it built to, so nothing here compares two programs";

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
///
/// The tree is the program the user wrote: nothing is instrumented in it, no
/// guard is spliced into it, and no runtime module is appended to it. What is
/// compared is what the project's own `cargo test --no-run` produces, under the
/// project's own test profile, because the profile is a parameter of the
/// question rather than of the answer: `x + 0` and `x - 0` are the same
/// instructions at `opt-level = 1` and different ones at `opt-level = 0`, and
/// the one the tests run is the one that decides.
#[derive(Debug)]
pub struct Prover {
    workspace: Workspace,
    original: Artifacts,
    withdrawn: bool,
    options: ProveOptions,
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
        let workspace = Workspace::open(root, open, cancel)?;
        let mut prover = Self {
            workspace,
            original: Artifacts::new(),
            withdrawn: false,
            options: options.clone(),
        };
        prover.original = prover.build(cancel)?.unwrap_or_default();
        Ok(prover)
    }

    /// Whether the compiler renders `candidate` identically to what it mutates.
    ///
    /// Every answer of [`Identity::Identical`] is followed by building the
    /// original again and checking that it still builds to the bytes it built
    /// to. A tree whose build is not reproducible proves nothing, and one
    /// answer that fails that check withdraws every answer this prover would
    /// give afterwards: a layer that cannot establish its premise keeps the
    /// execution.
    ///
    /// # Errors
    /// Whatever stopped a build or a write.
    pub fn identical(
        &mut self,
        candidate: &Candidate,
        cancel: &Cancel,
    ) -> Result<Identity, EngineError> {
        if self.withdrawn {
            return Ok(Identity::NotEstablished(CONTROL_DRIFTED));
        }
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
            self.withdrawn = true;
            return Ok(Identity::NotEstablished(CONTROL_DRIFTED));
        }
        let Some(mutated) = mutated? else {
            return Ok(Identity::NotEstablished(DOES_NOT_BUILD));
        };
        let answer = artifacts::compare(&self.original, &mutated);
        if answer != Identity::Identical {
            return Ok(answer);
        }
        let control = self.build(cancel)?.unwrap_or_default();
        if control == self.original {
            Ok(Identity::Identical)
        } else {
            self.withdrawn = true;
            Ok(Identity::NotEstablished(CONTROL_DRIFTED))
        }
    }

    /// Whether a control has already withdrawn this layer's answers.
    #[must_use]
    pub const fn withdrawn(&self) -> bool {
        self.withdrawn
    }

    /// Removes the tree.
    ///
    /// # Errors
    /// Whatever stopped the removal.
    pub fn close(self) -> Result<(), EngineError> {
        drop(self.workspace.close()?);
        Ok(())
    }

    /// What one build of the tree produced, or nothing when the tree did not build.
    ///
    /// A mutation the compiler refuses is not one it renders identically: the
    /// question is about two programs, and there is only one. Saying so is
    /// what keeps a build failure from reading as an empty set of artifacts
    /// equal to another empty set.
    fn build(&self, cancel: &Cancel) -> Result<Option<Artifacts>, EngineError> {
        let built = compile(
            &self.workspace.driver(cancel),
            &CompileOptions {
                kind: CompileKind::Tests,
                locked: self.options.open.locked,
                offline: self.options.open.offline,
                timeout: self.options.timeout,
                build: self.options.build.clone(),
                ..CompileOptions::default()
            },
        )?;
        if !built.success {
            return Ok(None);
        }
        let targets = targets_of(&built.messages, &self.workspace.metadata().packages, None);
        let executables: Vec<(&str, &Path)> = targets
            .iter()
            .map(|target| (target.id.as_str(), target.executable.as_path()))
            .collect();
        Ok(Some(artifacts::digests(executables).unwrap_or_default()))
    }
}
