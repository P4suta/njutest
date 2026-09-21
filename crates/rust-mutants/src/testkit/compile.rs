// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A compiler that instruments one file for real and decides the outcome from a script.

#![expect(
    clippy::panic,
    reason = "a fake reports a broken test by panicking: a script that does not discover, \
              catalog, or plan is a test with nothing left to assert, and returning an error \
              here would only move the panic to the call site"
)]

use std::collections::{BTreeMap, BTreeSet};

use crate::cargo::{CargoError, CargoErrorKind, Message};
use crate::catalog::{Builder, Catalog};
use crate::instrument::{Placement, instrument_file, plan_file};
use crate::rule::{Registry, Tier};
use crate::runner::Cancel;
use crate::syntax::{Selection, discover_file};
use crate::validate::{Attempt, Compile, ValidateError};

static REGISTRY: Registry = Registry::canonical();

/// A [`Compile`] that instruments one file for real and answers from a script: which mutants the compiler refuses, and whether the diagnostic that refuses each one lands inside its branch.
#[derive(Debug)]
pub struct ScriptedCompile {
    path: String,
    source: Vec<u8>,
    placements: Vec<Placement>,
    catalog: Catalog,
    attributable: BTreeSet<u32>,
    unattributable: BTreeSet<u32>,
    attempts: Vec<BTreeSet<u32>>,
    cancelling: Option<(usize, Cancel)>,
    interacting: BTreeSet<u32>,
}

impl ScriptedCompile {
    /// Discovers `source` at `path` under `tier` and plans its instrumentation, refusing nothing until [`ScriptedCompile::refusing`] says so.
    ///
    /// # Panics
    /// When the source does not discover, catalog, or plan, which is a broken
    /// test rather than a fact about the engine.
    #[must_use]
    pub fn from_source(path: &str, source: &str, tier: Tier) -> Self {
        let selection = Selection::tier(&REGISTRY, tier);
        let discovery = discover_file(path, source.as_bytes(), &selection)
            .unwrap_or_else(|error| panic!("{path} discovers: {error}"));
        let mut builder = Builder::new();
        for found in &discovery.candidates {
            builder
                .add(found.candidate.clone())
                .unwrap_or_else(|error| panic!("{path} catalogs: {error}"));
        }
        let catalog = builder
            .build()
            .unwrap_or_else(|error| panic!("{path} catalogs: {error}"));
        let placements = plan_file(&catalog, path, &discovery.candidates)
            .unwrap_or_else(|error| panic!("{path} plans: {error}"));
        Self {
            path: path.to_owned(),
            source: source.as_bytes().to_vec(),
            placements,
            catalog,
            attributable: BTreeSet::new(),
            unattributable: BTreeSet::new(),
            attempts: Vec::new(),
            cancelling: None,
            interacting: BTreeSet::new(),
        }
    }

    /// Refuses the tree only while every mutant of `together` is live, with a diagnostic inside no branch.
    #[must_use]
    pub fn interacting(mut self, together: &[u32]) -> Self {
        self.interacting = together.iter().copied().collect();
        self
    }

    /// Cancels `cancel` during the `nth` attempt, in the middle of the compilation rather than between two of them.
    #[must_use]
    pub fn cancelling_at(mut self, nth: usize, cancel: &Cancel) -> Self {
        self.cancelling = Some((nth, cancel.clone()));
        self
    }

    /// Refuses the mutants of `attributable` with a diagnostic inside each one's own branch, and those of `unattributable` with one that lands outside every branch.
    #[must_use]
    pub fn refusing(mut self, attributable: &[u32], unattributable: &[u32]) -> Self {
        self.attributable = attributable.iter().copied().collect();
        self.unattributable = unattributable.iter().copied().collect();
        self
    }

    /// The catalog the source yielded.
    #[must_use]
    pub const fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// The bytes the instrumentation is spliced into.
    #[must_use]
    pub fn source(&self) -> &[u8] {
        &self.source
    }

    /// Every mutant's place in the file, as instrumentation would use it.
    #[must_use]
    pub fn placements(&self) -> &[Placement] {
        &self.placements
    }

    /// What each attempt condemned, in the order validation asked, which is how a test says what the loop cost.
    #[must_use]
    pub fn attempts(&self) -> &[BTreeSet<u32>] {
        &self.attempts
    }
}

impl Compile for ScriptedCompile {
    fn attempt(&mut self, condemned: &BTreeSet<u32>) -> Result<Attempt, ValidateError> {
        self.attempts.push(condemned.clone());
        if let Some((nth, cancel)) = &self.cancelling
            && self.attempts.len() >= *nth
        {
            cancel.cancel();
            return Err(ValidateError::Cargo(CargoError::new(
                CargoErrorKind::Cancelled,
                "the compilation was cancelled",
            )));
        }
        let kept: Vec<Placement> = self
            .placements
            .iter()
            .filter(|placement| !condemned.contains(&placement.index))
            .cloned()
            .collect();
        let file = instrument_file(&crate::instrument::Instrumenting {
            path: &self.path,
            source: &self.source,
            placements: &kept,
            markers: &[],
            comparable: &BTreeSet::default(),
            probed: &BTreeMap::default(),
            catalog_digest: self.catalog.digest(),
        })?;
        let live: BTreeSet<u32> = kept.iter().map(|placement| placement.index).collect();
        let mut messages = Vec::new();
        for index in self.attributable.intersection(&live) {
            let branch = file
                .branches
                .iter()
                .find(|branch| branch.index == *index)
                .ok_or_else(|| ValidateError::AttemptFailed {
                    message: format!("no branch for live mutant {index}"),
                })?;
            messages.push(diagnostic_at(
                &self.path,
                branch.span.start,
                branch.span.end,
                *index,
            ));
        }
        for index in self.unattributable.intersection(&live) {
            messages.push(diagnostic_at(&self.path, 0, 1, *index));
        }
        if !self.interacting.is_empty() && self.interacting.is_subset(&live) {
            messages.push(diagnostic_at(&self.path, 0, 1, u32::MAX));
        }
        let success = messages.is_empty();
        messages.push(Message::BuildFinished { success });
        Ok(Attempt {
            files: vec![file],
            messages,
            success,
            written: 1,
        })
    }
}

