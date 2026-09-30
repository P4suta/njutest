// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Serialized builds of const item mutants, retaining the original tree and controls.

use std::collections::BTreeSet;
use std::sync::Mutex;

use super::{Compiled, PrepareOptions, Session};
use crate::EngineError;
use crate::runner::Cancel;

/// The compile-time catalog and its build options, with one build or execution owning the scratch outputs at a time.
#[derive(Debug)]
pub(super) struct Compiler {
    pub(super) indices: BTreeSet<u32>,
    pub(super) options: PrepareOptions,
    pub(super) lock: Mutex<()>,
}

impl Compiler {
    /// The build owner for every accepted initializer selector placed in this preparation.
    pub(super) fn of(
        placements: &std::collections::BTreeMap<String, Vec<crate::instrument::Placement>>,
        validated: &crate::validate::Validated,
        options: &PrepareOptions,
    ) -> Self {
        Self {
            indices: placements
                .values()
                .flat_map(|placements| placements.iter())
                .filter(|placement| {
                    placement.hint.form == crate::syntax::Form::B
                        && validated.accepted.contains(&placement.index)
                })
                .map(|placement| placement.index)
                .collect(),
            options: options.clone(),
            lock: Mutex::new(()),
        }
    }
}

impl Session {
    /// Whether this accepted index changes a const item and therefore requires a separate compilation.
    #[must_use]
    pub fn compiled_item(&self, index: u32) -> bool {
        self.compile_time.indices.contains(&index)
    }

    /// The selected targets of the current compilation, preserving their original identities.
    pub(super) fn compiled_targets<'session>(
        &'session self,
        request: &super::Request,
        compiled: Option<&'session Compiled<'_>>,
    ) -> Result<Vec<&'session crate::execute::TestTarget>, EngineError> {
        let targets = self.selected(request.target.as_deref())?;
        match compiled {
            None => Ok(targets),
            Some(compiled) => targets
                .into_iter()
                .map(|target| {
                    compiled
                        .targets
                        .iter()
                        .find(|built| built.id() == target.id())
                        .ok_or_else(|| {
                            crate::workspace::SessionError::NoTargets {
                                packages: vec![target.package().to_owned()],
                            }
                            .into()
                        })
                })
                .collect(),
        }
    }

    /// Builds this const item alone, serializing every build and use of the two reused output directories.
    pub(crate) fn compiled(
        &self,
        index: u32,
        cancel: &Cancel,
    ) -> Result<Option<Compiled<'_>>, EngineError> {
        if !self.compiled_item(index) {
            return Ok(None);
        }
        let owning = self.compile_time.lock.lock().map_err(|_poisoned| {
            crate::validate::ValidateError::AttemptFailed {
                message: "a panic poisoned the compile-time build owner".to_owned(),
            }
        })?;
        let (targets, sealed) =
            super::prepare::compiled_build(self, index, &self.compile_time.options, cancel)?;
        let apparatus = crate::apparatus::Apparatus::survey(
            targets.iter().map(|target| target.executable.as_path()),
            self.workspace.target_dir(),
        );
        Ok(Some(Compiled {
            targets,
            sealed,
            apparatus,
            _owning: owning,
        }))
    }
}
