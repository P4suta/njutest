// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a real compiler refuses, mutant by mutant, in its own words.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use mjutest_devkit::fixture::copy_tree;
use rust_mutants::cargo::{
    CompileKind, CompileOptions, Driver, LocateOptions, Metadata, MetadataOptions, Toolchain,
    compile,
};
use rust_mutants::catalog::Catalog;
use rust_mutants::instrument::{Instrumenting, Placement, instrument_file, plan_file};
use rust_mutants::rule::Tier;
use rust_mutants::runner::Cancel;
use rust_mutants::syntax::Selection;
use rust_mutants::trace::Recorder;
use rust_mutants::validate::{Attempt, Compile, ValidateError, ValidateOptions, validate};

/// Instruments a copy of a fixture and compiles it with a real cargo.
struct CargoScripted {
    root: PathBuf,
    sources: BTreeMap<String, Vec<u8>>,
    placements: BTreeMap<String, Vec<Placement>>,
    catalog: Catalog,
    toolchain: Toolchain,
    target: PathBuf,
    _dir: tempfile::TempDir,
    _target: tempfile::TempDir,
}

impl Compile for CargoScripted {
    fn attempt(&mut self, condemned: &BTreeSet<u32>) -> Result<Attempt, ValidateError> {
        let mut files = Vec::new();
        for (path, placements) in &self.placements {
            let kept: Vec<Placement> = placements
                .iter()
                .filter(|placement| !condemned.contains(&placement.index))
                .cloned()
                .collect();
            let source = &self.sources[path];
            let file = instrument_file(&Instrumenting {
                path,
                source,
                placements: &kept,
                markers: &[],
                comparable: &BTreeSet::default(),
                probed: &BTreeMap::default(),
                catalog_digest: self.catalog.digest(),
            })
            .map_err(ValidateError::from)?;
            std::fs::write(self.root.join(path), &file.text).map_err(|error| {
                ValidateError::AttemptFailed {
                    message: format!("cannot write {path}: {error}"),
                }
            })?;
            files.push(file);
        }
        let cancel = Cancel::new();
        let trace = Recorder::disabled();
        let checked = compile(
            &Driver {
                toolchain: &self.toolchain,
                dir: &self.root,
                cancel: &cancel,
                trace: &trace,
            },
            &CompileOptions {
                kind: CompileKind::Tests,
                packages: Vec::new(),
                target_dir: Some(self.target.clone()),
                locked: true,
                offline: true,
                timeout: None,
                env: Vec::new(),
                build: rust_mutants::cargo::BuildConfig::default(),
            },
        )
        .map_err(ValidateError::from)?;
        let written = u32::try_from(files.len()).unwrap_or(u32::MAX);
        Ok(Attempt {
            files,
            messages: checked.messages,
            success: checked.success,
            written,
        })
    }
}

static REGISTRY: rust_mutants::rule::Registry = rust_mutants::rule::Registry::canonical();

fn options() -> ValidateOptions {
    ValidateOptions::default()
}

fn prepare_fixture(name: &str) -> CargoScripted {
    let dir = tempfile::Builder::new()
        .prefix("rust-mutants-validate-")
        .tempdir()
        .expect("tempdir");
    let root = dir.path().join(name);
    copy_tree(&mjutest_devkit::paths::fixtures_dir().join(name), &root);
    let target = tempfile::Builder::new()
        .prefix("rust-mutants-validate-target-")
        .tempdir()
        .expect("tempdir");
    let cancel = Cancel::new();
    let trace = Recorder::disabled();
    let toolchain = Toolchain::locate(
        &LocateOptions {
            cargo: Some(mjutest_devkit::paths::cargo_binary()),
            ..LocateOptions::default()
        },
        &root,
        &cancel,
    )
    .expect("locate");
    let driver = Driver {
        toolchain: &toolchain,
        dir: &root,
        cancel: &cancel,
        trace: &trace,
    };
    let metadata = Metadata::load(
        &driver,
        MetadataOptions {
            locked: true,
            offline: true,
        },
    )
    .expect("metadata");
    let checked = compile(
        &driver,
        &CompileOptions {
            kind: CompileKind::Check,
            packages: Vec::new(),
            target_dir: Some(target.path().to_path_buf()),
            locked: true,
            offline: true,
            timeout: None,
            env: Vec::new(),
            build: rust_mutants::cargo::BuildConfig::default(),
        },
    )
    .expect("check");
    let discovery = rust_mutants::discover::discover(
        &rust_mutants::discover::Input {
            root: &root,
            metadata: &metadata,
            units: &checked.units,
        },
        &rust_mutants::discover::DiscoverOptions {
            selection: Selection::tier(&REGISTRY, Tier::All),
            include: Vec::new(),
            exclude: Vec::new(),
            packages: Vec::new(),
            skips: Vec::new(),
        },
        &trace,
    )
    .expect("discover");
    let found: Vec<rust_mutants::syntax::Found> = discovery
        .candidates
        .iter()
        .map(|located| located.found.clone())
        .collect();
    let mut sources = BTreeMap::new();
    let mut placements = BTreeMap::new();
    for file in &discovery.files {
        if file.candidates == 0 {
            continue;
        }
        sources.insert(
            file.path.clone(),
            std::fs::read(root.join(&file.path)).expect("read"),
        );
        placements.insert(
            file.path.clone(),
            plan_file(&discovery.catalog, &file.path, &found).expect("plan"),
        );
    }
    CargoScripted {
        root,
        sources,
        placements,
        catalog: discovery.catalog,
        toolchain,
        target: target.path().to_path_buf(),
        _dir: dir,
        _target: target,
    }
}
#[test]
fn the_compiler_decides_which_mutants_are_real_and_says_why_for_each() {
    let mut fixture = prepare_fixture("fixture-rejectable");
    let catalog = fixture.catalog.clone();
    let validated = validate(
        &catalog,
        &mut fixture,
        &rust_mutants::validate::Validating {
            options: options(),
            cancel: &Cancel::new(),
            trace: &Recorder::disabled(),
        },
    )
    .expect("validate");

    let mut rejected: Vec<(&str, &str)> = validated
        .rejections
        .iter()
        .map(|rejection| {
            (
                catalog
                    .by_index(rejection.index)
                    .expect("mutant")
                    .candidate
                    .rule
                    .name,
                rejection.code.as_deref().unwrap_or(""),
            )
        })
        .collect();
    rejected.sort_unstable();
    assert_eq!(
        rejected,
        [
            ("add-to-sub", "E0369"),
            ("mul-to-div", "unconditional_panic"),
            ("range-to-inclusive", "E0308"),
            ("return-default", "E0277"),
        ],
        "{:?}",
        validated
            .rejections
            .iter()
            .map(|r| r.diagnostic.lines().next().unwrap_or(""))
            .collect::<Vec<_>>()
    );
    assert!(
        validated
            .rejections
            .iter()
            .any(|rejection| rejection.diagnostic.contains("cannot subtract")),
        "the compiler's own words are attached"
    );
    assert_eq!(
        validated.accepted.len() + validated.rejections.len(),
        catalog.len(),
        "every mutant is accounted for"
    );
    assert!(validated.rounds >= 2);

    let final_attempt = fixture
        .attempt(&validated.rejections.iter().map(|r| r.index).collect())
        .expect("attempt");
    assert!(final_attempt.success, "the accepted tree compiles");
}