/// A `compiler-message` whose primary span is somewhere nothing owns and whose secondary span covers `[start, end)` of `path`.
///
/// # Panics
/// When the message this composes does not parse, which is a broken testkit.
#[must_use]
pub fn diagnostic_beside(path: &str, start: u32, end: u32, index: u32) -> Message {
    let json = format!(
        r#"{{"reason":"compiler-message","package_id":"p","manifest_path":"/w/Cargo.toml","target":{{"kind":["lib"],"crate_types":["lib"],"name":"demo","src_path":"/w/src/lib.rs","edition":"2024"}},"message":{{"message":"mutant {index} does not compile","code":{{"code":"E0999","explanation":""}},"level":"error","spans":[{{"file_name":"{path}","byte_start":0,"byte_end":1,"line_start":1,"line_end":1,"column_start":1,"column_end":2,"is_primary":true,"text":[],"label":null}},{{"file_name":"{path}","byte_start":{start},"byte_end":{end},"line_start":1,"line_end":1,"column_start":1,"column_end":2,"is_primary":false,"text":[],"label":"expected because of this"}}],"children":[],"rendered":"error[E0999]: mutant {index} does not compile\n"}}}}"#
    );
    match crate::cargo::parse_messages(json.as_bytes()) {
        Ok(messages) => messages
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("the composed diagnostic has one message")),
        Err(error) => panic!("the composed diagnostic parses: {error}"),
    }
}

/// A `compiler-message` whose spans name nothing and whose child note covers `[start, end)` of `path`.
///
/// # Panics
/// When the message this composes does not parse, which is a broken testkit.
#[must_use]
pub fn diagnostic_noted(path: &str, start: u32, end: u32, index: u32) -> Message {
    let json = format!(
        r#"{{"reason":"compiler-message","package_id":"p","manifest_path":"/w/Cargo.toml","target":{{"kind":["lib"],"crate_types":["lib"],"name":"demo","src_path":"/w/src/lib.rs","edition":"2024"}},"message":{{"message":"mutant {index} does not compile","code":{{"code":"E0999","explanation":""}},"level":"error","spans":[{{"file_name":"{path}","byte_start":0,"byte_end":1,"line_start":1,"line_end":1,"column_start":1,"column_end":2,"is_primary":true,"text":[],"label":null}}],"children":[{{"message":"the size is not known","code":null,"level":"note","spans":[{{"file_name":"{path}","byte_start":{start},"byte_end":{end},"line_start":1,"line_end":1,"column_start":1,"column_end":2,"is_primary":true,"text":[],"label":null}}],"children":[],"rendered":null}}],"rendered":"error[E0999]: mutant {index} does not compile\n"}}}}"#
    );
    match crate::cargo::parse_messages(json.as_bytes()) {
        Ok(messages) => messages
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("the composed diagnostic has one message")),
        Err(error) => panic!("the composed diagnostic parses: {error}"),
    }
}

/// A `compiler-message` about mutant `index`, whose primary span covers `[start, end)` of `path`.
///
/// # Panics
/// When the message this composes does not parse, which is a broken testkit.
#[must_use]
#[expect(
    clippy::panic,
    reason = "a testkit constructor cannot return a fabricated compiler message when JSON string encoding fails"
)]
pub fn diagnostic_at(path: &str, start: u32, end: u32, index: u32) -> Message {
    let path = match serde_json::to_string(path) {
        Ok(path) => path,
        Err(error) => panic!("encoding a diagnostic path as JSON failed: {error}"),
    };
    let path = path.trim_matches('"');
    let json = format!(
        r#"{{"reason":"compiler-message","package_id":"p","manifest_path":"/w/Cargo.toml","target":{{"kind":["lib"],"crate_types":["lib"],"name":"demo","src_path":"/w/src/lib.rs","edition":"2024"}},"message":{{"message":"mutant {index} does not compile","code":{{"code":"E0999","explanation":""}},"level":"error","spans":[{{"file_name":"{path}","byte_start":{start},"byte_end":{end},"line_start":1,"line_end":1,"column_start":1,"column_end":2,"is_primary":true,"text":[],"label":null}}],"children":[],"rendered":"error[E0999]: mutant {index} does not compile\n"}}}}"#
    );
    match crate::cargo::parse_messages(json.as_bytes()) {
        Ok(messages) => messages
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("the composed diagnostic has one message")),
        Err(error) => panic!("the composed diagnostic parses: {error}"),
    }
}
